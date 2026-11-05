use std::sync::atomic::{AtomicUsize, Ordering};

use tokio::io::{AsyncRead, AsyncReadExt};

/// One HTTP/1.1 request as a test server received it.
#[derive(Debug, Clone)]
pub struct ReceivedRequest {
    pub raw: Vec<u8>,
    pub request_line: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl ReceivedRequest {
    pub fn path(&self) -> &str {
        self.request_line.split(' ').nth(1).unwrap_or_default()
    }
}

/// Reads one request with a content-length body; `None` when the client
/// closed or sent something that is not HTTP.
pub async fn read_request<S: AsyncRead + Unpin>(
    stream: &mut S,
    bytes_read: &AtomicUsize,
) -> Option<ReceivedRequest> {
    let mut raw = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        if let Some(head_end) = find(&raw, b"\r\n\r\n") {
            let head = String::from_utf8(raw[..head_end].to_vec()).ok()?;
            let mut lines = head.split("\r\n");
            let request_line = lines.next()?.to_string();
            let headers: Vec<(String, String)> = lines
                .filter_map(|line| line.split_once(": "))
                .map(|(name, value)| (name.to_string(), value.to_string()))
                .collect();
            let length: usize = headers
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                .and_then(|(_, value)| value.parse().ok())
                .unwrap_or(0);
            let body_start = head_end + 4;
            if raw.len() >= body_start + length {
                let body = raw[body_start..body_start + length].to_vec();
                return Some(ReceivedRequest {
                    raw,
                    request_line,
                    headers,
                    body,
                });
            }
        }
        let read = stream.read(&mut chunk).await.ok()?;
        if read == 0 {
            return None;
        }
        bytes_read.fetch_add(read, Ordering::SeqCst);
        raw.extend_from_slice(&chunk[..read]);
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// A raw HTTP/1.1 response with a correct content length.
pub fn http_response(status: u16, headers: &[(&str, &str)], body: &[u8]) -> Vec<u8> {
    let mut response = format!("HTTP/1.1 {status} Status\r\n");
    for (name, value) in headers {
        response.push_str(&format!("{name}: {value}\r\n"));
    }
    response.push_str(&format!(
        "content-length: {}\r\nconnection: close\r\n\r\n",
        body.len()
    ));
    let mut bytes = response.into_bytes();
    bytes.extend_from_slice(body);
    bytes
}

/// A 200 JSON response with request IDs in both providers' headers.
pub fn json_response(body: &str) -> Vec<u8> {
    http_response(
        200,
        &[
            ("content-type", "application/json; charset=utf-8"),
            ("request-id", "req_test_1"),
            ("x-request-id", "req_test_1"),
        ],
        body.as_bytes(),
    )
}
