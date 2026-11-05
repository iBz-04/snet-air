use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use rustls::RootCertStore;
use rustls_pki_types::CertificateDer;
use rustls_pki_types::pem::PemObject;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::ceilings::*;
use super::wire_settings::{self, FormatFields};
use super::{BaseUrl, CertificateReader, EndpointConfig, InstructionRole, KeyHeader, WireFormat};
use crate::budget::{Prices, TaskLimits};
use crate::errors::ConfigError;

const MAX_CERTIFICATE_FILE_BYTES: u64 = 1024 * 1024;
const MAX_NAME_LENGTH: usize = 64;
const MAX_MODEL_LENGTH: usize = 256;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EndpointFile {
    name: String,
    base_url: String,
    wire_format: WireFormat,
    model: String,
    effort: String,
    allow_private_addresses: bool,
    max_output_tokens: u64,
    input_token_allowance: u64,
    request_bytes_cap: u64,
    reply_bytes_cap: u64,
    connect_timeout_seconds: u64,
    total_timeout_seconds: u64,
    switch_starts_on: bool,
    extra_ca_certificates: Vec<PathBuf>,
    replace_public_roots: bool,
    prices: PricesFile,
    task_limits: TaskLimitsFile,
    key_header: Option<KeyHeader>,
    instruction_role: Option<InstructionRole>,
    send_store_false: Option<bool>,
    anthropic_workspace: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PricesFile {
    currency: String,
    input_micro_per_million_tokens: u64,
    output_micro_per_million_tokens: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TaskLimitsFile {
    requests: u64,
    bytes_sent: u64,
    input_tokens: u64,
    output_tokens: u64,
    cost_micro: u64,
}

/// Parses an endpoint file and takes the CA certificates it names from the
/// caller's reader, reading no file itself; its
/// digest covers the text and each certificate.
pub(super) fn parse_endpoint(
    text: &str,
    read_certificate: CertificateReader<'_>,
) -> Result<EndpointConfig, ConfigError> {
    let (mut config, named_certificates) = parse_fields(text)?;
    let mut digest = Sha256::new();
    digest.update(text.as_bytes());
    for named_path in &named_certificates {
        let certificates = read_certificates(read_certificate, named_path)?;
        for certificate in certificates {
            digest.update((certificate.len() as u64).to_le_bytes());
            digest.update(certificate.as_ref());
            config.extra_ca_certificates.push(certificate);
        }
    }
    config.digest = digest.finalize().into();
    Ok(config)
}

/// Every check of the endpoint file that opens no file. The
/// certificates and the digest are filled in by `parse_endpoint`.
pub(crate) fn parse_fields(text: &str) -> Result<(EndpointConfig, Vec<PathBuf>), ConfigError> {
    let file: EndpointFile = toml::from_str(text).map_err(|error| ConfigError::Parse {
        message: error.message().to_string(),
    })?;

    check_name(&file.name)?;
    let base_url = BaseUrl::parse(&file.base_url)
        .map_err(|reason| ConfigError::invalid("base_url", reason))?;
    check_model(&file.model)?;
    let (wire, anthropic_workspace_id) = wire_settings::wire_settings(&FormatFields {
        wire_format: file.wire_format,
        key_header: file.key_header,
        instruction_role: file.instruction_role,
        send_store_false: file.send_store_false,
        anthropic_workspace: file.anthropic_workspace.clone(),
    })?;
    let effort = wire_settings::effort(&file.effort, file.wire_format)?;

    let max_output_tokens = within_ceiling(
        "max_output_tokens",
        file.max_output_tokens,
        MAX_OUTPUT_TOKENS_PER_REQUEST,
    )?;
    let input_token_allowance = within_ceiling(
        "input_token_allowance",
        file.input_token_allowance,
        MAX_INPUT_TOKEN_ALLOWANCE,
    )?;
    let request_bytes_cap = within_ceiling(
        "request_bytes_cap",
        file.request_bytes_cap,
        MAX_BODY_BYTES_CAP,
    )?;
    let reply_bytes_cap =
        within_ceiling("reply_bytes_cap", file.reply_bytes_cap, MAX_BODY_BYTES_CAP)?;
    let connect_timeout_seconds = within_ceiling(
        "connect_timeout_seconds",
        file.connect_timeout_seconds,
        MAX_CONNECT_TIMEOUT_SECONDS,
    )?;
    let total_timeout_seconds = within_ceiling(
        "total_timeout_seconds",
        file.total_timeout_seconds,
        MAX_TOTAL_TIMEOUT_SECONDS,
    )?;

    let prices = prices(file.prices)?;
    let task_limits = task_limits(&file.task_limits)?;

    if file.replace_public_roots && file.extra_ca_certificates.is_empty() {
        return Err(ConfigError::invalid(
            "replace_public_roots",
            "it is set, but no extra CA certificate is named, so nothing would be trusted",
        ));
    }

    check_certificate_names(&file.extra_ca_certificates)?;
    let named_certificates = file.extra_ca_certificates.clone();
    let config = EndpointConfig {
        name: file.name,
        base_url,
        wire,
        model: file.model,
        effort,
        anthropic_workspace_id,
        allow_private_addresses: file.allow_private_addresses,
        max_output_tokens,
        input_token_allowance,
        request_bytes_cap,
        reply_bytes_cap,
        connect_timeout: Duration::from_secs(connect_timeout_seconds),
        total_timeout: Duration::from_secs(total_timeout_seconds),
        switch_starts_on: file.switch_starts_on,
        extra_ca_certificates: Vec::new(),
        replace_public_roots: file.replace_public_roots,
        prices,
        task_limits,
        digest: [0; 32],
    };
    Ok((config, named_certificates))
}

pub(super) fn within_ceiling(
    field: &'static str,
    value: u64,
    ceiling: u64,
) -> Result<u64, ConfigError> {
    if value == 0 {
        return Err(ConfigError::invalid(field, "it must not be zero"));
    }
    if value > ceiling {
        return Err(ConfigError::invalid(
            field,
            format!("{value} is above the ceiling of {ceiling}"),
        ));
    }
    Ok(value)
}

fn check_name(name: &str) -> Result<(), ConfigError> {
    let allowed = |byte: u8| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.');
    if name.is_empty() || name.len() > MAX_NAME_LENGTH || !name.bytes().all(allowed) {
        return Err(ConfigError::invalid(
            "name",
            "it must be 1 to 64 letters, digits, `_`, `-` or `.`",
        ));
    }
    Ok(())
}

fn check_model(model: &str) -> Result<(), ConfigError> {
    if model.is_empty()
        || model.len() > MAX_MODEL_LENGTH
        || !model.bytes().all(|byte| byte.is_ascii_graphic())
    {
        return Err(ConfigError::invalid(
            "model",
            "it must be 1 to 256 visible ASCII characters",
        ));
    }
    Ok(())
}

fn prices(file: PricesFile) -> Result<Prices, ConfigError> {
    let currency_is_code =
        file.currency.len() == 3 && file.currency.bytes().all(|byte| byte.is_ascii_uppercase());
    if !currency_is_code {
        return Err(ConfigError::invalid(
            "prices.currency",
            "it must be a three-letter upper-case currency code",
        ));
    }
    Ok(Prices {
        input_micro_per_million_tokens: within_ceiling(
            "prices.input_micro_per_million_tokens",
            file.input_micro_per_million_tokens,
            MAX_PRICE_MICRO_PER_MILLION_TOKENS,
        )?,
        output_micro_per_million_tokens: within_ceiling(
            "prices.output_micro_per_million_tokens",
            file.output_micro_per_million_tokens,
            MAX_PRICE_MICRO_PER_MILLION_TOKENS,
        )?,
        currency: file.currency,
    })
}

fn task_limits(file: &TaskLimitsFile) -> Result<TaskLimits, ConfigError> {
    Ok(TaskLimits {
        requests: within_ceiling("task_limits.requests", file.requests, MAX_TASK_REQUESTS)?,
        bytes_sent: within_ceiling(
            "task_limits.bytes_sent",
            file.bytes_sent,
            MAX_TASK_BYTES_SENT,
        )?,
        input_tokens: within_ceiling(
            "task_limits.input_tokens",
            file.input_tokens,
            MAX_TASK_TOKENS,
        )?,
        output_tokens: within_ceiling(
            "task_limits.output_tokens",
            file.output_tokens,
            MAX_TASK_TOKENS,
        )?,
        cost_micro: within_ceiling(
            "task_limits.cost_micro",
            file.cost_micro,
            MAX_TASK_COST_MICRO,
        )?,
    })
}

/// Each CA name is one plain file name ending in `.pem` or `.crt`, so it
/// cannot leave the directory its reader serves or name a file the engine keeps
/// for something else, whose lines a PEM error would echo.
fn check_certificate_names(names: &[PathBuf]) -> Result<(), ConfigError> {
    for (index, name) in names.iter().enumerate() {
        let position = index + 1;
        let mut components = name.components();
        let first = components.next();
        let plain = components.next().is_none()
            && matches!(first, Some(Component::Normal(component)) if component == name.as_os_str());
        if !plain {
            return Err(ConfigError::invalid(
                "extra_ca_certificates",
                format!("entry {position} is not a plain file name in the configuration directory"),
            ));
        }
        let text = name.to_string_lossy();
        if !(text.ends_with(".pem") || text.ends_with(".crt")) {
            return Err(ConfigError::invalid(
                "extra_ca_certificates",
                format!("entry {position} does not end in `.pem` or `.crt`"),
            ));
        }
    }
    Ok(())
}

/// Takes one named CA certificate file from the caller's reader, refusing
/// one over 1 MiB.
fn read_certificates(
    read_certificate: CertificateReader<'_>,
    named_path: &Path,
) -> Result<Vec<CertificateDer<'static>>, ConfigError> {
    let shown = named_path.display().to_string();
    let pem = read_certificate(named_path).map_err(|message| {
        ConfigError::invalid(
            "extra_ca_certificates",
            format!("{shown} could not be read: {message}"),
        )
    })?;
    if pem.len() as u64 > MAX_CERTIFICATE_FILE_BYTES {
        return Err(ConfigError::invalid(
            "extra_ca_certificates",
            format!("{shown} is larger than 1 MiB"),
        ));
    }
    parse_certificates(&shown, &pem)
}

/// The certificates of one PEM file, each usable as a trust anchor.
pub(crate) fn parse_certificates(
    shown: &str,
    pem: &[u8],
) -> Result<Vec<CertificateDer<'static>>, ConfigError> {
    let certificates = CertificateDer::pem_slice_iter(pem)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
            ConfigError::invalid(
                "extra_ca_certificates",
                format!("{shown} is not valid PEM: {error}"),
            )
        })?;
    if certificates.is_empty() {
        return Err(ConfigError::invalid(
            "extra_ca_certificates",
            format!("{shown} holds no certificate"),
        ));
    }
    for certificate in &certificates {
        RootCertStore::empty()
            .add(certificate.clone())
            .map_err(|error| {
                ConfigError::invalid(
                    "extra_ca_certificates",
                    format!("{shown} holds a certificate that cannot be a trust anchor: {error}"),
                )
            })?;
    }
    Ok(certificates)
}
