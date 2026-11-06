use proptest::prelude::*;
use serde_json::{Value, json};

use super::*;
use crate::request::{AcceptedTurn, CloudSchema, CloudTurn};
use crate::test_support::{ANTHROPIC_ENDPOINT, load, openai_endpoint, outbound};

const TEST_OUTPUT_CAP: u64 = 4096;

fn schema() -> ReplySchema {
    cloud_schema().checked().clone()
}

fn cloud_schema() -> CloudSchema {
    let schema = json!({
        "type": "object",
        "properties": {"answer": {"type": "string"}},
        "required": ["answer"],
        "additionalProperties": false
    });
    CloudSchema::new("answer", outbound(&schema.to_string())).unwrap()
}

fn earlier_reply() -> CloudTurn {
    let mut object = crate::json::JsonObject::new();
    object.insert(
        "question".to_string(),
        crate::json::JsonValue::String("Which log?".to_string()),
    );
    CloudTurn::Assistant(AcceptedTurn::accepted(
        &object,
        snet_pseudonym::Destination::new("planner", "api.anthropic.com"),
    ))
}

fn request() -> CloudRequest {
    CloudRequest {
        instructions: outbound("Plan the work."),
        conversation: vec![
            CloudTurn::User(outbound("What failed?")),
            earlier_reply(),
            CloudTurn::User(outbound("The pump log.")),
        ],
        reply_schema: cloud_schema(),
        max_output_tokens: 1_000,
    }
}

fn body_json(config: &EndpointConfig, request: &CloudRequest) -> Value {
    serde_json::from_slice(&build_body(config, request).unwrap()).unwrap()
}

#[test]
fn the_openai_chat_body_holds_exactly_the_fields_of_cld_31() {
    let config = load(&openai_endpoint());
    let expected = json!({
        "model": "claude-opus-5-5",
        "messages": [
            {"role": "developer", "content": "Plan the work."},
            {"role": "user", "content": "What failed?"},
            {"role": "assistant", "content": "{\"question\":\"Which log?\"}"},
            {"role": "user", "content": "The pump log."}
        ],
        "max_completion_tokens": 1000,
        "n": 1,
        "store": false,
        "response_format": {
            "type": "json_schema",
            "json_schema": {"name": "answer", "schema": schema().as_json(), "strict": true}
        }
    });
    assert_eq!(body_json(&config, &request()), expected);

    let without_store = openai_endpoint()
        .replace("send_store_false = true", "send_store_false = false")
        .replace(
            "instruction_role = \"developer\"",
            "instruction_role = \"system\"",
        );
    let body = body_json(&load(&without_store), &request());
    assert!(body.get("store").is_none());
    assert_eq!(body["messages"][0]["role"], "system");
}

#[test]
fn the_melious_body_caps_output_with_max_tokens_in_the_system_role() {
    let choice = crate::config::PresetChoice {
        provider: crate::config::Provider::Melious,
        model: "glm-5.3".to_string(),
        base_url: None,
        effort: "low".to_string(),
        task_limits: crate::budget::TaskLimits {
            requests: 12,
            bytes_sent: 2 * 1024 * 1024,
            input_tokens: 400_000,
            output_tokens: 64_000,
            cost_micro: 0,
        },
    };
    let config = EndpointConfig::preset(&choice).unwrap();
    let expected = json!({
        "model": "glm-5.3",
        "messages": [
            {"role": "system", "content": "Plan the work."},
            {"role": "user", "content": "What failed?"},
            {"role": "assistant", "content": "{\"question\":\"Which log?\"}"},
            {"role": "user", "content": "The pump log."}
        ],
        "max_tokens": 1000,
        "n": 1,
        "reasoning_effort": "low",
        "response_format": {
            "type": "json_schema",
            "json_schema": {"name": "answer", "schema": schema().as_json(), "strict": true}
        }
    });
    assert_eq!(body_json(&config, &request()), expected);
}

#[test]
fn the_anthropic_messages_body_holds_exactly_the_fields_of_cld_32() {
    let config = load(ANTHROPIC_ENDPOINT);
    let expected = json!({
        "model": "claude-opus-5-5",
        "max_tokens": 1000,
        "system": "Plan the work.",
        "messages": [
            {"role": "user", "content": "What failed?"},
            {"role": "assistant", "content": "{\"question\":\"Which log?\"}"},
            {"role": "user", "content": "The pump log."}
        ],
        "output_config": {"format": {"type": "json_schema", "schema": schema().as_json()}}
    });
    assert_eq!(body_json(&config, &request()), expected);
}

#[test]
fn empty_text_and_bad_output_caps_are_refused_before_recording() {
    let config = load(ANTHROPIC_ENDPOINT);
    let mut cases: Vec<(CloudRequest, &str)> = Vec::new();
    let mut empty_instructions = request();
    empty_instructions.instructions = outbound(" \n");
    cases.push((empty_instructions, "instructions"));
    let mut no_messages = request();
    no_messages.conversation.clear();
    cases.push((no_messages, "no message"));
    let mut empty_message = request();
    empty_message.conversation[2] = CloudTurn::User(outbound(""));
    cases.push((empty_message, "message 2"));
    let mut zero_cap = request();
    zero_cap.max_output_tokens = 0;
    cases.push((zero_cap, "output token cap"));
    let mut over_cap = request();
    over_cap.max_output_tokens = 8_193;
    cases.push((over_cap, "output token cap"));
    for (request, expected) in cases {
        match build_body(&config, &request) {
            Err(Failure::RequestInvalid { reason }) => {
                assert!(reason.contains(expected), "{reason}")
            }
            other => panic!("{expected}: {other:?}"),
        }
    }
}

fn openai_reply(content: &str, finish_reason: &str) -> String {
    json!({
        "id": "chatcmpl-1",
        "model": "served-model",
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": content, "refusal": null},
            "finish_reason": finish_reason
        }],
        "usage": {"prompt_tokens": 120, "completion_tokens": 30, "total_tokens": 150}
    })
    .to_string()
}

fn anthropic_reply(blocks: Value, stop_reason: &str) -> String {
    json!({
        "id": "msg_1",
        "type": "message",
        "role": "assistant",
        "model": "served-model",
        "content": blocks,
        "stop_reason": stop_reason,
        "usage": {
            "input_tokens": 100,
            "output_tokens": 30,
            "cache_creation_input_tokens": 15,
            "cache_read_input_tokens": 5
        }
    })
    .to_string()
}

fn text_block(text: &str) -> Value {
    json!({"type": "text", "text": text})
}

fn accepted(format: WireFormat, body: &str) -> (ReportedUsage, AcceptedReply) {
    let parsed = parse_success(format, body.as_bytes(), &schema(), TEST_OUTPUT_CAP).unwrap();
    (parsed.usage, parsed.result.unwrap())
}

fn refused(format: WireFormat, body: &str) -> Failure {
    match parse_success(format, body.as_bytes(), &schema(), TEST_OUTPUT_CAP) {
        Ok(parsed) => parsed.result.unwrap_err(),
        Err(failure) => failure,
    }
}

#[test]
fn accepted_replies_return_the_object_usage_and_model() {
    let (usage, reply) = accepted(
        WireFormat::OpenaiChat,
        &openai_reply(r#"{"answer":"seal"}"#, "stop"),
    );
    assert_eq!(
        usage,
        ReportedUsage {
            input_tokens: 120,
            output_tokens: 30,
            thinking_tokens: None
        }
    );
    assert_eq!(reply.model, "served-model");
    assert_eq!(
        reply.object["answer"],
        JsonValue::String("seal".to_string())
    );

    let blocks = json!([{"type": "thinking", "thinking": "...", "signature": "s"}, text_block(r#"{"answer":"seal"}"#)]);
    let (usage, reply) = accepted(
        WireFormat::AnthropicMessages,
        &anthropic_reply(blocks, "end_turn"),
    );
    assert_eq!(
        usage,
        ReportedUsage {
            input_tokens: 120,
            output_tokens: 30,
            thinking_tokens: None
        }
    );
    assert_eq!(
        reply.object["answer"],
        JsonValue::String("seal".to_string())
    );
}

#[test]
fn openai_stop_reasons_refusals_and_choice_counts() {
    let format = WireFormat::OpenaiChat;
    let object = r#"{"answer":"seal"}"#;
    assert!(matches!(
        refused(format, &openai_reply(object, "length")),
        Failure::Truncated(_)
    ));
    assert!(matches!(
        refused(format, &openai_reply(object, "content_filter")),
        Failure::Filtered
    ));
    assert!(matches!(
        refused(format, &openai_reply(object, "tool_calls")),
        Failure::ToolRequested
    ));
    match refused(format, &openai_reply(object, "odd_reason")) {
        Failure::UnexpectedStop { reason } => assert_eq!(reason.as_deref(), Some("odd_reason")),
        other => panic!("{other:?}"),
    }
    match refused(format, &openai_reply(object, "free text with spaces")) {
        Failure::UnexpectedStop { reason } => assert_eq!(reason, None),
        other => panic!("{other:?}"),
    }

    let mut refusal: Value = serde_json::from_str(&openai_reply(object, "stop")).unwrap();
    refusal["choices"][0]["message"]["refusal"] = json!("I will not.");
    refusal["choices"][0]["message"]["content"] = Value::Null;
    assert!(matches!(
        refused(format, &refusal.to_string()),
        Failure::Refused
    ));

    let mut two: Value = serde_json::from_str(&openai_reply(object, "stop")).unwrap();
    let choice = two["choices"][0].clone();
    two["choices"].as_array_mut().unwrap().push(choice);
    assert!(matches!(
        refused(format, &two.to_string()),
        Failure::MalformedReply { .. }
    ));

    let mut none: Value = serde_json::from_str(&openai_reply(object, "stop")).unwrap();
    none["choices"] = json!([]);
    assert!(matches!(
        refused(format, &none.to_string()),
        Failure::MalformedReply { .. }
    ));
}

#[test]
fn anthropic_stop_reasons_and_text_block_counts() {
    let format = WireFormat::AnthropicMessages;
    let one = json!([text_block(r#"{"answer":"seal"}"#)]);
    for (stop_reason, expected) in [
        ("max_tokens", "truncated"),
        ("model_context_window_exceeded", "truncated"),
        ("refusal", "refused"),
        ("tool_use", "tool_requested"),
        ("pause_turn", "paused"),
        ("stop_sequence", "unexpected_stop"),
    ] {
        let failure = refused(format, &anthropic_reply(one.clone(), stop_reason));
        assert_eq!(failure.kind(), expected, "{stop_reason}");
    }
    let two = json!([
        text_block(r#"{"answer":"a"}"#),
        text_block(r#"{"answer":"b"}"#)
    ]);
    assert!(matches!(
        refused(format, &anthropic_reply(two, "end_turn")),
        Failure::MalformedReply { .. }
    ));
    let zero = json!([{"type": "thinking", "thinking": "..."}]);
    assert!(matches!(
        refused(format, &anthropic_reply(zero, "end_turn")),
        Failure::MalformedReply { .. }
    ));
}

#[test]
fn missing_usage_duplicate_keys_and_bad_text_are_refused() {
    let format = WireFormat::OpenaiChat;
    let mut no_usage: Value =
        serde_json::from_str(&openai_reply(r#"{"answer":"a"}"#, "stop")).unwrap();
    no_usage.as_object_mut().unwrap().remove("usage");
    assert!(
        parse_success(
            format,
            no_usage.to_string().as_bytes(),
            &schema(),
            TEST_OUTPUT_CAP
        )
        .is_err()
    );

    let duplicate_in_wrapper = openai_reply(r#"{"answer":"a"}"#, "stop").replacen(
        "\"model\":\"served-model\"",
        "\"model\":\"served-model\",\"model\":\"other\"",
        1,
    );
    assert!(duplicate_in_wrapper.contains("\"model\":\"other\""));
    assert!(
        parse_success(
            format,
            duplicate_in_wrapper.as_bytes(),
            &schema(),
            TEST_OUTPUT_CAP
        )
        .is_err()
    );

    let duplicate_in_object = openai_reply(r#"{"answer":"a","answer":"b"}"#, "stop");
    assert!(matches!(
        refused(format, &duplicate_in_object),
        Failure::MalformedReply { .. }
    ));
    assert!(matches!(
        refused(format, &openai_reply("[1, 2]", "stop")),
        Failure::MalformedReply { .. }
    ));
    assert!(matches!(
        refused(format, &openai_reply("not json", "stop")),
        Failure::MalformedReply { .. }
    ));
    assert!(matches!(
        refused(format, &openai_reply(r#"{"answer":7}"#, "stop")),
        Failure::ReplyBreaksSchema { .. }
    ));

    let mut not_utf8 = openai_reply(r#"{"answer":"a"}"#, "stop").into_bytes();
    not_utf8.insert(5, 0xff);
    assert!(parse_success(format, &not_utf8, &schema(), TEST_OUTPUT_CAP).is_err());
}

#[test]
fn refusal_errors_carry_no_reply_text() {
    let marker = "PRIVATE_REPLY_MARKER";
    let bodies = [
        openai_reply(&format!(r#"{{"answer":"{marker}","{marker}":1}}"#), "stop"),
        openai_reply(&format!(r#"{{"answer":{marker}}}"#), "stop"),
        openai_reply(r#"{"answer":"x"}"#, &format!("{marker} as free text")),
        openai_reply(&format!(r#"{{"answer":"{marker}"}}"#), "length"),
    ];
    for body in bodies {
        let failure = refused(WireFormat::OpenaiChat, &body);
        assert!(!failure.to_string().contains(marker), "{failure}");
        assert!(!format!("{failure:?}").contains(marker), "{failure:?}");
    }
}

fn provider(format: WireFormat, status: u16, body: &str) -> (Option<String>, Option<String>, bool) {
    match provider_failure(format, status, body.as_bytes(), Some("req_1".to_string())) {
        Failure::Provider {
            status: kept_status,
            error_type,
            error_code,
            request_id,
            retryable,
        } => {
            assert_eq!(kept_status, status);
            assert_eq!(request_id.as_deref(), Some("req_1"));
            (error_type, error_code, retryable)
        }
        other => panic!("{other:?}"),
    }
}

fn openai_error(kind: &str, code: Option<&str>) -> String {
    json!({"error": {"message": "text", "type": kind, "code": code, "param": null}}).to_string()
}

fn anthropic_error(kind: &str) -> String {
    json!({"type": "error", "error": {"type": kind, "message": "text"}}).to_string()
}

#[test]
fn openai_error_bodies_keep_type_code_and_retryability() {
    let format = WireFormat::OpenaiChat;
    let cases = [
        (400, openai_error("invalid_request_error", None), false),
        (
            401,
            openai_error("invalid_request_error", Some("invalid_api_key")),
            false,
        ),
        (408, openai_error("timeout", None), true),
        (409, openai_error("conflict", None), true),
        (
            429,
            openai_error("requests", Some("rate_limit_exceeded")),
            true,
        ),
        (429, openai_error("rate_limit_error", None), true),
        (429, openai_error("slow_down", None), true),
        (
            429,
            openai_error("insufficient_quota", Some("insufficient_quota")),
            false,
        ),
        (
            429,
            openai_error("requests", Some("spend_limit_reached")),
            false,
        ),
        (500, openai_error("server_error", None), true),
        (503, openai_error("server_error", None), true),
    ];
    for (status, body, retryable) in cases {
        let (error_type, _, kept_retryable) = provider(format, status, &body);
        assert!(error_type.is_some(), "{status} {body}");
        assert_eq!(kept_retryable, retryable, "{status} {body}");
    }
    let (error_type, error_code, _) = provider(
        format,
        401,
        &openai_error("invalid_request_error", Some("invalid_api_key")),
    );
    assert_eq!(error_type.as_deref(), Some("invalid_request_error"));
    assert_eq!(error_code.as_deref(), Some("invalid_api_key"));
}

#[test]
fn anthropic_error_bodies_keep_type_and_retryability() {
    let format = WireFormat::AnthropicMessages;
    let cases = [
        (400, "invalid_request_error", false),
        (401, "authentication_error", false),
        (408, "timeout_error", false),
        (429, "rate_limit_error", true),
        (429, "enforced_spend_limit_reached", false),
        (500, "api_error", true),
        (503, "api_error", true),
        (504, "timeout_error", true),
        (529, "overloaded_error", true),
    ];
    for (status, kind, retryable) in cases {
        let (error_type, error_code, kept_retryable) =
            provider(format, status, &anthropic_error(kind));
        assert_eq!(error_type.as_deref(), Some(kind));
        assert_eq!(error_code, None);
        assert_eq!(kept_retryable, retryable, "{status} {kind}");
    }
}

#[test]
fn a_non_json_error_body_keeps_its_status_and_request_id() {
    let html = "<html><body>502 Bad Gateway</body></html>";
    let (error_type, error_code, retryable) = provider(WireFormat::OpenaiChat, 502, html);
    assert_eq!((error_type, error_code, retryable), (None, None, true));
    let (error_type, _, retryable) = provider(WireFormat::AnthropicMessages, 403, html);
    assert_eq!((error_type, retryable), (None, false));
    let free_text = openai_error("type with spaces and a marker", Some("x y"));
    let (error_type, error_code, _) = provider(WireFormat::OpenaiChat, 400, &free_text);
    assert_eq!((error_type, error_code), (None, None));
}

fn arbitrary_json() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        any::<i64>().prop_map(|number| json!(number)),
        any::<u64>().prop_map(|number| json!(number)),
        any::<f64>().prop_filter_map("finite", |number| serde_json::Number::from_f64(number)
            .map(Value::Number)),
        ".{0,12}".prop_map(Value::String),
    ];
    leaf.prop_recursive(5, 64, 6, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..6).prop_map(Value::Array),
            prop::collection::btree_map("[a-z_]{0,10}", inner, 0..6)
                .prop_map(|members| Value::Object(members.into_iter().collect())),
        ]
    })
}

fn parse_everything(body: &[u8]) {
    for format in [WireFormat::OpenaiChat, WireFormat::AnthropicMessages] {
        if let Ok(parsed) = parse_success(format, body, &schema(), TEST_OUTPUT_CAP) {
            let _accepted_or_refused = parsed.result;
        }
        let _failure = provider_failure(format, 500, body, None);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2_000))]

    #[test]
    fn parsers_never_panic_on_random_bytes(body in prop::collection::vec(any::<u8>(), 0..512)) {
        parse_everything(&body);
    }

    #[test]
    fn parsers_never_panic_on_random_json(value in arbitrary_json()) {
        parse_everything(value.to_string().as_bytes());
    }

    #[test]
    fn parsers_never_panic_on_random_fields_in_real_shapes(
        content in arbitrary_json(),
        choices in arbitrary_json(),
        usage in arbitrary_json(),
        text in ".{0,40}",
    ) {
        let mut openai: Value = serde_json::from_str(&openai_reply(&text, "stop")).unwrap();
        openai["choices"][0]["message"]["content"] = content.clone();
        parse_everything(openai.to_string().as_bytes());
        openai["choices"] = choices;
        openai["usage"] = usage.clone();
        parse_everything(openai.to_string().as_bytes());

        let mut anthropic: Value = serde_json::from_str(&anthropic_reply(json!([text_block(&text)]), "end_turn")).unwrap();
        anthropic["content"] = json!([{"type": "text", "text": content}]);
        parse_everything(anthropic.to_string().as_bytes());
        anthropic["usage"] = usage;
        parse_everything(anthropic.to_string().as_bytes());
        parse_everything(openai_reply(&text, "stop").as_bytes());
    }
}

fn set_effort(text: &str, effort: &str) -> String {
    text.replace(
        "effort = \"model_default\"",
        &format!("effort = \"{effort}\""),
    )
}

#[test]
fn an_effort_is_sent_in_the_providers_field_only_when_set() {
    let anthropic_low = load(&set_effort(ANTHROPIC_ENDPOINT, "low"));
    let body = body_json(&anthropic_low, &request());
    assert_eq!(body["output_config"]["effort"], "low");
    assert_eq!(body["output_config"]["format"]["type"], "json_schema");
    let anthropic_default = body_json(&load(ANTHROPIC_ENDPOINT), &request());
    assert!(anthropic_default["output_config"].get("effort").is_none());

    let openai_high = load(&set_effort(&openai_endpoint(), "high"));
    let body = body_json(&openai_high, &request());
    assert_eq!(body["reasoning_effort"], "high");
    let openai_default = body_json(&load(&openai_endpoint()), &request());
    assert!(openai_default.get("reasoning_effort").is_none());
}

fn cut_anthropic(stop_reason: &str, thinking: Option<u64>) -> String {
    let mut usage = json!({"input_tokens": 120, "output_tokens": 4096});
    if let Some(tokens) = thinking {
        usage["output_tokens_details"] = json!({"thinking_tokens": tokens});
    }
    json!({"id": "msg_1", "type": "message", "role": "assistant", "model": "served-model",
           "content": [text_block("{\"answer\": \"SECRET-REPLY-TEXT")], "stop_reason": stop_reason,
           "usage": usage})
    .to_string()
}

fn cut_openai(reasoning: Option<u64>) -> String {
    let mut usage = json!({"prompt_tokens": 120, "completion_tokens": 4096, "total_tokens": 4216});
    if let Some(tokens) = reasoning {
        usage["completion_tokens_details"] = json!({"reasoning_tokens": tokens});
    }
    json!({"id": "c", "object": "chat.completion", "model": "served-model",
           "choices": [{"index": 0, "message": {"role": "assistant", "content": "{\"answer\": \"SECRET-REPLY-TEXT", "refusal": null}, "finish_reason": "length"}],
           "usage": usage})
    .to_string()
}

fn truncation(format: WireFormat, body: &str) -> (crate::truncation::TruncatedReply, String) {
    match refused(format, body) {
        Failure::Truncated(cut) => {
            let message = Failure::Truncated(cut).to_string();
            assert!(!message.contains("SECRET-REPLY-TEXT"), "{message}");
            assert_eq!(Failure::Truncated(cut).kind(), "truncated");
            (cut, message)
        }
        other => panic!("expected a cut reply, got {other:?}"),
    }
}

#[test]
fn a_reply_cut_at_the_output_cap_names_its_counts_and_both_remedies() {
    let (cut, message) = truncation(
        WireFormat::AnthropicMessages,
        &cut_anthropic("max_tokens", Some(3987)),
    );
    assert_eq!(cut.stop, crate::truncation::TruncationStop::OutputCap);
    assert_eq!(
        (cut.output_tokens, cut.thinking_tokens, cut.output_cap),
        (4096, Some(3987), TEST_OUTPUT_CAP)
    );
    assert_eq!(
        message,
        "the reply stopped at the output cap of 4096 tokens after 4096 output tokens, about 3987 of them thinking; lower `effort` or raise `max_output_tokens`"
    );
    let (cut, message) = truncation(
        WireFormat::AnthropicMessages,
        &cut_anthropic("max_tokens", None),
    );
    assert_eq!(cut.thinking_tokens, None);
    assert!(!message.contains("thinking;"), "{message}");
}

#[test]
fn a_reply_that_filled_the_context_window_is_not_told_to_raise_the_cap() {
    let (cut, message) = truncation(
        WireFormat::AnthropicMessages,
        &cut_anthropic("model_context_window_exceeded", Some(100)),
    );
    assert_eq!(cut.stop, crate::truncation::TruncationStop::ContextWindow);
    assert!(
        message.contains("filled the model's context window"),
        "{message}"
    );
    assert!(message.contains("send less"), "{message}");
    assert!(!message.contains("max_output_tokens"), "{message}");
}

#[test]
fn an_openai_style_length_stop_names_both_causes_and_the_reasoning_count() {
    let (cut, message) = truncation(WireFormat::OpenaiChat, &cut_openai(Some(4000)));
    assert_eq!(cut.stop, crate::truncation::TruncationStop::Length);
    assert_eq!(cut.thinking_tokens, Some(4000));
    assert_eq!(
        message,
        "the reply stopped at the output cap of 4096 tokens or at the model's context window after 4096 output tokens, about 4000 of them reasoning; lower `effort` or raise `max_output_tokens`, or send less"
    );
    let (cut, _) = truncation(WireFormat::OpenaiChat, &cut_openai(None));
    assert_eq!(cut.thinking_tokens, None);
}

#[test]
fn a_cut_reply_without_usage_stays_malformed() {
    let mut body: Value = serde_json::from_str(&cut_anthropic("max_tokens", Some(1))).unwrap();
    body.as_object_mut().unwrap().remove("usage");
    assert!(matches!(
        refused(WireFormat::AnthropicMessages, &body.to_string()),
        Failure::MalformedReply { .. }
    ));
}
