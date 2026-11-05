use serde_json::{Value, json};

use super::subset::compile;
use super::*;
use crate::json::parse_strict;

fn answer_schema() -> Value {
    json!({
        "type": "object",
        "description": "an answer with its citations",
        "properties": {
            "answer": {"type": "string"},
            "confidence": {"type": "number"},
            "rounds": {"type": "integer"},
            "final": {"type": "boolean"},
            "verdict": {"type": "string", "enum": ["yes", "no", "unsure"]},
            "citations": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "chunk": {"type": "integer"},
                        "quote": {"type": "string"}
                    },
                    "required": ["chunk", "quote"],
                    "additionalProperties": false
                }
            },
            "note": {"anyOf": [{"type": "string"}, {"type": "null"}]}
        },
        "required": ["answer", "confidence", "rounds", "final", "verdict", "citations", "note"],
        "additionalProperties": false
    })
}

fn object_with(properties: Value) -> Value {
    let names: Vec<String> = properties.as_object().unwrap().keys().cloned().collect();
    json!({
        "type": "object",
        "properties": properties,
        "required": names,
        "additionalProperties": false
    })
}

fn refused_reason(schema: Value) -> String {
    match ReplySchema::new("answer", schema) {
        Err(error) => error.reason,
        Ok(_) => panic!("the schema was accepted"),
    }
}

fn check(schema: &ReplySchema, reply: &str) -> Result<(), SchemaViolation> {
    let JsonValue::Object(object) = parse_strict(reply.as_bytes()).unwrap() else {
        panic!("the reply is not an object");
    };
    schema.check(&object)
}

#[test]
fn the_supported_subset_is_accepted() {
    let schema = ReplySchema::new("answer_v1", answer_schema()).unwrap();
    assert_eq!(schema.name(), "answer_v1");
    assert_eq!(schema.as_json(), &answer_schema());
}

#[test]
fn names_are_one_to_sixty_four_plain_characters() {
    for bad in ["", "has space", "dot.ted", &"a".repeat(65)] {
        assert!(ReplySchema::new(bad, answer_schema()).is_err(), "{bad}");
    }
    assert!(ReplySchema::new(&"a".repeat(64), answer_schema()).is_ok());
    assert!(ReplySchema::new("A-b_9", answer_schema()).is_ok());
}

#[test]
fn each_subset_rule_refuses_once() {
    let string = json!({"type": "string"});
    let cases: Vec<(Value, &str)> = vec![
        (json!({"type": "string"}), "root must have type `object`"),
        (json!([1, 2]), "must be a JSON object"),
        (
            json!({"type": "object", "properties": {}, "required": [], "additionalProperties": false, "enum": [1]}),
            "keyword `enum`",
        ),
        (
            json!({"type": "object", "properties": {}, "required": [], "additionalProperties": false, "anyOf": []}),
            "keyword `anyOf`",
        ),
        (
            object_with(json!({"a": {"type": "integer", "minimum": 1}})),
            "keyword `minimum`",
        ),
        (
            object_with(json!({"a": {"type": "string", "maxLength": 9}})),
            "keyword `maxLength`",
        ),
        (object_with(json!({"a": {"$ref": "#/x"}})), "keyword `$ref`"),
        (
            object_with(json!({"a": {"type": ["string", "null"]}})),
            "type arrays",
        ),
        (
            object_with(json!({"a": {"type": 5}})),
            "`type` must be a string",
        ),
        (object_with(json!({"a": {}})), "must state its `type`"),
        (object_with(json!({"a": {"type": "date"}})), "type `date`"),
        (
            json!({"type": "object", "properties": {"a": string}, "required": ["a"]}),
            "`additionalProperties` to false",
        ),
        (
            json!({"type": "object", "properties": {"a": string}, "required": ["a"], "additionalProperties": true}),
            "`additionalProperties` to false",
        ),
        (
            json!({"type": "object", "properties": {"a": string, "b": string}, "required": ["a"], "additionalProperties": false}),
            "every property must be listed",
        ),
        (
            json!({"type": "object", "properties": {"a": string}, "required": ["a", "z"], "additionalProperties": false}),
            "not a property",
        ),
        (
            json!({"type": "object", "properties": {"a": string}, "required": ["a", "a"], "additionalProperties": false}),
            "twice",
        ),
        (
            json!({"type": "object", "properties": {"a": string}, "required": [1], "additionalProperties": false}),
            "only strings",
        ),
        (
            json!({"type": "object", "required": [], "additionalProperties": false}),
            "`properties` object",
        ),
        (
            json!({"type": "object", "properties": {}, "additionalProperties": false}),
            "`required` array",
        ),
        (
            object_with(json!({"a": {"type": "array"}})),
            "must have `items`",
        ),
        (
            object_with(json!({"a": {"type": "array", "items": string, "properties": {}}})),
            "`properties` is not allowed on an array",
        ),
        (
            object_with(json!({"a": {"type": "string", "items": string}})),
            "`items` is not allowed on a scalar",
        ),
        (
            object_with(
                json!({"a": {"type": "object", "properties": {}, "required": [], "additionalProperties": false, "enum": [1]}}),
            ),
            "`enum` is not allowed on an object",
        ),
        (
            object_with(json!({"a": {"type": "string", "enum": []}})),
            "`enum` must not be empty",
        ),
        (
            object_with(json!({"a": {"type": "string", "enum": "yes"}})),
            "`enum` must be an array",
        ),
        (
            object_with(json!({"a": {"type": "string", "enum": [["x"]]}})),
            "only scalar values",
        ),
        (
            object_with(json!({"a": {"type": "string", "enum": [1]}})),
            "does not match the `type`",
        ),
        (
            object_with(json!({"a": {"anyOf": []}})),
            "`anyOf` must not be empty",
        ),
        (
            object_with(json!({"a": {"anyOf": {"type": "string"}}})),
            "`anyOf` must be an array",
        ),
        (
            object_with(json!({"a": {"type": "string", "anyOf": [string]}})),
            "`type` is not allowed beside `anyOf`",
        ),
        (
            object_with(json!({"a": {"type": "string", "description": 3}})),
            "`description` must be a string",
        ),
    ];
    for (schema, expected) in cases {
        let reason = refused_reason(schema.clone());
        assert!(reason.contains(expected), "{schema}: {reason}");
    }
}

fn nested(levels: usize) -> Value {
    let mut schema = json!({"type": "string"});
    for _ in 1..levels {
        schema = object_with(json!({"inner": schema}));
    }
    schema
}

#[test]
fn nesting_is_limited_to_ten_levels() {
    assert!(ReplySchema::new("answer", nested(10)).is_ok());
    assert!(refused_reason(nested(11)).contains("deeper than 10"));
}

#[test]
fn property_enum_and_any_of_totals_are_limited() {
    let many = |count: usize, value: Value| -> Value {
        let properties: serde_json::Map<String, Value> = (0..count)
            .map(|index| (format!("p{index}"), value.clone()))
            .collect();
        object_with(Value::Object(properties))
    };
    let null = json!({"type": "null"});
    assert!(compile(&many(5_000, null.clone())).is_ok());
    assert!(
        compile(&many(5_001, null))
            .unwrap_err()
            .reason
            .contains("5000 properties")
    );

    let values: Vec<u32> = (0..1_000).collect();
    let at_limit = object_with(json!({"a": {"type": "integer", "enum": values}}));
    assert!(compile(&at_limit).is_ok());
    let over = object_with(
        json!({"a": {"type": "integer", "enum": values}, "b": {"type": "integer", "enum": [1]}}),
    );
    assert!(
        compile(&over)
            .unwrap_err()
            .reason
            .contains("1000 enum values")
    );

    let either = json!({"anyOf": [{"type": "string"}, {"type": "null"}]});
    assert!(compile(&many(16, either.clone())).is_ok());
    assert!(
        compile(&many(17, either))
            .unwrap_err()
            .reason
            .contains("16 `anyOf` groups")
    );
}

#[test]
fn the_serialized_schema_is_limited_to_120_000_characters() {
    let description = |length: usize| {
        let mut schema = object_with(json!({"a": {"type": "string"}}));
        let overhead = schema.to_string().chars().count() + r#","description":"""#.len();
        schema["description"] = Value::String("d".repeat(length - overhead));
        schema
    };
    assert_eq!(description(120_000).to_string().chars().count(), 120_000);
    assert!(ReplySchema::new("answer", description(120_000)).is_ok());
    assert!(refused_reason(description(120_001)).contains("characters"));
}

#[test]
fn conforming_objects_pass() {
    let schema = ReplySchema::new("answer", answer_schema()).unwrap();
    let reply = r#"{"answer":"42","confidence":0.5,"rounds":3,"final":true,"verdict":"yes",
        "citations":[{"chunk":1,"quote":"x"}],"note":null}"#;
    check(&schema, reply).unwrap();
    let integral_float = reply.replace("\"rounds\":3", "\"rounds\":3.0");
    check(&schema, &integral_float).unwrap();
    let note_text = reply.replace("\"note\":null", "\"note\":\"seen\"");
    check(&schema, &note_text).unwrap();
}

#[test]
fn breaking_objects_fail_with_a_schema_path() {
    let schema = ReplySchema::new("answer", answer_schema()).unwrap();
    let good = r#"{"answer":"42","confidence":0.5,"rounds":3,"final":true,"verdict":"yes","citations":[{"chunk":1,"quote":"x"}],"note":null}"#;
    let cases = [
        (good.replace(r#""answer":"42","#, ""), "#/answer", "missing"),
        (
            good.replace(r#""note":null"#, r#""note":null,"secret_marker":1"#),
            "#",
            "does not name",
        ),
        (
            good.replace(r#""answer":"42""#, r#""answer":42"#),
            "#/answer",
            "a string",
        ),
        (
            good.replace(r#""rounds":3"#, r#""rounds":3.5"#),
            "#/rounds",
            "an integer",
        ),
        (
            good.replace(r#""final":true"#, r#""final":"true""#),
            "#/final",
            "a boolean",
        ),
        (
            good.replace(r#""verdict":"yes""#, r#""verdict":"maybe""#),
            "#/verdict",
            "enum",
        ),
        (
            good.replace(r#""note":null"#, r#""note":7"#),
            "#/note",
            "no `anyOf` branch",
        ),
        (
            good.replace(r#""chunk":1"#, r#""chunk":"one""#),
            "#/citations/0/chunk",
            "an integer",
        ),
        (
            good.replace(r#"[{"chunk":1,"quote":"x"}]"#, r#"{}"#),
            "#/citations",
            "an array",
        ),
    ];
    for (reply, path, reason) in cases {
        let violation = check(&schema, &reply).unwrap_err();
        assert_eq!(violation.path, path, "{reply}");
        assert!(
            violation.reason.contains(reason),
            "{reply}: {}",
            violation.reason
        );
        assert!(!violation.to_string().contains("secret_marker"));
    }
}
