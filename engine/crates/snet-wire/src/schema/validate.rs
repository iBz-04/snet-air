use super::{ScalarKind, SchemaNode, SchemaViolation};
use crate::json::{JsonObject, JsonValue};

pub(super) fn check_object_root(
    root: &SchemaNode,
    object: &JsonObject,
) -> Result<(), SchemaViolation> {
    let SchemaNode::Object { properties } = root else {
        return Err(violation("#", "the schema root is not an object"));
    };
    check_members(properties, object, "#")
}

/// Paths are built from the schema's own property names and array indexes,
/// never from keys found only in the reply, so an error repeats no reply text.
fn check(node: &SchemaNode, value: &JsonValue, path: &str) -> Result<(), SchemaViolation> {
    match node {
        SchemaNode::Object { properties } => match value {
            JsonValue::Object(object) => check_members(properties, object, path),
            _ => Err(violation(path, "expected an object")),
        },
        SchemaNode::Array { items } => match value {
            JsonValue::Array(elements) => {
                for (index, element) in elements.iter().enumerate() {
                    check(items, element, &format!("{path}/{index}"))?;
                }
                Ok(())
            }
            _ => Err(violation(path, "expected an array")),
        },
        SchemaNode::Scalar {
            kind,
            allowed_values,
        } => {
            if !scalar_matches(*kind, value) {
                return Err(violation(path, format!("expected {}", kind_name(*kind))));
            }
            match allowed_values {
                Some(allowed) if !allowed.contains(value) => {
                    Err(violation(path, "the value is not one of the enum values"))
                }
                _ => Ok(()),
            }
        }
        SchemaNode::AnyOf { branches } => {
            if branches
                .iter()
                .any(|branch| check(branch, value, path).is_ok())
            {
                Ok(())
            } else {
                Err(violation(path, "the value matches no `anyOf` branch"))
            }
        }
    }
}

fn check_members(
    properties: &std::collections::BTreeMap<String, SchemaNode>,
    object: &JsonObject,
    path: &str,
) -> Result<(), SchemaViolation> {
    for (name, property) in properties {
        let property_path = format!("{path}/{name}");
        match object.get(name) {
            Some(value) => check(property, value, &property_path)?,
            None => {
                return Err(violation(
                    &property_path,
                    "the required property is missing",
                ));
            }
        }
    }
    if object.keys().any(|key| !properties.contains_key(key)) {
        return Err(violation(
            path,
            "the object holds a property the schema does not name",
        ));
    }
    Ok(())
}

pub(super) fn scalar_matches(kind: ScalarKind, value: &JsonValue) -> bool {
    match (kind, value) {
        (ScalarKind::String, JsonValue::String(_)) => true,
        (ScalarKind::Number, JsonValue::Number(_)) => true,
        (ScalarKind::Integer, JsonValue::Number(number)) => {
            number.is_i64()
                || number.is_u64()
                || number
                    .as_f64()
                    .is_some_and(|float| float.is_finite() && float.fract() == 0.0)
        }
        (ScalarKind::Boolean, JsonValue::Bool(_)) => true,
        (ScalarKind::Null, JsonValue::Null) => true,
        _ => false,
    }
}

fn kind_name(kind: ScalarKind) -> &'static str {
    match kind {
        ScalarKind::String => "a string",
        ScalarKind::Number => "a number",
        ScalarKind::Integer => "an integer",
        ScalarKind::Boolean => "a boolean",
        ScalarKind::Null => "null",
    }
}

fn violation(path: &str, reason: impl Into<String>) -> SchemaViolation {
    SchemaViolation {
        path: path.to_string(),
        reason: reason.into(),
    }
}
