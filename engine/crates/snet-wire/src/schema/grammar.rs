use super::{ScalarKind, SchemaNode};
use crate::json::JsonValue;

/// Whitespace a model may put between tokens, bounded so it cannot spend
/// its whole output on spaces.
const WHITESPACE_RULE: &str = r#"ws ::= [ \t\n]{0,16}"#;
const STRING_RULE: &str = r#"string ::= "\"" char* "\""
char ::= [^"\\\x00-\x1F\x7F] | "\\" (["\\/bfnrt] | "u" [0-9a-fA-F]{4})"#;
const NUMBER_RULE: &str =
    r#"number ::= "-"? ("0" | [1-9] [0-9]*) ("." [0-9]+)? ([eE] [-+]? [0-9]+)?"#;
const INTEGER_RULE: &str = r#"integer ::= "-"? ("0" | [1-9] [0-9]*)"#;

/// Writes the GBNF grammar whose language is the JSON texts `root` accepts.
pub(super) fn write(root: &SchemaNode) -> String {
    let mut writer = Writer::default();
    let root_rule = writer.node(root);
    let mut grammar = format!("root ::= ws {root_rule} ws\n");
    for rule in &writer.rules {
        grammar.push_str(rule);
        grammar.push('\n');
    }
    for shared in [WHITESPACE_RULE, STRING_RULE, NUMBER_RULE, INTEGER_RULE] {
        grammar.push_str(shared);
        grammar.push('\n');
    }
    grammar
}

#[derive(Default)]
struct Writer {
    rules: Vec<String>,
}

impl Writer {
    /// Adds a rule for `node` and returns its name.
    fn node(&mut self, node: &SchemaNode) -> String {
        let body = match node {
            SchemaNode::Object { properties } => {
                let members: Vec<String> = properties
                    .iter()
                    .map(|(name, property)| {
                        let value = self.node(property);
                        format!("{} ws \":\" ws {value}", literal(&json_string(name)))
                    })
                    .collect();
                if members.is_empty() {
                    "\"{\" ws \"}\"".to_string()
                } else {
                    format!("\"{{\" ws {} ws \"}}\"", members.join(" ws \",\" ws "))
                }
            }
            SchemaNode::Array { items } => {
                let item = self.node(items);
                format!("\"[\" ws ({item} (ws \",\" ws {item})*)? ws \"]\"")
            }
            SchemaNode::Scalar {
                allowed_values: Some(values),
                ..
            } => values
                .iter()
                .map(|value| literal(&compact_json(value)))
                .collect::<Vec<_>>()
                .join(" | "),
            SchemaNode::Scalar {
                kind,
                allowed_values: None,
            } => match kind {
                ScalarKind::String => "string".to_string(),
                ScalarKind::Number => "number".to_string(),
                ScalarKind::Integer => "integer".to_string(),
                ScalarKind::Boolean => "\"true\" | \"false\"".to_string(),
                ScalarKind::Null => "\"null\"".to_string(),
            },
            SchemaNode::AnyOf { branches } => branches
                .iter()
                .map(|branch| self.node(branch))
                .collect::<Vec<_>>()
                .join(" | "),
        };
        // Children were added above, so the count is already past them.
        let name = format!("node-{}", self.rules.len() + 1);
        self.rules.push(format!("{name} ::= {body}"));
        name
    }
}

fn json_string(text: &str) -> String {
    serde_json::Value::String(text.to_string()).to_string()
}

fn compact_json(value: &JsonValue) -> String {
    value.to_serde_json().to_string()
}

/// A GBNF string literal matching the JSON text `json` exactly. JSON text
/// holds no raw control character, so only `"` and `\` need escaping.
fn literal(json: &str) -> String {
    let mut quoted = String::with_capacity(json.len() + 2);
    quoted.push('"');
    for character in json.chars() {
        match character {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            other => quoted.push(other),
        }
    }
    quoted.push('"');
    quoted
}

#[cfg(test)]
mod tests;
