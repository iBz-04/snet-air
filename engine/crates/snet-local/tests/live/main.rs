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
