use serde_json::json;
use snet_local::{
    ConversationMessage, ConversationRole, JsonObject, JsonValue, LocalRequest, ReplySchema,
};
use snet_protocol::{FailureKind, ProtocolError};

use crate::prompts::{CHECK_PROMPT, LOCAL_PROMPT};
use crate::relay::RelayTask;

const SCHEMA_NAME: &str = "screened_question";
const CHECK_SCHEMA_NAME: &str = "checked_question";
const MAX_CLOUD_QUESTION_CHARACTERS: usize = 1_000;
const MAX_PRIVATE_TERMS: usize = 32;
const MIN_TERM_CHARACTERS: usize = 2;
/// The categories the local model may give a private detail.
pub(crate) const CATEGORIES: [&str; 6] = [
    "PERSON",
    "ORGANISATION",
    "LOCATION",
    "ACCOUNT",
    "SECRET",
    "OTHER",
];

const MAX_LOCAL_ANSWER_CHARACTERS: usize = 32_000;

/// What the local model made of the question: its own answer, or the one
/// question the cloud may read and the private details to replace in it.
pub(crate) enum Screened {
    Local {
        answer: String,
    },
    Cloud {
        cloud_question: String,
        /// Category and text of each private detail, trimmed.
        private_terms: Vec<(String, String)>,
    },
}

fn terms_schema() -> serde_json::Value {
    json!({
        "type": "array",
        "items": {
            "type": "object",
            "properties": {
                "category": {"type": "string", "enum": CATEGORIES},
                "text": {"type": "string"}
            },
            "required": ["category", "text"],
            "additionalProperties": false
        }
    })
}

fn local_request(
    instructions: &str,
    message: serde_json::Value,
    schema_name: &str,
    schema: serde_json::Value,
    max_output_tokens: u64,
) -> Result<LocalRequest, ProtocolError> {
    let reply_schema = ReplySchema::new(schema_name, schema)
        .map_err(|error| ProtocolError::new(FailureKind::Local, error.to_string()))?;
    Ok(LocalRequest {
        instructions: instructions.to_string(),
        conversation: vec![ConversationMessage {
            role: ConversationRole::User,
            text: message.to_string(),
        }],
        reply_schema,
        max_output_tokens,
    })
}

/// The request that asks the local model to screen and condense the
/// question and earlier exchanges.
pub(crate) fn request(task: &RelayTask) -> Result<LocalRequest, ProtocolError> {
    let earlier: Vec<serde_json::Value> = task
        .earlier
        .iter()
        .map(|exchange| json!({"question": exchange.question, "answer": exchange.answer}))
        .collect();
    let message = json!({"question": task.question, "earlier": earlier});
    let schema = json!({
        "type": "object",
        "properties": {
            "local_answer": {"type": "string"},
            "cloud_question": {"type": "string"},
            "private_terms": terms_schema()
        },
        "required": ["local_answer", "cloud_question", "private_terms"],
        "additionalProperties": false
    });
    local_request(
        LOCAL_PROMPT,
        message,
        SCHEMA_NAME,
        schema,
        task.local_output_tokens,
    )
}

/// The request that asks the local model to list any private detail still
/// in the question it wrote, before that question leaves the machine.
pub(crate) fn check_request(
    cloud_question: &str,
    max_output_tokens: u64,
) -> Result<LocalRequest, ProtocolError> {
    let schema = json!({
        "type": "object",
        "properties": {"private_terms": terms_schema()},
        "required": ["private_terms"],
        "additionalProperties": false
    });
    local_request(
        CHECK_PROMPT,
        json!({"question": cloud_question}),
        CHECK_SCHEMA_NAME,
        schema,
        max_output_tokens,
    )
}

/// Checks the local screening reply. Exactly one of `local_answer` and
/// `cloud_question` is filled, and that one is the route: a local answer,
/// or a cloud question with its private details.
pub(crate) fn screened(object: &JsonObject) -> Result<Screened, ProtocolError> {
    let invalid = |detail: &str| ProtocolError::new(FailureKind::InvalidLocalReply, detail);
    let text_of = |field: &str| match object.get(field) {
        Some(JsonValue::String(text)) => Ok(text.trim()),
        _ => Err(invalid("the reply lacks a string field of its schema")),
    };
    let answer = text_of("local_answer")?;
    let cloud_question = text_of("cloud_question")?;
    match (answer.is_empty(), cloud_question.is_empty()) {
        (false, true) => {
            if answer.chars().count() > MAX_LOCAL_ANSWER_CHARACTERS {
                return Err(invalid("`local_answer` is over 32 000 characters"));
            }
            Ok(Screened::Local {
                answer: answer.to_string(),
            })
        }
        (true, false) => {
            if cloud_question.chars().count() > MAX_CLOUD_QUESTION_CHARACTERS {
                return Err(invalid("`cloud_question` is over 1 000 characters"));
            }
            Ok(Screened::Cloud {
                cloud_question: cloud_question.to_string(),
                private_terms: private_terms(object)?,
            })
        }
        _ => Err(invalid(
            "exactly one of `local_answer` and `cloud_question` must be filled",
        )),
    }
}

/// The private details a local reply lists, each trimmed; a detail of
/// fewer than two letters or digits after trimming names no one and is
/// left out.
pub(crate) fn private_terms(object: &JsonObject) -> Result<Vec<(String, String)>, ProtocolError> {
    let invalid = |detail: &str| ProtocolError::new(FailureKind::InvalidLocalReply, detail);
    let Some(JsonValue::Array(items)) = object.get("private_terms") else {
        return Err(invalid("the reply has no array `private_terms`"));
    };
    if items.len() > MAX_PRIVATE_TERMS {
        return Err(invalid("`private_terms` holds more than 32 details"));
    }
    let mut private_terms = Vec::with_capacity(items.len());
    for item in items {
        let JsonValue::Object(term) = item else {
            return Err(invalid("a private detail is not an object"));
        };
        let (Some(JsonValue::String(category)), Some(JsonValue::String(text))) =
            (term.get("category"), term.get("text"))
        else {
            return Err(invalid("a private detail lacks its category or text"));
        };
        if !CATEGORIES.contains(&category.as_str()) {
            return Err(invalid("a private detail has an unknown category"));
        }
        let trimmed = text
            .trim_matches(|character: char| !(character.is_alphabetic() || character.is_numeric()));
        if trimmed.chars().count() >= MIN_TERM_CHARACTERS {
            private_terms.push((category.clone(), trimmed.to_string()));
        }
    }
    Ok(private_terms)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn object(value: serde_json::Value) -> JsonObject {
        match serde_json::from_str::<JsonValue>(&value.to_string()).unwrap() {
            JsonValue::Object(object) => object,
            _ => panic!("not an object"),
        }
    }

    #[test]
    fn details_are_trimmed_and_single_characters_left_out() {
        let screened = screened(&object(json!({
            "local_answer": "",
            "cloud_question": " Should a supplier renegotiate? ",
            "private_terms": [
                {"category": "PERSON", "text": "\"Ayşe Demir,\""},
                {"category": "OTHER", "text": "x"}
            ]
        })))
        .unwrap();
        let Screened::Cloud {
            cloud_question,
            private_terms,
        } = screened
        else {
            panic!("not routed to the cloud");
        };
        assert_eq!(cloud_question, "Should a supplier renegotiate?");
        assert_eq!(
            private_terms,
            vec![("PERSON".to_string(), "Ayşe Demir".to_string())]
        );
    }

    #[test]
    fn a_local_route_carries_the_local_answer() {
        let screened = screened(&object(json!({
            "local_answer": "Hi! How can I help?",
            "cloud_question": "",
            "private_terms": []
        })))
        .unwrap();
        assert!(matches!(screened, Screened::Local { answer } if answer == "Hi! How can I help?"));
    }

    #[test]
    fn replies_off_the_rules_are_refused() {
        for reply in [
            json!({"local_answer": "", "cloud_question": "", "private_terms": []}),
            json!({"local_answer": "a", "cloud_question": "b", "private_terms": []}),
            json!({"local_answer": "", "cloud_question": "q".repeat(1_001), "private_terms": []}),
            json!({"local_answer": "", "cloud_question": "Why?", "private_terms": [{"category": "NAME", "text": "Jo"}]}),
            json!({"local_answer": "", "cloud_question": "Why?"}),
            json!({"local_answer": " ", "cloud_question": " ", "private_terms": []}),
        ] {
            let error = screened(&object(reply)).err().unwrap();
            assert_eq!(error.kind, FailureKind::InvalidLocalReply);
        }
    }
}
