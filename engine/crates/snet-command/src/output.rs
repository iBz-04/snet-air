use snet_cloud::Charge;
use snet_protocol::CitedPassage;

/// What a finished task spent, printed on standard error.
pub struct Spending {
    /// Conversation rounds or fan-out local requests, named by `steps_name`.
    pub steps: u64,
    pub steps_name: &'static str,
    pub charge: Charge,
    pub local_tokens: u64,
}

/// The answer and the passages it rests on, for standard output.
pub fn answer_lines(text: &str, citations: &[CitedPassage]) -> Vec<String> {
    let mut lines = vec![text.to_string()];
    if !citations.is_empty() {
        lines.push(String::new());
    }
    for passage in citations {
        lines.push(format!(
            "[{}] {}, bytes {}..{}",
            passage.citation, passage.document, passage.range.start, passage.range.end
        ));
        for line in passage.text.lines() {
            lines.push(format!("    {line}"));
        }
    }
    lines
}

pub fn spending_line(spending: &Spending) -> String {
    let charge = spending.charge;
    format!(
        "{} {}, cloud requests {}, sent {} bytes, cloud tokens {} in and {} out, local tokens {}",
        spending.steps_name,
        spending.steps,
        charge.requests,
        charge.bytes_sent,
        charge.input_tokens,
        charge.output_tokens,
        spending.local_tokens
    )
}
