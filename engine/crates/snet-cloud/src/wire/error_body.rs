use super::plain_identifier;
use crate::config::WireFormat;
use crate::errors::Failure;
use crate::json::{self, JsonValue};

/// The failure for a non-2xx reply: the status, the provider's
/// error type and code when the body states them as JSON, the request ID and
/// whether a retry may succeed. The client itself never retries.
pub(crate) fn provider_failure(
    format: WireFormat,
    status: u16,
    body: &[u8],
    request_id: Option<String>,
) -> Failure {
    let (error_type, error_code) = error_fields(body);
    let retryable = is_retryable(format, status, error_type.as_deref(), error_code.as_deref());
    Failure::Provider {
        status,
        error_type,
        error_code,
        request_id,
        retryable,
    }
}

/// Both formats nest the error as `{"error": {"type": ..., "code": ...}}`.
/// A body that is not such JSON, such as a proxy's HTML page, states neither.
fn error_fields(body: &[u8]) -> (Option<String>, Option<String>) {
    let Ok(JsonValue::Object(root)) = json::parse_strict(body) else {
        return (None, None);
    };
    let Some(JsonValue::Object(error)) = root.get("error") else {
        return (None, None);
    };
    let field = |name: &str| match error.get(name) {
        Some(JsonValue::String(text)) => plain_identifier(text),
        Some(JsonValue::Number(number)) => plain_identifier(&number.to_string()),
        _ => None,
    };
    (field("type"), field("code"))
}

fn is_retryable(
    format: WireFormat,
    status: u16,
    error_type: Option<&str>,
    error_code: Option<&str>,
) -> bool {
    let names_a_spending_stop = [error_type, error_code].into_iter().flatten().any(|name| {
        name == "insufficient_quota" || name.contains("spend_limit") || name.contains("billing")
    });
    match (format, status) {
        (WireFormat::OpenaiChat, 408 | 409) => true,
        (_, 429) => !names_a_spending_stop,
        (_, status) => status >= 500,
    }
}
