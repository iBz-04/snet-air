//! A live run against Ollama on this machine. Run it by hand with a pulled
//! model: `cargo test -p snet-local --test live -- --ignored`, choosing the
//! model with `SNET_LIVE_OLLAMA_MODEL` (default `qwen3:4b`).

use serde_json::json;
use snet_local::{
    ConversationMessage, ConversationRole, LocalConfig, LocalModel, LocalRequest, ReplySchema,
};

#[tokio::test]
#[ignore = "needs Ollama running with the model pulled"]
async fn ollama_answers_within_the_schema() {
    let model_name =
        std::env::var("SNET_LIVE_OLLAMA_MODEL").unwrap_or_else(|_| "qwen3:4b".to_string());
    let host = std::env::var("OLLAMA_HOST").ok();
    let model = LocalModel::new(LocalConfig::ollama(&model_name, host.as_deref()).unwrap());
    let schema = ReplySchema::new(
        "part",
        json!({"type": "object", "properties": {"part": {"type": "string"}}, "required": ["part"], "additionalProperties": false}),
    )
    .unwrap();
    let request = LocalRequest {
        instructions: "Name the part that failed, in one or two words.".to_string(),
        conversation: vec![ConversationMessage {
            role: ConversationRole::User,
            text: "Maintenance log, line 4: the pump stopped because its shaft seal leaked."
                .to_string(),
        }],
        reply_schema: schema,
        max_output_tokens: 128,
    };
    let answer = model.ask(&request).await.unwrap();
    println!("{:?} {:?}", answer, answer.object);
    assert!(answer.completion_tokens <= 128);
    assert_eq!(answer.model.digest.len(), 64);
}

/// Run by hand: `SNET_LIVE_GGUF=/path/model.gguf cargo test -p snet-local
/// --features gguf --test live -- --ignored`.
#[cfg(feature = "gguf")]
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a GGUF file named by SNET_LIVE_GGUF"]
async fn a_gguf_model_answers_within_the_schema_and_stops_at_the_cap() {
    let path = std::env::var("SNET_LIVE_GGUF").expect("SNET_LIVE_GGUF names a GGUF file");
    let model =
        LocalModel::new(LocalConfig::gguf(std::path::Path::new(&path)).expect("a GGUF file"));
    let listed = model.prepare().await.unwrap();
    assert_eq!(listed.digest.len(), 64);
    let schema = ReplySchema::new(
        "report",
        json!({"type": "object", "properties": {
            "part": {"type": "string"},
            "severity": {"type": "string", "enum": ["low", "high"]},
            "lines": {"type": "array", "items": {"type": "integer"}}
        }, "required": ["part", "severity", "lines"], "additionalProperties": false}),
    )
    .unwrap();
    let request = LocalRequest {
        instructions: "Answer with the failed part, how severe it is and the log line numbers that mention it.".to_string(),
        conversation: vec![ConversationMessage {
            role: ConversationRole::User,
            text: "Line 3: routine check. Line 4: the pump stopped because its shaft seal leaked badly.".to_string(),
        }],
        reply_schema: schema,
        max_output_tokens: 256,
    };
    let started = std::time::Instant::now();
    let answer = model.ask(&request).await.unwrap();
    println!(
        "{:?} {:?} in {:?}",
        answer,
        answer.object,
        started.elapsed()
    );
    assert!(answer.completion_tokens <= 256);
    assert!(answer.prompt_tokens > 0);

    let mut tiny = request.clone();
    tiny.max_output_tokens = 3;
    assert_eq!(
        model.ask(&tiny).await.unwrap_err(),
        snet_local::LocalError::Truncated
    );
    let again = model.ask(&request).await.unwrap();
    assert_eq!(again.object, answer.object, "greedy answers repeat");
}
