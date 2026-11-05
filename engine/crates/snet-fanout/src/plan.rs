use std::collections::BTreeSet;
use std::ops::Range;

use snet_local::LocalModel;
use snet_protocol::{Citations, FailureKind, ProtocolError};
use snet_pseudonym::{Pseudonymiser, TaskMapping, fold_for_matching, on_word_boundaries};

use crate::chunks::escaped_len;
use crate::messages::{CloudStep, Step, local_request};

const MAX_CLOUD_TEXT_CHARACTERS: usize = 4_000;
const MAX_CITATIONS: usize = 32;
const MAX_JOBS: usize = 8;
const MAX_QUESTION_CHARACTERS: usize = 300;
const MAX_QUESTION_ESCAPED: usize = 1_200;
const MAX_TERMS: usize = 8;
const MAX_TERM_BYTES: usize = 64;

/// The material split into chunks, with each chunk's matching fold kept
/// for the task.
pub(crate) struct Material {
    pub(crate) chunks: Vec<Chunk>,
}

pub(crate) struct Chunk {
    pub(crate) document: usize,
    pub(crate) number: usize,
    pub(crate) range: Range<usize>,
    /// The chunk's matching fold.
    pub(crate) folded: String,
}

/// A checked job: its restored question and terms, and the chunks it runs
/// on, as positions in `Material::chunks`.
pub(crate) struct PlannedJob {
    pub(crate) question: String,
    pub(crate) terms: Vec<String>,
    pub(crate) chunks: Vec<usize>,
}

fn invalid(detail: impl Into<String>) -> ProtocolError {
    ProtocolError::new(FailureKind::InvalidCloudReply, detail)
}

/// The rules for `text`, `citations` and the presence of jobs.
pub(crate) fn check_step(step: &CloudStep) -> Result<(), ProtocolError> {
    let characters = step.text.chars().count();
    if characters == 0 || characters > MAX_CLOUD_TEXT_CHARACTERS || step.text.trim().is_empty() {
        return Err(invalid(
            "the reply's text is empty or over 4 000 characters",
        ));
    }
    match step.step {
        Step::Plan | Step::NotFound if !step.citations.is_empty() => {
            Err(invalid("a plan or not_found carries citations"))
        }
        Step::Answer | Step::NotFound if !step.jobs.is_empty() => {
            Err(invalid("an answer or not_found carries jobs"))
        }
        Step::Plan if step.jobs.is_empty() || step.jobs.len() > MAX_JOBS => {
            Err(invalid("a plan must hold 1 to 8 jobs"))
        }
        Step::Answer => {
            let mut seen = BTreeSet::new();
            let distinct = step.citations.iter().all(|citation| seen.insert(citation));
            if step.citations.is_empty() || step.citations.len() > MAX_CITATIONS || !distinct {
                return Err(invalid("an answer must cite 1 to 32 distinct citations"));
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// The citations of an answer, each issued in this task.
pub(crate) fn check_citations(
    step: &CloudStep,
    citations: &Citations,
) -> Result<(), ProtocolError> {
    if step
        .citations
        .iter()
        .all(|name| citations.find(name).is_some())
    {
        Ok(())
    } else {
        Err(invalid("an answer cites a citation not issued"))
    }
}

fn is_plain_term(term: &str) -> bool {
    if term.is_empty() || term.len() > MAX_TERM_BYTES {
        return false;
    }
    let mut previous_was_letter = false;
    for character in term.chars() {
        if character.is_alphabetic() {
            previous_was_letter = true;
        } else if (character == ' ' || character == '-') && previous_was_letter {
            previous_was_letter = false;
        } else {
            return false;
        }
    }
    previous_was_letter
}

/// A term is one token of the task, restored, or a plain term that
/// pseudonymisation leaves unchanged.
fn restored_term(
    term: &str,
    mapping: &TaskMapping,
    pseudonymiser: &Pseudonymiser<'_>,
) -> Result<String, ProtocolError> {
    if term.starts_with('⟦') && term.ends_with('⟧') {
        let restored = mapping
            .restore(term)
            .map_err(|error| invalid(error.to_string()))?;
        let whole = restored.ranges().len() == 1
            && restored.ranges()[0].start == 0
            && restored.ranges()[0].end == restored.as_str().len();
        return whole
            .then(|| restored.as_str().to_string())
            .ok_or_else(|| invalid("a term must be exactly one token"));
    }
    if !is_plain_term(term) {
        return Err(invalid("a plain term must be 1 to 64 bytes of letters"));
    }
    let checked = pseudonymiser
        .text(&mut TaskMapping::new(), term)
        .map_err(|error| invalid(error.to_string()))?;
    if checked.as_str() != term {
        return Err(invalid(
            "a plain term must be text pseudonymisation leaves alone",
        ));
    }
    Ok(term.to_string())
}

/// Whether a chunk's fold holds a term's fold on word boundaries.
fn chunk_matches(chunk: &Chunk, term: &str) -> bool {
    let folded_term = fold_for_matching(term);
    let text = chunk.folded.as_str();
    text.match_indices(folded_term.as_str())
        .any(|(start, found)| on_word_boundaries(text, start, start + found.len()))
}

/// The job rules for each job of a plan.
pub(crate) fn plan_jobs(
    step: &CloudStep,
    mapping: &TaskMapping,
    pseudonymiser: &Pseudonymiser<'_>,
    material: &Material,
    longest_chunk: &str,
    local: &LocalModel,
    output_tokens: u64,
) -> Result<Vec<PlannedJob>, ProtocolError> {
    let mut planned = Vec::with_capacity(step.jobs.len());
    for job in &step.jobs {
        let characters = job.question.chars().count();
        if characters == 0 || characters > MAX_QUESTION_CHARACTERS || job.question.trim().is_empty()
        {
            return Err(invalid("a job question must be 1 to 300 characters"));
        }
        if job.terms.len() > MAX_TERMS {
            return Err(invalid("a job holds more than 8 terms"));
        }
        let question = mapping
            .restore(&job.question)
            .map_err(|error| invalid(error.to_string()))?;
        if escaped_len(question.as_str()) > MAX_QUESTION_ESCAPED {
            return Err(invalid("a restored job question is over 1 200 bytes"));
        }
        let probe =
            local_request(longest_chunk, question.as_str(), output_tokens).map_err(invalid)?;
        local
            .check(&probe)
            .map_err(|error| invalid(error.to_string()))?;
        let mut terms = Vec::with_capacity(job.terms.len());
        for term in &job.terms {
            terms.push(restored_term(term, mapping, pseudonymiser)?);
        }
        let chunks = (0..material.chunks.len())
            .filter(|position| {
                terms.is_empty()
                    || terms
                        .iter()
                        .any(|term| chunk_matches(&material.chunks[*position], term))
            })
            .collect();
        planned.push(PlannedJob {
            question: question.as_str().to_string(),
            terms,
            chunks,
        });
    }
    Ok(planned)
}
