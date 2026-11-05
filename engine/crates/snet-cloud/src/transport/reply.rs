use http::header::{CONTENT_ENCODING, CONTENT_TYPE};
use http::{HeaderMap, Response};
use http_body_util::BodyExt;
use hyper::body::Incoming;

use super::{ExchangeProgress, classify_hyper_error};
use crate::errors::Failure;

const MAX_REQUEST_ID_LENGTH: usize = 200;

/// Reads the status, the provider's request ID and at most `reply_bytes_cap`
/// body bytes into `progress`, so an outcome record has them even when the
/// exchange later fails.
pub(super) async fn read_reply(
    response: Response<Incoming>,
    reply_bytes_cap: u64,
    request_id_header: &'static str,
    progress: &mut ExchangeProgress,
) -> Result<(), Failure> {
    let status = response.status();
    progress.status = Some(status.as_u16());
    progress.request_id = request_id(response.headers(), request_id_header);

    if status.is_redirection() {
        return Err(Failure::Redirect {
            status: status.as_u16(),
        });
    }
    if status.is_success() {
        check_success_content(response.headers())?;
    }

    let mut body = response.into_body();
    while let Some(frame) = body.frame().await {
        let frame = frame.map_err(classify_hyper_error)?;
        let Ok(data) = frame.into_data() else {
            continue;
        };
        let length_after = progress.reply_body.len() as u64 + data.len() as u64;
        if length_after > reply_bytes_cap {
            return Err(Failure::ReplyTooLarge {
                cap: reply_bytes_cap,
            });
        }
        progress.reply_body.extend_from_slice(&data);
    }
    Ok(())
}

fn check_success_content(headers: &HeaderMap) -> Result<(), Failure> {
    for encoding in headers.get_all(CONTENT_ENCODING) {
        let is_identity = encoding
            .to_str()
            .is_ok_and(|text| text.trim().eq_ignore_ascii_case("identity"));
        if !is_identity {
            return Err(Failure::UnexpectedEncoding);
        }
    }
    let mut content_types = headers.get_all(CONTENT_TYPE).iter();
    let (Some(content_type), None) = (content_types.next(), content_types.next()) else {
        return Err(Failure::UnexpectedContentType);
    };
    let is_json = content_type.to_str().is_ok_and(|text| {
        let media_type = text.split(';').next().unwrap_or_default();
        media_type.trim().eq_ignore_ascii_case("application/json")
    });
    if !is_json {
        return Err(Failure::UnexpectedContentType);
    }
    Ok(())
}

fn request_id(headers: &HeaderMap, header_name: &'static str) -> Option<String> {
    let text = headers.get(header_name)?.to_str().ok()?;
    let is_plain = !text.is_empty()
        && text.len() <= MAX_REQUEST_ID_LENGTH
        && text.bytes().all(|byte| byte.is_ascii_graphic());
    is_plain.then(|| text.to_string())
}
