mod subset;
mod validate;

use std::collections::BTreeMap;

use crate::json::{JsonObject, JsonValue};

const MAX_NAME_LENGTH: usize = 64;
const MAX_SERIALIZED_CHARACTERS: usize = 120_000;

/// A schema inside the subset both wire formats enforce strictly.
/// Every reply object is checked against it here as well, because some
/// servers ignore `strict`.
#[derive(Debug, Clone)]
pub struct ReplySchema {
    name: String,
    schema: serde_json::Value,
    root: SchemaNode,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("reply schema at {path}: {reason}")]
pub struct SchemaError {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{path}: {reason}")]
pub struct SchemaViolation {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScalarKind {
    String,
    Number,
    Integer,
    Boolean,
    Null,
}

#[derive(Debug, Clone)]
pub(crate) enum SchemaNode {
    Object {
        properties: BTreeMap<String, SchemaNode>,
    },
    Array {
        items: Box<SchemaNode>,
    },
    Scalar {
        kind: ScalarKind,
        allowed_values: Option<Vec<JsonValue>>,
    },
    AnyOf {
        branches: Vec<SchemaNode>,
    },
}

impl ReplySchema {
    pub fn new(name: &str, schema: serde_json::Value) -> Result<Self, SchemaError> {
        let name_is_valid = !name.is_empty()
            && name.len() <= MAX_NAME_LENGTH
            && name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-');
        if !name_is_valid {
            return Err(SchemaError {
                path: "name".to_string(),
                reason: "the name must be 1 to 64 letters, digits, `_` or `-`".to_string(),
            });
        }
        let serialized_characters = schema.to_string().chars().count();
        if serialized_characters > MAX_SERIALIZED_CHARACTERS {
            return Err(SchemaError {
                path: "#".to_string(),
                reason: format!(
                    "the schema is {serialized_characters} characters, above {MAX_SERIALIZED_CHARACTERS}"
                ),
            });
        }
        let root = subset::compile(&schema)?;
        Ok(Self {
            name: name.to_string(),
            schema,
            root,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn as_json(&self) -> &serde_json::Value {
        &self.schema
    }

    /// Checks a reply object against the schema; the violation names schema
    /// paths only, never reply text.
    pub fn check(&self, object: &JsonObject) -> Result<(), SchemaViolation> {
        validate::check_object_root(&self.root, object)
    }
}

#[cfg(test)]
mod tests;
