use http::uri::PathAndQuery;
use rustls_pki_types::ServerName;

const HTTPS_SCHEME: &str = "https://";
const HTTPS_DEFAULT_PORT: u16 = 443;

/// An `https` base URL with nothing that could change where or how a request
/// goes: no user information, query, fragment, percent-encoding or trailing
/// `/`. Paths are joined to it as text, which is why those are refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaseUrl {
    text: String,
    authority: String,
    host: String,
    port: u16,
    path_prefix: String,
    server_name: ServerName<'static>,
}

impl BaseUrl {
    pub(crate) fn parse(text: &str) -> Result<Self, String> {
        let Some(rest) = text.strip_prefix(HTTPS_SCHEME) else {
            return Err("the scheme must be https".to_string());
        };
        if let Some(position) = text.find(|character: char| !character.is_ascii_graphic()) {
            return Err(format!(
                "it holds a space, control or non-ASCII character at byte {position}"
            ));
        }
        if rest.contains('@') {
            return Err("it holds user information".to_string());
        }
        if rest.contains('?') {
            return Err("it holds a query".to_string());
        }
        if rest.contains('#') {
            return Err("it holds a fragment".to_string());
        }
        if rest.contains('%') {
            return Err("it holds a percent-encoded byte".to_string());
        }
        if rest.ends_with('/') {
            return Err("it ends with `/`".to_string());
        }

        let (authority, path_prefix) = match rest.find('/') {
            Some(slash) => rest.split_at(slash),
            None => (rest, ""),
        };
        if authority.is_empty() {
            return Err("it has no host".to_string());
        }
        let (host, port) = split_host_and_port(authority)?;
        if host.is_empty() {
            return Err("it has no host".to_string());
        }
        let server_name = ServerName::try_from(host.to_string())
            .map_err(|_| format!("`{host}` is neither a DNS name nor an IP address"))?;
        if !path_prefix.is_empty() {
            path_prefix
                .parse::<PathAndQuery>()
                .map_err(|_| "its path is not a valid request path".to_string())?;
        }

        Ok(Self {
            text: text.to_string(),
            authority: authority.to_string(),
            host: host.to_string(),
            port,
            path_prefix: path_prefix.to_string(),
            server_name,
        })
    }

    pub fn as_str(&self) -> &str {
        &self.text
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// The `host` header value: the authority exactly as configured.
    pub(crate) fn authority(&self) -> &str {
        &self.authority
    }

    pub(crate) fn server_name(&self) -> &ServerName<'static> {
        &self.server_name
    }

    pub(crate) fn path_for(&self, endpoint_path: &str) -> String {
        format!("{}{endpoint_path}", self.path_prefix)
    }
}

fn split_host_and_port(authority: &str) -> Result<(&str, u16), String> {
    if let Some(bracketed) = authority.strip_prefix('[') {
        let Some((host, after_host)) = bracketed.split_once(']') else {
            return Err("its IPv6 address has no closing `]`".to_string());
        };
        let port = match after_host {
            "" => HTTPS_DEFAULT_PORT,
            _ => match after_host.strip_prefix(':') {
                Some(port_text) => parse_port(port_text)?,
                None => return Err("text follows its IPv6 address".to_string()),
            },
        };
        return Ok((host, port));
    }
    match authority.split_once(':') {
        None => Ok((authority, HTTPS_DEFAULT_PORT)),
        Some((host, port_text)) => Ok((host, parse_port(port_text)?)),
    }
}

fn parse_port(port_text: &str) -> Result<u16, String> {
    if port_text.is_empty() || !port_text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("its port is not a number".to_string());
    }
    match port_text.parse::<u16>() {
        Ok(0) | Err(_) => Err("its port is outside 1 to 65535".to_string()),
        Ok(port) => Ok(port),
    }
}
