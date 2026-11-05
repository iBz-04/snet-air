use std::fmt;

use http::HeaderValue;
use zeroize::Zeroizing;

use crate::config::KeyHeader;
use crate::errors::Failure;

const BEARER_PREFIX: &str = "Bearer ";

/// A cloud API key. Its memory is wiped when it is dropped and its `Debug`
/// prints none of it.
pub struct ApiKey {
    value: Zeroizing<String>,
}

impl ApiKey {
    /// Takes ownership of the key so no unwiped copy is left with the caller.
    pub fn new(value: String) -> Result<Self, Failure> {
        let value = Zeroizing::new(value);
        if value.is_empty() {
            return Err(Failure::KeyInvalid {
                reason: "it is empty",
            });
        }
        if !value.bytes().all(|byte| byte.is_ascii_graphic()) {
            return Err(Failure::KeyInvalid {
                reason: "it holds a character other than visible ASCII",
            });
        }
        Ok(Self { value })
    }

    /// The key header's name and value, the value marked sensitive so that
    /// hyper and `http` never print it.
    pub(crate) fn header(
        &self,
        key_header: KeyHeader,
    ) -> Result<(&'static str, HeaderValue), Failure> {
        let (name, text) = match key_header {
            KeyHeader::AuthorizationBearer => {
                let mut text = Zeroizing::new(String::with_capacity(
                    BEARER_PREFIX.len() + self.value.len(),
                ));
                text.push_str(BEARER_PREFIX);
                text.push_str(&self.value);
                ("authorization", text)
            }
            KeyHeader::ApiKey => ("api-key", self.value.clone()),
        };
        let mut value = HeaderValue::from_str(&text).map_err(|_| Failure::KeyInvalid {
            reason: "it cannot be sent as a header value",
        })?;
        value.set_sensitive(true);
        Ok((name, value))
    }
}

impl fmt::Debug for ApiKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ApiKey(redacted)")
    }
}
