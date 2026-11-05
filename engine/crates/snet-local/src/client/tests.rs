use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use snet_testkit::loopback::{
    LoopbackHandler, LoopbackObservations, LoopbackReply, LoopbackServer,
};
use snet_wire::{ConversationMessage, ConversationRole, ReplySchema};

use super::*;
use crate::reply::ListedModel;

const MARKER: &str = "PRIVATE_MATERIAL_MARKER";
const DIGEST: &str = "3a256cc5d3b69933c87c12e52ef330494a3f91361020dffc51819c867088b8e0";

#[derive(Clone)]
enum Chat {
    Reply(Vec<u8>),
    Stall,
}

fn response(status: u16, content_type: &str, body: &[u8]) -> Vec<u8> {
    let mut bytes = format!("HTTP/1.1 {status} X\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n", body.len()).into_bytes();
    bytes.extend_from_slice(body);
    bytes
}

fn tags(name: &str, digest: &str) -> Vec<u8> {
    let body = json!({"models": [
        {"name": "other:1b", "model": "other:1b", "digest": DIGEST, "size": 1},
        {"name": name, "model": name, "digest": digest, "size": 1, "details": {"family": "x"}}
    ]});
    response(200, "application/json", body.to_string().as_bytes())
}

fn chat(content: &str, done_reason: &str, model: &str, eval_count: u64) -> Vec<u8> {
    let body = json!({"model": model, "created_at": "2026-11-06T00:00:00Z",
        "message": {"role": "assistant", "content": content},
        "done": true, "done_reason": done_reason, "total_duration": 1,
        "prompt_eval_count": 40, "eval_count": eval_count});
    response(200, "application/json", body.to_string().as_bytes())
}

struct Server {
    kit: LoopbackServer,
}

impl Server {
    /// The tags route answers at once; the chat route after 100 ms, so
    /// concurrent requests overlap, or never.
    async fn start(tags_reply: Vec<u8>, chat: Chat) -> Self {
        let handler: LoopbackHandler = Arc::new(move |request| {
            if request.request_line.starts_with("GET /api/tags") {
                return LoopbackReply::Bytes(tags_reply.clone());
            }
            match &chat {
                Chat::Reply(bytes) => {
                    LoopbackReply::After(Duration::from_millis(100), bytes.clone())
                }
                Chat::Stall => LoopbackReply::Stall,
            }
        });
        Self {
            kit: LoopbackServer::start(handler).await.unwrap(),
        }
    }

    fn observations(&self) -> &LoopbackObservations {
        &self.kit.observations
    }

    fn model(&self, name: &str) -> LocalModel {
        let host = self.kit.address.to_string();
        LocalModel::new(LocalConfig::ollama(name, Some(&host)).unwrap())
    }
}

fn schema() -> ReplySchema {
    ReplySchema::new("part", json!({"type": "object", "properties": {"part": {"type": "string"}}, "required": ["part"], "additionalProperties": false})).unwrap()
}

fn request() -> LocalRequest {
    LocalRequest {
        instructions: "Extract the failed part.".to_string(),
        conversation: vec![ConversationMessage {
            role: ConversationRole::User,
            text: format!("Log: {MARKER} seal leak."),
        }],
        reply_schema: schema(),
        max_output_tokens: 64,
    }
}

async fn failure(tags_reply: Vec<u8>, chat_reply: Vec<u8>) -> LocalError {
    let server = Server::start(tags_reply, Chat::Reply(chat_reply)).await;
    server.model("qwen3:4b").ask(&request()).await.unwrap_err()
}

#[tokio::test]
async fn an_accepted_answer_carries_the_object_tokens_and_digest() {
    let server = Server::start(
        tags("qwen3:4b", DIGEST),
        Chat::Reply(chat("{\"part\":\"seal\"}", "stop", "qwen3:4b", 5)),
    )
    .await;
    let answer = server.model("qwen3:4b").ask(&request()).await.unwrap();
    assert_eq!(
        answer.object["part"],
        snet_wire::JsonValue::String("seal".to_string())
    );
    assert_eq!((answer.prompt_tokens, answer.completion_tokens), (40, 5));
    assert_eq!(
        answer.model,
        ListedModel {
            name: "qwen3:4b".to_string(),
            digest: DIGEST.to_string()
        }
    );
    assert_eq!(server.observations().connections(), 2);
    let requests = server.observations().requests();
    assert!(requests[1].request_line.starts_with("POST /api/chat"));
    assert!(String::from_utf8_lossy(&requests[1].body).contains(MARKER));
    assert!(!format!("{answer:?}").contains("seal"));
}

#[tokio::test]
async fn a_name_without_a_tag_means_latest() {
    let server = Server::start(
        tags("qwen3:latest", DIGEST),
        Chat::Reply(chat("{\"part\":\"seal\"}", "stop", "qwen3:latest", 5)),
    )
    .await;
    let answer = server.model("qwen3").ask(&request()).await.unwrap();
    assert_eq!(answer.model.name, "qwen3:latest");
}

#[tokio::test]
async fn a_missing_model_names_the_pull_command() {
    let error = failure(tags("llama3:8b", DIGEST), Vec::new()).await;
    assert_eq!(
        error,
        LocalError::ModelNotListed {
            model: "qwen3:4b".to_string()
        }
    );
    assert!(error.to_string().contains("ollama pull qwen3:4b"));
}

#[tokio::test]
async fn a_prompt_count_left_out_for_a_cached_prompt_is_zero() {
    let body = json!({"model": "qwen3:4b", "message": {"role": "assistant", "content": "{\"part\":\"seal\"}"},
        "done": true, "done_reason": "stop", "eval_count": 3});
    let server = Server::start(
        tags("qwen3:4b", DIGEST),
        Chat::Reply(response(
            200,
            "application/json",
            body.to_string().as_bytes(),
        )),
    )
    .await;
    let answer = server.model("qwen3:4b").ask(&request()).await.unwrap();
    assert_eq!((answer.prompt_tokens, answer.completion_tokens), (0, 3));
}

#[tokio::test]
async fn bad_answers_are_refused() {
    let ok_tags = || tags("qwen3:4b", DIGEST);
    assert_eq!(
        failure(ok_tags(), chat("{\"part\":\"se", "length", "qwen3:4b", 64)).await,
        LocalError::Truncated
    );
    assert!(matches!(
        failure(ok_tags(), chat("{\"part\":7}", "stop", "qwen3:4b", 5)).await,
        LocalError::ReplyBreaksSchema { .. }
    ));
    assert!(matches!(
        failure(ok_tags(), chat("not json", "stop", "qwen3:4b", 5)).await,
        LocalError::MalformedReply { .. }
    ));
    assert!(matches!(
        failure(
            ok_tags(),
            chat("{\"part\":\"seal\"}", "stop", "other:1b", 5)
        )
        .await,
        LocalError::MalformedReply { .. }
    ));
    assert!(matches!(
        failure(
            ok_tags(),
            chat("{\"part\":\"seal\"}", "stop", "qwen3:4b", 65)
        )
        .await,
        LocalError::MalformedReply { .. }
    ));
    assert!(matches!(
        failure(
            ok_tags(),
            chat("{\"part\":\"seal\"}", "unload", "qwen3:4b", 5)
        )
        .await,
        LocalError::MalformedReply { .. }
    ));
    assert!(matches!(
        failure(tags("qwen3:4b", "not-a-digest"), Vec::new()).await,
        LocalError::MalformedReply { .. }
    ));
}

#[tokio::test]
async fn ollama_errors_carry_their_status_and_short_message() {
    let error = failure(
        tags("qwen3:4b", DIGEST),
        response(
            500,
            "application/json",
            br#"{"error":"model requires more system memory"}"#,
        ),
    )
    .await;
    assert_eq!(
        error,
        LocalError::Server {
            status: 500,
            message: Some("model requires more system memory".to_string())
        }
    );
}

#[tokio::test]
async fn an_unreachable_server_is_named_with_its_address() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    let model =
        LocalModel::new(LocalConfig::ollama("qwen3:4b", Some(&address.to_string())).unwrap());
    let error = model.ask(&request()).await.unwrap_err();
    assert!(matches!(error, LocalError::Connect { address: named, .. } if named == address));
    assert!(error.to_string().contains("ollama serve"));
}

#[tokio::test]
async fn a_stalled_answer_times_out() {
    let server = Server::start(tags("qwen3:4b", DIGEST), Chat::Stall).await;
    let config = LocalConfig::ollama("qwen3:4b", Some(&server.kit.address.to_string())).unwrap();
    let bounds = crate::LocalBounds {
        total_timeout_seconds: 1,
        ..config.bounds()
    };
    let model = LocalModel::new(config.with_bounds(bounds).unwrap());
    let error = model.ask(&request()).await.unwrap_err();
    assert_eq!(error, LocalError::Timeout);
}

#[tokio::test]
async fn concurrent_requests_stay_within_the_permits() {
    let server = Server::start(
        tags("qwen3:4b", DIGEST),
        Chat::Reply(chat("{\"part\":\"seal\"}", "stop", "qwen3:4b", 5)),
    )
    .await;
    let model = Arc::new(server.model("qwen3:4b"));
    let mut tasks = Vec::new();
    for _ in 0..10 {
        let model = Arc::clone(&model);
        tasks.push(tokio::spawn(async move { model.ask(&request()).await }));
    }
    for task in tasks {
        task.await.unwrap().unwrap();
    }
    let most = server
        .observations()
        .most_in_flight
        .load(std::sync::atomic::Ordering::SeqCst);
    assert!(most <= 4, "{most} chat requests were in flight at once");
}
