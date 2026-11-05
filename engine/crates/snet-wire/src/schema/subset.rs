use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value};

use super::{ScalarKind, SchemaError, SchemaNode};
use crate::json::JsonValue;

const MAX_NESTING_LEVELS: usize = 10;
const MAX_PROPERTIES: usize = 5_000;
const MAX_ENUM_VALUES: usize = 1_000;
const MAX_ANY_OF_GROUPS: usize = 16;

const ROOT_KEYWORDS: &[&str] = &[
    "type",
    "properties",
    "required",
    "additionalProperties",
    "description",
];
const NESTED_KEYWORDS: &[&str] = &[
    "type",
    "properties",
    "required",
    "additionalProperties",
    "description",
    "items",
    "enum",
    "anyOf",
];

#[derive(Default)]
struct Totals {
    properties: usize,
    enum_values: usize,
    any_of_groups: usize,
}

pub(super) fn compile(schema: &Value) -> Result<SchemaNode, SchemaError> {
    let mut totals = Totals::default();
    let root = compile_node(schema, "#", 1, true, &mut totals)?;
    match root {
        SchemaNode::Object { .. } => Ok(root),
        _ => Err(refuse("#", "the root must have type `object`")),
    }
}

fn refuse(path: &str, reason: impl Into<String>) -> SchemaError {
    SchemaError {
        path: path.to_string(),
        reason: reason.into(),
    }
}

fn compile_node(
    schema: &Value,
    path: &str,
    level: usize,
    is_root: bool,
    totals: &mut Totals,
) -> Result<SchemaNode, SchemaError> {
    if level > MAX_NESTING_LEVELS {
        return Err(refuse(
            path,
            format!("nesting is deeper than {MAX_NESTING_LEVELS} levels"),
        ));
    }
    let Value::Object(keywords) = schema else {
        return Err(refuse(path, "a schema must be a JSON object"));
    };
    let allowed = if is_root {
        ROOT_KEYWORDS
    } else {
        NESTED_KEYWORDS
    };
    for keyword in keywords.keys() {
        if !allowed.contains(&keyword.as_str()) {
            return Err(refuse(
                path,
                format!("keyword `{keyword}` is outside the supported subset"),
            ));
        }
    }
    if let Some(description) = keywords.get("description")
        && !description.is_string()
    {
        return Err(refuse(path, "`description` must be a string"));
    }

    if let Some(branches) = keywords.get("anyOf") {
        return compile_any_of(keywords, branches, path, level, totals);
    }

    let kind = match keywords.get("type") {
        Some(Value::String(kind)) => kind.as_str(),
        Some(Value::Array(_)) => return Err(refuse(path, "type arrays are not supported")),
        Some(_) => return Err(refuse(path, "`type` must be a string")),
        None => return Err(refuse(path, "every schema must state its `type`")),
    };
    match kind {
        "object" => compile_object(keywords, path, level, totals),
        "array" => compile_array(keywords, path, level, totals),
        "string" => compile_scalar(keywords, ScalarKind::String, path, totals),
        "number" => compile_scalar(keywords, ScalarKind::Number, path, totals),
        "integer" => compile_scalar(keywords, ScalarKind::Integer, path, totals),
        "boolean" => compile_scalar(keywords, ScalarKind::Boolean, path, totals),
        "null" => compile_scalar(keywords, ScalarKind::Null, path, totals),
        other => Err(refuse(path, format!("type `{other}` is not supported"))),
    }
}

fn forbid(
    keywords: &Map<String, Value>,
    names: &[&str],
    path: &str,
    why: &str,
) -> Result<(), SchemaError> {
    for name in names {
        if keywords.contains_key(*name) {
            return Err(refuse(path, format!("`{name}` is not allowed {why}")));
        }
    }
    Ok(())
}

fn compile_any_of(
    keywords: &Map<String, Value>,
    branches: &Value,
    path: &str,
    level: usize,
    totals: &mut Totals,
) -> Result<SchemaNode, SchemaError> {
    let others: Vec<&str> = NESTED_KEYWORDS
        .iter()
        .copied()
        .filter(|name| *name != "anyOf" && *name != "description")
        .collect();
    forbid(keywords, &others, path, "beside `anyOf`")?;
    let Value::Array(branches) = branches else {
        return Err(refuse(path, "`anyOf` must be an array"));
    };
    if branches.is_empty() {
        return Err(refuse(path, "`anyOf` must not be empty"));
    }
    totals.any_of_groups += 1;
    if totals.any_of_groups > MAX_ANY_OF_GROUPS {
        return Err(refuse(
            path,
            format!("there are more than {MAX_ANY_OF_GROUPS} `anyOf` groups"),
        ));
    }
    let mut compiled = Vec::with_capacity(branches.len());
    for (index, branch) in branches.iter().enumerate() {
        let branch_path = format!("{path}/anyOf/{index}");
        compiled.push(compile_node(branch, &branch_path, level, false, totals)?);
    }
    Ok(SchemaNode::AnyOf { branches: compiled })
}

fn compile_object(
    keywords: &Map<String, Value>,
    path: &str,
    level: usize,
    totals: &mut Totals,
) -> Result<SchemaNode, SchemaError> {
    forbid(keywords, &["items", "enum"], path, "on an object")?;
    let Some(Value::Object(properties)) = keywords.get("properties") else {
        return Err(refuse(path, "an object must have a `properties` object"));
    };
    if keywords.get("additionalProperties") != Some(&Value::Bool(false)) {
        return Err(refuse(
            path,
            "an object must set `additionalProperties` to false",
        ));
    }
    let Some(Value::Array(required)) = keywords.get("required") else {
        return Err(refuse(path, "an object must have a `required` array"));
    };
    let mut required_names = BTreeSet::new();
    for name in required {
        let Value::String(name) = name else {
            return Err(refuse(path, "`required` must hold only strings"));
        };
        if !required_names.insert(name.as_str()) {
            return Err(refuse(path, format!("`required` names `{name}` twice")));
        }
        if !properties.contains_key(name) {
            return Err(refuse(
                path,
                format!("`required` names `{name}`, which is not a property"),
            ));
        }
    }
    if required_names.len() != properties.len() {
        return Err(refuse(path, "every property must be listed in `required`"));
    }

    totals.properties += properties.len();
    if totals.properties > MAX_PROPERTIES {
        return Err(refuse(
            path,
            format!("there are more than {MAX_PROPERTIES} properties"),
        ));
    }
    let mut compiled = BTreeMap::new();
    for (name, property) in properties {
        let property_path = format!("{path}/properties/{name}");
        let node = compile_node(property, &property_path, level + 1, false, totals)?;
        compiled.insert(name.clone(), node);
    }
    Ok(SchemaNode::Object {
        properties: compiled,
    })
}

fn compile_array(
    keywords: &Map<String, Value>,
    path: &str,
    level: usize,
    totals: &mut Totals,
) -> Result<SchemaNode, SchemaError> {
    forbid(
        keywords,
        &["properties", "required", "additionalProperties", "enum"],
        path,
        "on an array",
    )?;
    let Some(items) = keywords.get("items") else {
        return Err(refuse(path, "an array must have `items`"));
    };
    let items_path = format!("{path}/items");
    let items = compile_node(items, &items_path, level + 1, false, totals)?;
    Ok(SchemaNode::Array {
        items: Box::new(items),
    })
}

fn compile_scalar(
    keywords: &Map<String, Value>,
    kind: ScalarKind,
    path: &str,
    totals: &mut Totals,
) -> Result<SchemaNode, SchemaError> {
    forbid(
        keywords,
        &["properties", "required", "additionalProperties", "items"],
        path,
        "on a scalar",
    )?;
    let allowed_values = match keywords.get("enum") {
        None => None,
        Some(Value::Array(values)) => {
            if values.is_empty() {
                return Err(refuse(path, "`enum` must not be empty"));
            }
            totals.enum_values += values.len();
            if totals.enum_values > MAX_ENUM_VALUES {
                return Err(refuse(
                    path,
                    format!("there are more than {MAX_ENUM_VALUES} enum values"),
                ));
            }
            let mut converted = Vec::with_capacity(values.len());
            for value in values {
                let value = scalar_value(value)
                    .ok_or_else(|| refuse(path, "`enum` may hold only scalar values"))?;
                if !super::validate::scalar_matches(kind, &value) {
                    return Err(refuse(path, "an `enum` value does not match the `type`"));
                }
                converted.push(value);
            }
            Some(converted)
        }
        Some(_) => return Err(refuse(path, "`enum` must be an array")),
    };
    Ok(SchemaNode::Scalar {
        kind,
        allowed_values,
    })
}

fn scalar_value(value: &Value) -> Option<JsonValue> {
    match value {
        Value::Null => Some(JsonValue::Null),
        Value::Bool(value) => Some(JsonValue::Bool(*value)),
        Value::Number(value) => Some(JsonValue::Number(value.clone())),
        Value::String(value) => Some(JsonValue::String(value.clone())),
        Value::Array(_) | Value::Object(_) => None,
    }
}
