use std::collections::BTreeMap;
use std::fmt;

use serde::de::{self, Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};

pub type JsonObject = BTreeMap<String, JsonValue>;

/// A JSON value whose parser refuses a duplicate key at any depth, so neither
/// a provider's reply nor the object it carries can hold two values for one
/// key.
#[derive(Debug, Clone, PartialEq)]
pub enum JsonValue {
    Null,
    Bool(bool),
    Number(serde_json::Number),
    String(String),
    Array(Vec<JsonValue>),
    Object(JsonObject),
}

impl JsonValue {
    /// For protocol code that deserializes a checked object into its own types.
    pub fn to_serde_json(&self) -> serde_json::Value {
        match self {
            Self::Null => serde_json::Value::Null,
            Self::Bool(value) => serde_json::Value::Bool(*value),
            Self::Number(value) => serde_json::Value::Number(value.clone()),
            Self::String(value) => serde_json::Value::String(value.clone()),
            Self::Array(items) => {
                serde_json::Value::Array(items.iter().map(Self::to_serde_json).collect())
            }
            Self::Object(members) => serde_json::Value::Object(
                members
                    .iter()
                    .map(|(key, value)| (key.clone(), value.to_serde_json()))
                    .collect(),
            ),
        }
    }
}

/// Parses UTF-8 JSON strictly. Into this type serde_json fails only with its
/// fixed syntax messages or the visitor's own, so the error never repeats
/// the text that caused it.
pub fn parse_strict(bytes: &[u8]) -> Result<JsonValue, String> {
    std::str::from_utf8(bytes).map_err(|error| {
        format!(
            "it is not UTF-8 (first bad byte at {})",
            error.valid_up_to()
        )
    })?;
    serde_json::from_slice(bytes).map_err(|error| error.to_string())
}

/// For typed parsing, where serde_json's data errors can quote the value
/// they refused: names only the kind of fault and its position.
pub fn describe_json_error(error: &serde_json::Error) -> String {
    let fault = match error.classify() {
        serde_json::error::Category::Io => "it could not be read",
        serde_json::error::Category::Syntax => "it is not valid JSON",
        serde_json::error::Category::Data => "it does not have the expected shape",
        serde_json::error::Category::Eof => "it ends early",
    };
    format!("{fault} (line {}, column {})", error.line(), error.column())
}

struct JsonValueVisitor;

impl<'de> Visitor<'de> for JsonValueVisitor {
    type Value = JsonValue;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON value")
    }

    fn visit_unit<E: de::Error>(self) -> Result<JsonValue, E> {
        Ok(JsonValue::Null)
    }

    fn visit_bool<E: de::Error>(self, value: bool) -> Result<JsonValue, E> {
        Ok(JsonValue::Bool(value))
    }

    fn visit_i64<E: de::Error>(self, value: i64) -> Result<JsonValue, E> {
        Ok(JsonValue::Number(value.into()))
    }

    fn visit_u64<E: de::Error>(self, value: u64) -> Result<JsonValue, E> {
        Ok(JsonValue::Number(value.into()))
    }

    fn visit_f64<E: de::Error>(self, value: f64) -> Result<JsonValue, E> {
        serde_json::Number::from_f64(value)
            .map(JsonValue::Number)
            .ok_or_else(|| E::custom("a number is not finite"))
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<JsonValue, E> {
        Ok(JsonValue::String(value.to_string()))
    }

    fn visit_string<E: de::Error>(self, value: String) -> Result<JsonValue, E> {
        Ok(JsonValue::String(value))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<JsonValue, A::Error> {
        let mut items = Vec::new();
        while let Some(item) = sequence.next_element()? {
            items.push(item);
        }
        Ok(JsonValue::Array(items))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<JsonValue, A::Error> {
        let mut members = JsonObject::new();
        while let Some(key) = map.next_key::<String>()? {
            let value = map.next_value()?;
            if members.insert(key, value).is_some() {
                return Err(de::Error::custom("an object holds a duplicate key"));
            }
        }
        Ok(JsonValue::Object(members))
    }
}

impl<'de> Deserialize<'de> for JsonValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(JsonValueVisitor)
    }
}
