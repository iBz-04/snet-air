use serde_json::json;

use crate::schema::ReplySchema;

fn grammar_of(schema: serde_json::Value) -> String {
    ReplySchema::new("test", schema).unwrap().grammar()
}

fn rules(grammar: &str) -> Vec<&str> {
    grammar.lines().collect()
}

#[test]
fn an_object_lists_every_property_in_order_with_its_value_rule() {
    let grammar = grammar_of(json!({
        "type": "object",
        "properties": {
            "answer": {"type": "string"},
            "count": {"type": "integer"},
            "done": {"type": "boolean"}
        },
        "required": ["answer", "count", "done"],
        "additionalProperties": false
    }));
    assert_eq!(
        rules(&grammar),
        [
            "root ::= ws node-4 ws",
            "node-1 ::= string",
            "node-2 ::= integer",
            r#"node-3 ::= "true" | "false""#,
            r#"node-4 ::= "{" ws "\"answer\"" ws ":" ws node-1 ws "," ws "\"count\"" ws ":" ws node-2 ws "," ws "\"done\"" ws ":" ws node-3 ws "}""#,
            r#"ws ::= [ \t\n]{0,16}"#,
            r#"string ::= "\"" char* "\"""#,
            r#"char ::= [^"\\\x00-\x1F\x7F] | "\\" (["\\/bfnrt] | "u" [0-9a-fA-F]{4})"#,
            r#"number ::= "-"? ("0" | [1-9] [0-9]*) ("." [0-9]+)? ([eE] [-+]? [0-9]+)?"#,
            r#"integer ::= "-"? ("0" | [1-9] [0-9]*)"#,
        ]
    );
}

#[test]
fn arrays_enums_any_of_and_empty_objects_have_their_own_shapes() {
    let grammar = grammar_of(json!({
        "type": "object",
        "properties": {
            "step": {"type": "string", "enum": ["ask", "say \"hi\"", "back\\slash"]},
            "tags": {"type": "array", "items": {"type": "number"}},
            "either": {"anyOf": [{"type": "null"}, {"type": "object", "properties": {}, "required": [], "additionalProperties": false}]}
        },
        "required": ["step", "tags", "either"],
        "additionalProperties": false
    }));
    let lines = rules(&grammar);
    assert!(lines.contains(&r#"node-1 ::= "null""#), "{grammar}");
    assert!(lines.contains(&r#"node-2 ::= "{" ws "}""#), "{grammar}");
    assert!(lines.contains(&"node-3 ::= node-1 | node-2"), "{grammar}");
    assert!(
        lines.contains(&r#"node-4 ::= "\"ask\"" | "\"say \\\"hi\\\"\"" | "\"back\\\\slash\"""#),
        "{grammar}"
    );
    assert!(lines.contains(&"node-5 ::= number"), "{grammar}");
    assert!(
        lines.contains(&r#"node-6 ::= "[" ws (node-5 (ws "," ws node-5)*)? ws "]""#),
        "{grammar}"
    );
    assert_eq!(lines[0], "root ::= ws node-7 ws");
}

#[test]
fn every_rule_name_is_defined_once_and_every_reference_is_defined() {
    let grammar = grammar_of(json!({
        "type": "object",
        "properties": {
            "jobs": {"type": "array", "items": {"type": "object", "properties": {
                "question": {"type": "string"},
                "terms": {"type": "array", "items": {"type": "string"}}
            }, "required": ["question", "terms"], "additionalProperties": false}},
            "text": {"type": "string"}
        },
        "required": ["jobs", "text"],
        "additionalProperties": false
    }));
    let defined: Vec<&str> = grammar
        .lines()
        .map(|line| line.split(" ::= ").next().unwrap())
        .collect();
    let mut unique = defined.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), defined.len(), "{grammar}");
    for line in grammar.lines() {
        let body = line.split(" ::= ").nth(1).unwrap();
        for word in body.split_whitespace() {
            let word = word.trim_matches(|character| "()*?|".contains(character));
            if word.starts_with("node-") {
                assert!(defined.contains(&word), "{word} in {grammar}");
            }
        }
    }
}
