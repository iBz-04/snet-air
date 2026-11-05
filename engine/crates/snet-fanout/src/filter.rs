use std::ops::Range;

use snet_protocol::{
    Citations, FailureKind, ProtocolError, holds_bracket, quote_range, spells_out_a_value,
};
use snet_pseudonym::{Pseudonymiser, TaskMapping};

use crate::messages::{JobReport, SentResult, local_result};
use crate::plan::{Material, PlannedJob};
use crate::run::Slot;

const MAX_FINDING_CHARACTERS: usize = 300;
const MAX_QUOTES: usize = 2;
const MAX_SENT_PER_JOB: usize = 16;
const MAX_SENT_FINDING_BYTES: usize = 32 * 1024;
const MAX_CITATIONS: usize = 990;

/// What became of one chunk's result, for its record.
pub(crate) struct ChunkOutcome {
    /// Per quote of the result: whether it was verified, and its citation
    /// when its result was sent.
    pub(crate) quotes: Vec<(bool, Option<u32>)>,
    pub(crate) outcome: &'static str,
}

/// The task's state that filtering reads and changes.
pub(crate) struct FilterState<'s> {
    pub(crate) mapping: &'s mut TaskMapping,
    pub(crate) citations: &'s mut Citations,
    pub(crate) verified_texts: &'s mut Vec<String>,
    pub(crate) sent_bytes: &'s mut usize,
}

struct Candidate {
    slot: usize,
    finding: snet_cloud::OutboundText,
    quote: (String, Range<usize>),
}

/// The quarter rule, then the discard rules and citations over a round whose requests
/// have all ended. Returns each job's report and each slot's outcome.
pub(crate) fn filter_round(
    slots: &[Slot],
    jobs: &[PlannedJob],
    material: &Material,
    documents: &[snet_protocol::Document],
    pseudonymiser: &Pseudonymiser<'_>,
    state: FilterState<'_>,
) -> Result<(Vec<JobReport>, Vec<ChunkOutcome>), ProtocolError> {
    let failed = slots
        .iter()
        .filter(|slot| !matches!(slot.result, Some(Ok(_))))
        .count();
    if failed * 4 > slots.len() {
        return Err(ProtocolError::new(
            FailureKind::Local,
            "more than a quarter of the round's local requests failed",
        ));
    }

    let chunk_text = |slot: &Slot| {
        let chunk = &material.chunks[slot.chunk];
        (chunk, &documents[chunk.document].text[chunk.range.clone()])
    };
    let mut verified: Vec<Vec<(String, Range<usize>)>> = Vec::with_capacity(slots.len());
    let mut results = Vec::with_capacity(slots.len());
    for slot in slots {
        let result = match &slot.result {
            Some(Ok(answer)) => local_result(&answer.object),
            _ => None,
        };
        let (chunk, text) = chunk_text(slot);
        let found: Vec<(String, Range<usize>)> = result
            .as_ref()
            .map(|result| {
                result
                    .quotes
                    .iter()
                    .filter_map(|quote| {
                        quote_range(text, quote).map(|range| {
                            let start = chunk.range.start + range.start;
                            (quote.clone(), start..start + quote.len())
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        state
            .verified_texts
            .extend(found.iter().map(|(quote, _)| quote.clone()));
        verified.push(found);
        results.push(result);
    }

    let mut outcomes: Vec<ChunkOutcome> = slots
        .iter()
        .map(|_| ChunkOutcome {
            quotes: Vec::new(),
            outcome: "failed",
        })
        .collect();
    let mut reports: Vec<JobReport> = jobs
        .iter()
        .map(|job| JobReport {
            chunks: job.chunks.len() as u32,
            failed: 0,
            discarded: [0; 6],
            held_back: 0,
            results: Vec::new(),
        })
        .collect();
    let mut candidates: Vec<Vec<Candidate>> = jobs.iter().map(|_| Vec::new()).collect();
    for (index, slot) in slots.iter().enumerate() {
        let report = &mut reports[slot.job];
        outcomes[index].quotes = results[index]
            .as_ref()
            .map(|result| {
                result
                    .quotes
                    .iter()
                    .map(|quote| (verified[index].iter().any(|(text, _)| text == quote), None))
                    .collect()
            })
            .unwrap_or_default();
        let Some(result) = &results[index] else {
            report.failed += 1;
            continue;
        };
        let reason = if !result.relevant {
            Some(0)
        } else if result.finding.is_empty()
            || result.finding.chars().count() > MAX_FINDING_CHARACTERS
            || result.quotes.len() > MAX_QUOTES
        {
            Some(1)
        } else if verified[index].is_empty() {
            Some(2)
        } else if state
            .verified_texts
            .iter()
            .any(|quote| result.finding.contains(quote.as_str()))
        {
            Some(3)
        } else if holds_bracket(&result.finding) {
            Some(4)
        } else {
            None
        };
        if let Some(reason) = reason {
            report.discarded[reason] += 1;
            outcomes[index].outcome = crate::messages::DISCARD_REASONS[reason];
            continue;
        }
        let finding = pseudonymiser
            .text(state.mapping, &result.finding)
            .map_err(|error| {
                ProtocolError::new(FailureKind::Pseudonymisation, error.to_string())
            })?;
        if spells_out_a_value(state.mapping, finding.as_str()) {
            report.discarded[5] += 1;
            outcomes[index].outcome = crate::messages::DISCARD_REASONS[5];
            continue;
        }
        candidates[slot.job].push(Candidate {
            slot: index,
            finding,
            quote: verified[index][0].clone(),
        });
    }

    for (job, job_candidates) in candidates.into_iter().enumerate() {
        let mut holding_back = false;
        for candidate in job_candidates {
            let slot = &slots[candidate.slot];
            let document = material.chunks[slot.chunk].document;
            let bytes = candidate.finding.as_str().len();
            let new_citation = state
                .citations
                .find_by(document, &candidate.quote.0)
                .is_none();
            let fits = reports[job].results.len() < MAX_SENT_PER_JOB
                && *state.sent_bytes + bytes <= MAX_SENT_FINDING_BYTES
                && state.citations.len() + usize::from(new_citation) <= MAX_CITATIONS;
            if holding_back || !fits {
                holding_back = true;
                reports[job].held_back += 1;
                outcomes[candidate.slot].outcome = "held_back";
                continue;
            }
            let (quote, range) = candidate.quote;
            let number = state.citations.cite(document, &quote, range);
            *state.sent_bytes += bytes;
            let position = results[candidate.slot]
                .as_ref()
                .and_then(|result| result.quotes.iter().position(|text| *text == quote));
            if let Some(entry) =
                position.and_then(|position| outcomes[candidate.slot].quotes.get_mut(position))
            {
                entry.1 = Some(number);
            }
            outcomes[candidate.slot].outcome = "sent";
            let chunk = &material.chunks[slot.chunk];
            reports[job].results.push(SentResult {
                document: chunk.document as u32 + 1,
                chunk: chunk.number as u32,
                finding: candidate.finding,
                citation: number,
            });
        }
    }
    Ok((reports, outcomes))
}
