use snet_protocol::{Citations, CitedPassage, FailureKind, ProtocolError};
use snet_pseudonym::TaskMapping;

use crate::fanout::FanOutOutcome;
use crate::messages::{Step, cloud_step};
use crate::plan::{PlannedJob, check_citations, check_step, plan_jobs};
use crate::rounds::RunInputs;

/// A cloud step that kept every rule.
pub(crate) enum Checked {
    Ended(FanOutOutcome),
    Plan(Vec<PlannedJob>),
}

fn invalid(detail: impl Into<String>) -> ProtocolError {
    ProtocolError::new(FailureKind::InvalidCloudReply, detail)
}

/// Every rule a cloud step must keep: an ending with its citations, or a
/// plan whose jobs can run.
pub(crate) fn checked_step(
    object: &snet_cloud::JsonObject,
    last: bool,
    inputs: &RunInputs<'_>,
    mapping: &TaskMapping,
    citations: &Citations,
) -> Result<Checked, ProtocolError> {
    let RunInputs {
        fan_out,
        task,
        pseudonymiser,
        material,
        longest_chunk,
        ..
    } = inputs;
    let step = cloud_step(object)
        .ok_or_else(|| invalid("the reply's parts do not match the request's schema"))?;
    check_step(&step)?;
    let text = mapping
        .restore(&step.text)
        .map_err(|error| invalid(error.to_string()))?;
    match step.step {
        Step::Answer => {
            check_citations(&step, citations)?;
            let citations = step
                .citations
                .iter()
                .filter_map(|name| citations.find(name))
                .map(|cited| CitedPassage {
                    citation: format!("C{}", cited.number),
                    document: task.documents[cited.document].name.clone(),
                    range: cited.range.clone(),
                    text: cited.text.clone(),
                })
                .collect();
            Ok(Checked::Ended(FanOutOutcome::Answer { text, citations }))
        }
        Step::NotFound => Ok(Checked::Ended(FanOutOutcome::NotFound { text })),
        Step::Plan if last => Err(invalid("the last request planned again")),
        Step::Plan => plan_jobs(
            &step,
            mapping,
            pseudonymiser,
            material,
            longest_chunk,
            fan_out.local,
            task.limits.local_output_tokens,
        )
        .map(Checked::Plan),
    }
}
