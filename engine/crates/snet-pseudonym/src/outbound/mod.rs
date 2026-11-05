pub(crate) mod typed;

use std::collections::BTreeMap;

use crate::errors::PseudonymError;

pub use typed::OutboundJson;

/// Where a text may be sent: an endpoint's name and host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Destination {
    endpoint_name: String,
    host: String,
}

impl Destination {
    pub fn new(endpoint_name: impl Into<String>, host: impl Into<String>) -> Self {
        Self {
            endpoint_name: endpoint_name.into(),
            host: host.into(),
        }
    }

    pub fn endpoint_name(&self) -> &str {
        &self.endpoint_name
    }

    pub fn host(&self) -> &str {
        &self.host
    }
}

/// What pseudonymisation did to a text: occurrences replaced per
/// category and occurrences left in clear per passthrough rule position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub policy_digest: [u8; 32],
    pub destination: Destination,
    pub replaced: BTreeMap<String, u64>,
    pub passed_through: BTreeMap<usize, u64>,
}

impl Report {
    pub(crate) fn empty(policy_digest: [u8; 32], destination: Destination) -> Self {
        Self {
            policy_digest,
            destination,
            replaced: BTreeMap::new(),
            passed_through: BTreeMap::new(),
        }
    }

    /// Adds `other`'s counts after checking it was made for the same
    /// destination and policy.
    pub(crate) fn absorb(&mut self, other: &Report) -> Result<(), PseudonymError> {
        if other.destination != self.destination {
            return Err(PseudonymError::DestinationMismatch);
        }
        if other.policy_digest != self.policy_digest {
            return Err(PseudonymError::PolicyMismatch);
        }
        for (category, count) in &other.replaced {
            *self.replaced.entry(category.clone()).or_default() += count;
        }
        for (rule, count) in &other.passed_through {
            *self.passed_through.entry(*rule).or_default() += count;
        }
        Ok(())
    }
}

/// Text that may be sent to its destination: made only by pseudonymisation,
/// typed JSON, a static template or a join. Its `Debug` shows its
/// size and report, never its text, which can hold values a passthrough
/// rule let through.
#[derive(Clone, PartialEq, Eq)]
pub struct OutboundText {
    text: String,
    report: Report,
}

impl std::fmt::Debug for OutboundText {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OutboundText")
            .field("bytes", &self.text.len())
            .field("report", &self.report)
            .finish()
    }
}

impl OutboundText {
    pub(crate) fn made(text: String, report: Report) -> Self {
        Self { text, report }
    }

    pub fn as_str(&self) -> &str {
        &self.text
    }

    pub fn report(&self) -> &Report {
        &self.report
    }

    pub fn destination(&self) -> &Destination {
        &self.report.destination
    }

    /// Joins two texts of the same destination and policy; the report sums
    /// both.
    pub fn join(mut self, other: &OutboundText) -> Result<OutboundText, PseudonymError> {
        self.report.absorb(&other.report)?;
        self.text.push_str(&other.text);
        Ok(self)
    }
}
