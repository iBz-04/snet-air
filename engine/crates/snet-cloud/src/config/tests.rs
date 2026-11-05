use std::path::Path;

use rcgen::{BasicConstraints, CertificateParams, IsCa, KeyPair};

use super::*;
use crate::test_support::{
    ANTHROPIC_ENDPOINT, OPENAI_SETTINGS, load, no_certificate_files, openai_endpoint,
};
use snet_testkit::certificates::read_from;

fn parse(text: &str) -> Result<EndpointConfig, ConfigError> {
    EndpointConfig::parse(text, &no_certificate_files)
}

fn replace_line(text: &str, key: &str, new_line: &str) -> String {
    let prefix = format!("{key} = ");
    let mut found = false;
    let lines: Vec<String> = text
        .lines()
        .map(|line| {
            if line.starts_with(&prefix) && !found {
                found = true;
                new_line.to_string()
            } else {
                line.to_string()
            }
        })
        .collect();
    assert!(found, "no line for {key}");
    lines.join("\n") + "\n"
}

fn invalid_field(result: Result<EndpointConfig, ConfigError>) -> &'static str {
    match result {
        Err(ConfigError::Invalid { field, .. }) => field,
        other => panic!("expected an invalid field, got {other:?}"),
    }
}

fn ca_pem() -> String {
    let key = KeyPair::generate().unwrap();
    let mut params = CertificateParams::new(Vec::<String>::new()).unwrap();
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.self_signed(&key).unwrap().pem()
}

#[test]
fn a_full_configuration_of_each_format_loads() {
    let anthropic = load(ANTHROPIC_ENDPOINT);
    assert_eq!(anthropic.name(), "planner");
    assert_eq!(anthropic.wire(), WireSettings::AnthropicMessages);
    assert_eq!(anthropic.base_url().host(), "api.anthropic.com");
    assert_eq!(anthropic.base_url().port(), 443);
    assert!(!anthropic.switch_starts_on());
    assert_eq!(anthropic.task_limits().requests, 8);

    let openai = load(&openai_endpoint());
    assert_eq!(
        openai.wire(),
        WireSettings::OpenaiChat(OpenaiChatSettings {
            key_header: KeyHeader::AuthorizationBearer,
            instruction_role: InstructionRole::Developer,
            send_store_false: true,
        })
    );
}

#[test]
fn each_missing_field_fails_naming_it() {
    let fields = [
        "name",
        "base_url",
        "wire_format",
        "model",
        "allow_private_addresses",
        "max_output_tokens",
        "input_token_allowance",
        "request_bytes_cap",
        "reply_bytes_cap",
        "connect_timeout_seconds",
        "total_timeout_seconds",
        "switch_starts_on",
        "extra_ca_certificates",
        "replace_public_roots",
        "currency",
        "input_micro_per_million_tokens",
        "output_micro_per_million_tokens",
        "requests",
        "bytes_sent",
        "input_tokens",
        "output_tokens",
        "cost_micro",
    ];
    for field in fields {
        let without = replace_line(ANTHROPIC_ENDPOINT, field, "");
        match parse(&without) {
            Err(ConfigError::Parse { message }) => {
                assert!(message.contains(field), "{field}: {message}")
            }
            other => panic!("{field}: {other:?}"),
        }
    }
    for table in ["[prices]", "[task_limits]"] {
        let start = ANTHROPIC_ENDPOINT.find(table).unwrap();
        let end = ANTHROPIC_ENDPOINT[start + 1..]
            .find('[')
            .map_or(ANTHROPIC_ENDPOINT.len(), |offset| start + 1 + offset);
        let without = format!(
            "{}{}",
            &ANTHROPIC_ENDPOINT[..start],
            &ANTHROPIC_ENDPOINT[end..]
        );
        let name = table.trim_matches(['[', ']']);
        match parse(&without) {
            Err(ConfigError::Parse { message }) => assert!(message.contains(name), "{message}"),
            other => panic!("{table}: {other:?}"),
        }
    }
}

#[test]
fn an_unknown_field_is_refused() {
    let text = format!("telemetry = true\n{ANTHROPIC_ENDPOINT}");
    assert!(matches!(parse(&text), Err(ConfigError::Parse { .. })));
}

#[test]
fn openai_chat_needs_its_three_settings_and_anthropic_refuses_them() {
    for setting in ["key_header", "instruction_role", "send_store_false"] {
        let without = replace_line(&openai_endpoint(), setting, "");
        assert_eq!(invalid_field(parse(&without)), setting);
    }
    for setting_line in OPENAI_SETTINGS.lines() {
        let with_setting = format!("{setting_line}\n{ANTHROPIC_ENDPOINT}");
        let setting = setting_line.split(' ').next().unwrap();
        assert_eq!(invalid_field(parse(&with_setting)), setting);
    }
}

#[test]
fn base_url_rules_of_cld_3() {
    let refused = [
        "http://api.anthropic.com/v1",
        "https://user:pass@api.anthropic.com/v1",
        "https://api.anthropic.com/v1?beta=1",
        "https://api.anthropic.com/v1#part",
        "https://api.anthropic.com/v%31",
        "https://api.anthropic.com/v1/",
        "https://api.anthropic.com/",
        "https://",
        "https:///v1",
        "https://:443/v1",
        "https://api.anthropic.com:0/v1",
        "https://api.anthropic.com:65536/v1",
        "https://api.anthropic.com:x/v1",
        "https://api anthropic.com/v1",
        "https://[::1/v1",
        "ftp://api.anthropic.com/v1",
    ];
    for url in refused {
        let text = replace_line(
            ANTHROPIC_ENDPOINT,
            "base_url",
            &format!("base_url = \"{url}\""),
        );
        assert_eq!(invalid_field(parse(&text)), "base_url", "{url}");
    }

    let accepted = [
        ("https://api.anthropic.com", "api.anthropic.com", 443),
        (
            "https://api.anthropic.com:8443/v1",
            "api.anthropic.com",
            8443,
        ),
        ("https://127.0.0.1:9000/v1", "127.0.0.1", 9000),
        ("https://[::1]:9000", "::1", 9000),
    ];
    for (url, host, port) in accepted {
        let text = replace_line(
            ANTHROPIC_ENDPOINT,
            "base_url",
            &format!("base_url = \"{url}\""),
        );
        let config = load(&text);
        assert_eq!(config.base_url().host(), host);
        assert_eq!(config.base_url().port(), port);
        assert_eq!(config.base_url().as_str(), url);
    }
}

#[test]
fn each_number_is_refused_at_zero_and_over_its_ceiling() {
    let numbers: [(&str, &'static str, u64); 13] = [
        ("max_output_tokens", "max_output_tokens", 128_000),
        ("input_token_allowance", "input_token_allowance", 1_000_000),
        ("request_bytes_cap", "request_bytes_cap", 16 * 1024 * 1024),
        ("reply_bytes_cap", "reply_bytes_cap", 16 * 1024 * 1024),
        ("connect_timeout_seconds", "connect_timeout_seconds", 60),
        ("total_timeout_seconds", "total_timeout_seconds", 1_800),
        (
            "input_micro_per_million_tokens",
            "prices.input_micro_per_million_tokens",
            1_000_000_000_000,
        ),
        (
            "output_micro_per_million_tokens",
            "prices.output_micro_per_million_tokens",
            1_000_000_000_000,
        ),
        ("requests", "task_limits.requests", 10_000),
        ("bytes_sent", "task_limits.bytes_sent", 1024 * 1024 * 1024),
        ("input_tokens", "task_limits.input_tokens", 1_000_000_000),
        ("output_tokens", "task_limits.output_tokens", 1_000_000_000),
        (
            "cost_micro",
            "task_limits.cost_micro",
            1_000_000_000_000_000,
        ),
    ];
    for (key, field, ceiling) in numbers {
        let with = |value: u64| replace_line(ANTHROPIC_ENDPOINT, key, &format!("{key} = {value}"));
        assert_eq!(invalid_field(parse(&with(0))), field, "{key} at zero");
        assert!(parse(&with(ceiling)).is_ok(), "{key} at its ceiling");
        assert_eq!(
            invalid_field(parse(&with(ceiling + 1))),
            field,
            "{key} over"
        );
    }
}

#[test]
fn name_model_and_currency_are_checked() {
    for (key, line) in [
        ("name", "name = \"\""),
        ("name", "name = \"has space\""),
        ("model", "model = \"\""),
        ("model", "model = \"two words\""),
        ("currency", "currency = \"usd\""),
        ("currency", "currency = \"EURO\""),
    ] {
        let text = replace_line(ANTHROPIC_ENDPOINT, key, line);
        assert!(
            matches!(parse(&text), Err(ConfigError::Invalid { .. })),
            "{line}"
        );
    }
}

#[test]
fn certificates_add_to_or_replace_the_public_roots() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("extra-ca.pem"), ca_pem()).unwrap();
    let public_roots = webpki_roots::TLS_SERVER_ROOTS.len();

    let adding = replace_line(
        ANTHROPIC_ENDPOINT,
        "extra_ca_certificates",
        "extra_ca_certificates = [\"extra-ca.pem\"]",
    );
    let config = EndpointConfig::parse(&adding, &read_from(directory.path())).unwrap();
    assert_eq!(
        crate::tls::root_store(&config).unwrap().len(),
        public_roots + 1
    );

    let replacing = replace_line(
        &adding,
        "replace_public_roots",
        "replace_public_roots = true",
    );
    let config = EndpointConfig::parse(&replacing, &read_from(directory.path())).unwrap();
    assert_eq!(crate::tls::root_store(&config).unwrap().len(), 1);
}

#[test]
fn a_bad_certificate_or_an_empty_replacement_fails() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("empty.pem"), "").unwrap();
    std::fs::write(
        directory.path().join("broken.pem"),
        "-----BEGIN CERTIFICATE-----\nnot base64!\n-----END CERTIFICATE-----\n",
    )
    .unwrap();
    std::fs::write(
        directory.path().join("garbage.pem"),
        "-----BEGIN CERTIFICATE-----\nAAAA\n-----END CERTIFICATE-----\n",
    )
    .unwrap();
    std::fs::write(
        directory.path().join("huge.pem"),
        vec![b'A'; 1024 * 1024 + 1],
    )
    .unwrap();
    for file in [
        "empty.pem",
        "broken.pem",
        "garbage.pem",
        "missing.pem",
        "huge.pem",
    ] {
        let text = replace_line(
            ANTHROPIC_ENDPOINT,
            "extra_ca_certificates",
            &format!("extra_ca_certificates = [\"{file}\"]"),
        );
        let result = EndpointConfig::parse(&text, &read_from(directory.path()));
        if file == "huge.pem" {
            assert!(
                matches!(&result, Err(ConfigError::Invalid { reason, .. }) if reason.contains("larger than 1 MiB")),
                "{result:?}"
            );
        }
        assert_eq!(invalid_field(result), "extra_ca_certificates", "{file}");
    }

    let empty_replacement = replace_line(
        ANTHROPIC_ENDPOINT,
        "replace_public_roots",
        "replace_public_roots = true",
    );
    assert_eq!(
        invalid_field(parse(&empty_replacement)),
        "replace_public_roots"
    );
}

#[test]
fn the_digest_covers_the_text_and_the_certificates() {
    let directory = tempfile::tempdir().unwrap();
    let with_certificate = replace_line(
        ANTHROPIC_ENDPOINT,
        "extra_ca_certificates",
        "extra_ca_certificates = [\"extra-ca.pem\"]",
    );
    std::fs::write(directory.path().join("extra-ca.pem"), ca_pem()).unwrap();
    let first = EndpointConfig::parse(&with_certificate, &read_from(directory.path())).unwrap();
    std::fs::write(directory.path().join("extra-ca.pem"), ca_pem()).unwrap();
    let second = EndpointConfig::parse(&with_certificate, &read_from(directory.path())).unwrap();
    assert_ne!(first.digest(), second.digest());

    let base = load(ANTHROPIC_ENDPOINT);
    assert_eq!(base.digest(), load(ANTHROPIC_ENDPOINT).digest());
    let changed = load(&replace_line(
        ANTHROPIC_ENDPOINT,
        "requests",
        "requests = 9",
    ));
    assert_ne!(base.digest(), changed.digest());
}

#[test]
fn the_reader_is_called_once_per_name_in_order_with_the_name_as_written() {
    let text = replace_line(
        ANTHROPIC_ENDPOINT,
        "extra_ca_certificates",
        "extra_ca_certificates = [\"first.pem\", \"second.crt\"]",
    );
    let pem = ca_pem();
    let names = std::cell::RefCell::new(Vec::new());
    let reader = |name: &Path| {
        names.borrow_mut().push(name.to_path_buf());
        Ok(pem.clone().into_bytes())
    };
    let config = EndpointConfig::parse(&text, &reader).unwrap();
    assert_eq!(
        names.into_inner(),
        [Path::new("first.pem"), Path::new("second.crt")]
    );
    assert_eq!(config.extra_ca_certificates.len(), 2);
}

#[test]
fn a_reader_error_names_the_field_the_name_and_the_message() {
    let text = replace_line(
        ANTHROPIC_ENDPOINT,
        "extra_ca_certificates",
        "extra_ca_certificates = [\"extra-ca.pem\"]",
    );
    let failing = |_name: &Path| Err("refused by the caller's file checks".to_string());
    let result = EndpointConfig::parse(&text, &failing);
    assert!(
        matches!(&result, Err(ConfigError::Invalid { field, reason })
            if *field == "extra_ca_certificates"
                && reason.contains("extra-ca.pem")
                && reason.contains("refused by the caller's file checks")),
        "{result:?}"
    );
}

#[test]
fn only_plain_pem_or_crt_file_names_are_accepted_and_the_reader_is_not_called_otherwise() {
    for (name, accepted) in [
        ("ca.pem", true),
        ("ca.crt", true),
        ("", false),
        ("/etc/ssl/ca.pem", false),
        ("../ca.pem", false),
        ("certs/ca.pem", false),
        ("ca.pem/", false),
        ("ca.pem/.", false),
        (".", false),
        ("..", false),
        ("secrets.toml", false),
        ("people.txt", false),
        ("ca.PEM", false),
    ] {
        let text = replace_line(
            ANTHROPIC_ENDPOINT,
            "extra_ca_certificates",
            &format!("extra_ca_certificates = [\"extra-ca.pem\", \"{name}\"]"),
        );
        let pem = ca_pem();
        let calls = std::cell::Cell::new(0);
        let reader = |_name: &Path| {
            calls.set(calls.get() + 1);
            Ok(pem.clone().into_bytes())
        };
        let result = EndpointConfig::parse(&text, &reader);
        if accepted {
            assert!(result.is_ok(), "{name}: {result:?}");
        } else {
            assert!(
                matches!(&result, Err(ConfigError::Invalid { field, reason })
                    if *field == "extra_ca_certificates" && reason.contains("entry 2")),
                "{name}: {result:?}"
            );
            assert_eq!(calls.get(), 0, "{name}");
        }
    }
}

#[test]
fn the_digest_is_the_text_then_each_certificate_length_and_bytes() {
    use sha2::{Digest, Sha256};
    let text = replace_line(
        ANTHROPIC_ENDPOINT,
        "extra_ca_certificates",
        "extra_ca_certificates = [\"extra-ca.pem\"]",
    );
    let pem = ca_pem();
    let reader = |_name: &Path| Ok(pem.clone().into_bytes());
    let config = EndpointConfig::parse(&text, &reader).unwrap();
    let der = &config.extra_ca_certificates[0];
    let mut expected = Sha256::new();
    expected.update(text.as_bytes());
    expected.update((der.len() as u64).to_le_bytes());
    expected.update(der.as_ref());
    let expected: [u8; 32] = expected.finalize().into();
    assert_eq!(config.digest(), expected);
}

fn without_line(text: &str, key: &str) -> String {
    let prefix = format!("{key} = ");
    let lines: Vec<&str> = text
        .lines()
        .filter(|line| !line.starts_with(&prefix))
        .collect();
    lines.join("\n") + "\n"
}

#[test]
fn every_listed_effort_loads_for_its_format_and_no_other_does() {
    let anthropic = ["model_default", "low", "medium", "high", "xhigh", "max"];
    let openai = [
        "model_default",
        "none",
        "minimal",
        "low",
        "medium",
        "high",
        "xhigh",
        "max",
    ];
    for effort in anthropic {
        let text = replace_line(
            ANTHROPIC_ENDPOINT,
            "effort",
            &format!("effort = \"{effort}\""),
        );
        assert!(parse(&text).is_ok(), "anthropic {effort}");
    }
    for effort in openai {
        let text = replace_line(
            &openai_endpoint(),
            "effort",
            &format!("effort = \"{effort}\""),
        );
        assert!(parse(&text).is_ok(), "openai {effort}");
    }
    for effort in ["none", "minimal", "extreme", "LOW", ""] {
        let text = replace_line(
            ANTHROPIC_ENDPOINT,
            "effort",
            &format!("effort = \"{effort}\""),
        );
        assert_eq!(invalid_field(parse(&text)), "effort", "anthropic {effort}");
    }
    let unknown = replace_line(&openai_endpoint(), "effort", "effort = \"extreme\"");
    assert_eq!(invalid_field(parse(&unknown)), "effort");
    assert!(matches!(
        parse(&without_line(ANTHROPIC_ENDPOINT, "effort")),
        Err(ConfigError::Parse { .. })
    ));
}

#[test]
fn the_anthropic_workspace_is_key_scoped_or_a_plain_workspace_id() {
    for (value, expected) in [
        ("key_scoped", None),
        (
            "wrkspc_01JwQvzr7rXLA5AGx3HKfFUJ",
            Some("wrkspc_01JwQvzr7rXLA5AGx3HKfFUJ"),
        ),
    ] {
        let text = replace_line(
            ANTHROPIC_ENDPOINT,
            "anthropic_workspace",
            &format!("anthropic_workspace = \"{value}\""),
        );
        let config = parse(&text).unwrap();
        assert_eq!(
            config.anthropic_workspace_id.as_deref(),
            expected,
            "{value}"
        );
    }
    let too_long = format!("wrkspc_{}", "a".repeat(65));
    for value in [
        "wrkspc_",
        too_long.as_str(),
        "wrkspc_01-2",
        "workspace_1",
        "KEY_SCOPED",
        "",
    ] {
        let text = replace_line(
            ANTHROPIC_ENDPOINT,
            "anthropic_workspace",
            &format!("anthropic_workspace = \"{value}\""),
        );
        assert_eq!(
            invalid_field(parse(&text)),
            "anthropic_workspace",
            "{value}"
        );
    }
    assert_eq!(
        invalid_field(parse(&without_line(
            ANTHROPIC_ENDPOINT,
            "anthropic_workspace"
        ))),
        "anthropic_workspace"
    );
    let on_openai =
        openai_endpoint().replace("[prices]", "anthropic_workspace = \"key_scoped\"\n[prices]");
    assert_eq!(invalid_field(parse(&on_openai)), "anthropic_workspace");
}

#[test]
fn the_digest_changes_with_the_effort_and_the_workspace() {
    let base = load(ANTHROPIC_ENDPOINT).digest();
    let low = load(&replace_line(
        ANTHROPIC_ENDPOINT,
        "effort",
        "effort = \"low\"",
    ))
    .digest();
    let named = load(&replace_line(
        ANTHROPIC_ENDPOINT,
        "anthropic_workspace",
        "anthropic_workspace = \"wrkspc_01JwQvzr7rXLA5AGx3HKfFUJ\"",
    ))
    .digest();
    assert_ne!(base, low);
    assert_ne!(base, named);
    assert_ne!(low, named);
}
