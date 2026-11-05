use std::collections::BTreeSet;

use super::{OutboundText, Report};
use crate::errors::PseudonymError;
use crate::json_write::{Sink, string};

const MAX_INTEGER: u32 = 999_999;
const MAX_DEPTH: usize = 64;

/// A JSON tree whose structure can never hold material: keys are
/// static strings, leaves are small integers, static strings, a static
/// name followed by a small number, or already pseudonymised text.
#[derive(Debug, Clone)]
pub enum OutboundJson {
    Null,
    Bool(bool),
    Integer(u32),
    Static(&'static str),
    Numbered(&'static str, u32),
    Text(OutboundText),
    Array(Vec<OutboundJson>),
    Object(Vec<(&'static str, OutboundJson)>),
}

/// Serialises a typed tree, checking every leaf and summing their reports
/// into `report`.
pub(crate) fn build(value: &OutboundJson, report: &mut Report) -> Result<String, PseudonymError> {
    let mut text = String::new();
    write(value, report, &mut text, 1, "")?;
    Ok(text)
}

fn write(
    value: &OutboundJson,
    report: &mut Report,
    sink: &mut String,
    level: usize,
    path: &str,
) -> Result<(), PseudonymError> {
    if level > MAX_DEPTH {
        return Err(PseudonymError::JsonTooDeep);
    }
    let small = |number: u32| {
        if number > MAX_INTEGER {
            Err(PseudonymError::IntegerOutOfRange { value: number })
        } else {
            Ok(number)
        }
    };
    match value {
        OutboundJson::Null => sink.push_str("null"),
        OutboundJson::Bool(flag) => sink.push_str(if *flag { "true" } else { "false" }),
        OutboundJson::Integer(number) => sink.push_str(&small(*number)?.to_string()),
        OutboundJson::Static(text) => string(sink, text),
        OutboundJson::Numbered(name, number) => {
            let joined = format!("{name}{}", small(*number)?);
            string(sink, &joined);
        }
        OutboundJson::Text(text) => {
            report.absorb(text.report())?;
            string(sink, text.as_str());
        }
        OutboundJson::Array(items) => {
            Sink::push_str(sink, "[");
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    Sink::push_str(sink, ",");
                }
                write(item, report, sink, level + 1, &format!("{path}/{index}"))?;
            }
            Sink::push_str(sink, "]");
        }
        OutboundJson::Object(members) => {
            let mut keys = BTreeSet::new();
            Sink::push_str(sink, "{");
            for (index, (key, item)) in members.iter().enumerate() {
                if !keys.insert(*key) {
                    return Err(PseudonymError::KeysCollide {
                        path: if path.is_empty() {
                            "/".to_string()
                        } else {
                            path.to_string()
                        },
                    });
                }
                if index > 0 {
                    Sink::push_str(sink, ",");
                }
                string(sink, key);
                Sink::push_str(sink, ":");
                write(item, report, sink, level + 1, &format!("{path}/{index}"))?;
            }
            Sink::push_str(sink, "}");
        }
    }
    Ok(())
}
