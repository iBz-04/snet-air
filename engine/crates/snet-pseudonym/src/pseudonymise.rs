use std::collections::BTreeMap;

use snet_wire::JsonValue;

use crate::detect::{Detection, canonical, detect_all};
use crate::errors::PseudonymError;
use crate::json_write::{self, Counter};
use crate::mapping::{Staging, TaskMapping};
use crate::outbound::{Destination, OutboundJson, OutboundText, Report, typed};
use crate::policy::Policy;
use crate::resolve::merge;
use crate::text::{TOKEN_CLOSE, TOKEN_OPEN, character_position, is_bracket, nfc};

const MAX_TEXT_BYTES: usize = 1024 * 1024;
const MAX_JSON_DEPTH: usize = 64;

/// The policy applied for one destination.
pub struct Pseudonymiser<'p> {
    policy: &'p Policy,
    destination: Destination,
    /// Category name to the first rule position that passes it through.
    passed: BTreeMap<String, usize>,
}

impl Policy {
    pub fn for_destination(&self, destination: Destination) -> Pseudonymiser<'_> {
        let mut passed = BTreeMap::new();
        for position in self.rules_for(&destination) {
            passed
                .entry(self.passthrough[position].category.clone())
                .or_insert(position);
        }
        Pseudonymiser {
            policy: self,
            destination,
            passed,
        }
    }
}

impl Pseudonymiser<'_> {
    fn empty_report(&self) -> Report {
        Report::empty(self.policy.digest, self.destination.clone())
    }

    /// A fixed text of our own, such as a prompt.
    pub fn template(&self, text: &'static str) -> OutboundText {
        OutboundText::made(text.to_string(), self.empty_report())
    }

    /// Typed JSON for this destination and policy.
    pub fn typed(&self, value: &OutboundJson) -> Result<OutboundText, PseudonymError> {
        let mut report = self.empty_report();
        let text = typed::build(value, &mut report)?;
        Ok(OutboundText::made(text, report))
    }

    /// Replaces every value the policy finds in `input`.
    pub fn text(
        &self,
        mapping: &mut TaskMapping,
        input: &str,
    ) -> Result<OutboundText, PseudonymError> {
        let mut staging = Staging::new();
        let mut report = self.empty_report();
        let replaced = self.replace(mapping, &mut staging, &mut report, input)?;
        staging.commit(mapping);
        Ok(OutboundText::made(replaced, report))
    }

    /// Pseudonymises every decoded string and key of `value` separately,
    /// refuses numbers that look like values, then serialises.
    pub fn json(
        &self,
        mapping: &mut TaskMapping,
        value: &JsonValue,
    ) -> Result<OutboundText, PseudonymError> {
        if json_write::nests_deeper_than(value, MAX_JSON_DEPTH) {
            return Err(PseudonymError::JsonTooDeep);
        }
        let mut counter = Counter(0);
        json_write::value(&mut counter, value);
        if counter.0 > MAX_TEXT_BYTES {
            return Err(PseudonymError::JsonTooLarge { size: counter.0 });
        }
        let mut staging = Staging::new();
        let mut report = self.empty_report();
        let replaced = self.replace_json(mapping, &mut staging, &mut report, value, "")?;
        let mut text = String::new();
        json_write::value(&mut text, &replaced);
        staging.commit(mapping);
        Ok(OutboundText::made(text, report))
    }

    fn replace_json(
        &self,
        mapping: &TaskMapping,
        staging: &mut Staging,
        report: &mut Report,
        value: &JsonValue,
        path: &str,
    ) -> Result<JsonValue, PseudonymError> {
        Ok(match value {
            JsonValue::Null | JsonValue::Bool(_) => value.clone(),
            JsonValue::Number(number) => {
                self.check_number(&number.to_string(), path)?;
                value.clone()
            }
            JsonValue::String(text) => {
                JsonValue::String(self.replace(mapping, staging, report, text)?)
            }
            JsonValue::Array(items) => {
                let mut replaced = Vec::with_capacity(items.len());
                for (index, item) in items.iter().enumerate() {
                    replaced.push(self.replace_json(
                        mapping,
                        staging,
                        report,
                        item,
                        &format!("{path}/{index}"),
                    )?);
                }
                JsonValue::Array(replaced)
            }
            JsonValue::Object(members) => {
                let mut replaced = BTreeMap::new();
                for (index, (key, item)) in members.iter().enumerate() {
                    let member_path = format!("{path}/{index}");
                    let new_key = self.replace(mapping, staging, report, key)?;
                    let new_item =
                        self.replace_json(mapping, staging, report, item, &member_path)?;
                    if replaced.insert(new_key, new_item).is_some() {
                        return Err(PseudonymError::KeysCollide {
                            path: if path.is_empty() {
                                "/".to_string()
                            } else {
                                path.to_string()
                            },
                        });
                    }
                }
                JsonValue::Object(replaced)
            }
        })
    }

    /// A number's text and the digits of its integer part must
    /// match no detector whose category is not passed through.
    fn check_number(&self, written: &str, path: &str) -> Result<(), PseudonymError> {
        let integer_digits: String = written
            .split(['.', 'e', 'E'])
            .next()
            .unwrap_or_default()
            .chars()
            .filter(char::is_ascii_digit)
            .collect();
        for text in [written, integer_digits.as_str()] {
            if let Some(found) = self.kept_detections(text).first() {
                return Err(PseudonymError::NumberMatched {
                    path: if path.is_empty() {
                        "/".to_string()
                    } else {
                        path.to_string()
                    },
                    category: self.policy.category_of(found.source).to_string(),
                });
            }
        }
        Ok(())
    }

    fn kept_detections(&self, text: &str) -> Vec<Detection> {
        detect_all(self.policy, text)
            .into_iter()
            .filter(|found| {
                !self
                    .passed
                    .contains_key(self.policy.category_of(found.source))
            })
            .collect()
    }

    fn replace(
        &self,
        mapping: &TaskMapping,
        staging: &mut Staging,
        report: &mut Report,
        input: &str,
    ) -> Result<String, PseudonymError> {
        if input.len() > MAX_TEXT_BYTES {
            return Err(PseudonymError::TextTooLarge { size: input.len() });
        }
        if let Some((index, _)) = input
            .char_indices()
            .find(|(_, character)| is_bracket(*character))
        {
            return Err(PseudonymError::Bracket {
                position: character_position(input, index),
            });
        }
        let normal = nfc(input);
        let detections = detect_all(self.policy, &normal);
        let mut kept = Vec::new();
        let mut passed: BTreeMap<usize, Vec<Detection>> = BTreeMap::new();
        for found in detections {
            match self.passed.get(self.policy.category_of(found.source)) {
                Some(rule) => passed.entry(*rule).or_default().push(found),
                None => kept.push(found),
            }
        }
        for (rule, found) in passed {
            *report.passed_through.entry(rule).or_default() += merge(found).len() as u64;
        }
        let spans = merge(kept);
        let mut output = String::with_capacity(normal.len());
        let mut copied = 0;
        for span in spans {
            let category = self.policy.category_of(span.source);
            let surface = &normal[span.start..span.end];
            let form = if span.whole {
                canonical(span.source, surface).ok_or_else(|| PseudonymError::NoCanonicalForm {
                    category: category.to_string(),
                })?
            } else {
                crate::text::fold(surface)
            };
            let number = staging.number_for(mapping, category, form, surface)?;
            output.push_str(&normal[copied..span.start]);
            output.push(TOKEN_OPEN);
            output.push_str(category);
            output.push('_');
            output.push_str(&number.to_string());
            output.push(TOKEN_CLOSE);
            copied = span.end;
            *report.replaced.entry(category.to_string()).or_default() += 1;
        }
        output.push_str(&normal[copied..]);
        Ok(output)
    }
}

#[cfg(test)]
mod tests;
