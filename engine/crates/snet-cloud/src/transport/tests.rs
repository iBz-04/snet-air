use bytes::Bytes;

use super::*;
use crate::secret::ApiKey;
use crate::test_support::{ANTHROPIC_ENDPOINT, load, openai_endpoint};

const KEY: &str = "sk-test-KEYBYTES-0123456789";

fn header_names(request: &PreparedRequest) -> Vec<String> {
    request
        .headers
        .keys()
        .map(|name| name.as_str().to_string())
        .collect()
}

#[test]
fn anthropic_requests_carry_exactly_the_headers_of_cld_13() {
    let config = load(ANTHROPIC_ENDPOINT);
    let key = ApiKey::new(KEY.to_string()).unwrap();
    let request = PreparedRequest::new(&config, &key, Bytes::from_static(b"{}")).unwrap();
    assert_eq!(request.path, "/v1/messages");
    assert_eq!(
        header_names(&request),
        [
            "host",
            "content-type",
            "content-length",
            "authorization",
            "anthropic-version",
            "connection"
        ]
    );
    assert_eq!(request.headers["host"], "api.anthropic.com");
    assert_eq!(request.headers["content-length"], "2");
    assert_eq!(request.headers["anthropic-version"], "2026-10-21");
    assert_eq!(request.headers["connection"], "close");
    assert!(request.headers["authorization"].is_sensitive());
}

#[test]
fn openai_requests_use_the_configured_key_header() {
    let key = ApiKey::new(KEY.to_string()).unwrap();
    let bearer = PreparedRequest::new(&load(&openai_endpoint()), &key, Bytes::new()).unwrap();
    assert_eq!(bearer.path, "/v1/chat/completions");
    assert_eq!(
        header_names(&bearer),
        [
            "host",
            "content-type",
            "content-length",
            "authorization",
            "connection"
        ]
    );

    let api_key_config = openai_endpoint().replace(
        "key_header = \"authorization_bearer\"",
        "key_header = \"api_key\"",
    );
    let api_key = PreparedRequest::new(&load(&api_key_config), &key, Bytes::new()).unwrap();
    assert_eq!(
        header_names(&api_key),
        [
            "host",
            "content-type",
            "content-length",
            "api-key",
            "connection"
        ]
    );
    assert!(api_key.headers["api-key"].is_sensitive());
}

#[test]
fn the_key_never_appears_in_debug_output_or_recordable_headers() {
    let key = ApiKey::new(KEY.to_string()).unwrap();
    assert_eq!(format!("{key:?}"), "ApiKey(redacted)");
    for text in [ANTHROPIC_ENDPOINT.to_string(), openai_endpoint()] {
        let request = PreparedRequest::new(&load(&text), &key, Bytes::from_static(b"{}")).unwrap();
        assert!(!format!("{:?}", request.headers).contains("KEYBYTES"));
        assert!(!format!("{request:?}").contains("KEYBYTES"));
        let recordable = request.recordable_headers();
        assert!(!format!("{recordable:?}").contains("KEYBYTES"));
        assert!(recordable.iter().all(|(name, _)| name != "authorization"));
        assert_eq!(recordable.len(), request.headers.len() - 1);
        let http = request.into_http().unwrap();
        assert!(!format!("{http:?}").contains("KEYBYTES"));
    }
}

#[test]
fn keys_must_be_visible_ascii() {
    assert!(ApiKey::new(String::new()).is_err());
    assert!(ApiKey::new("has space".to_string()).is_err());
    assert!(ApiKey::new("line\nbreak".to_string()).is_err());
    let error = ApiKey::new("sk-ü-KEYBYTES".to_string()).unwrap_err();
    assert!(!error.to_string().contains("KEYBYTES"));
}

#[test]
fn the_wire_length_counts_the_request_line_headers_and_body() {
    let key = ApiKey::new(KEY.to_string()).unwrap();
    let request = PreparedRequest::new(
        &load(ANTHROPIC_ENDPOINT),
        &key,
        Bytes::from_static(b"{\"a\":1}"),
    )
    .unwrap();
    let mut serialized = format!("POST {} HTTP/1.1\r\n", request.path);
    for (name, value) in &request.headers {
        serialized.push_str(&format!("{}: {}\r\n", name, value.to_str().unwrap()));
    }
    serialized.push_str("\r\n{\"a\":1}");
    assert_eq!(request.wire_length(), serialized.len() as u64);
}

#[test]
fn a_named_anthropic_workspace_is_sent_after_the_version_and_recorded() {
    let key = ApiKey::new(KEY.to_string()).unwrap();
    let named = ANTHROPIC_ENDPOINT.replace(
        "anthropic_workspace = \"key_scoped\"",
        "anthropic_workspace = \"wrkspc_01JwQvzr7rXLA5AGx3HKfFUJ\"",
    );
    let request = PreparedRequest::new(&load(&named), &key, Bytes::from_static(b"{}")).unwrap();
    assert_eq!(
        header_names(&request),
        [
            "host",
            "content-type",
            "content-length",
            "authorization",
            "anthropic-version",
            "anthropic-workspace-id",
            "connection"
        ]
    );
    assert_eq!(
        request.headers["anthropic-workspace-id"],
        "wrkspc_01JwQvzr7rXLA5AGx3HKfFUJ"
    );
    assert!(request.recordable_headers().contains(&(
        "anthropic-workspace-id".to_string(),
        "wrkspc_01JwQvzr7rXLA5AGx3HKfFUJ".to_string()
    )));

    let scoped = PreparedRequest::new(&load(ANTHROPIC_ENDPOINT), &key, Bytes::new()).unwrap();
    assert!(scoped.headers.get("anthropic-workspace-id").is_none());
    let openai = PreparedRequest::new(&load(&openai_endpoint()), &key, Bytes::new()).unwrap();
    assert!(openai.headers.get("anthropic-workspace-id").is_none());
}
