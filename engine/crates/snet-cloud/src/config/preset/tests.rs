use super::*;
use crate::config::WireSettings;

fn limits() -> TaskLimits {
    TaskLimits {
        requests: 12,
        bytes_sent: 2 * 1024 * 1024,
        input_tokens: 400_000,
        output_tokens: 64_000,
        cost_micro: 0,
    }
}

fn choice(provider: Provider, base_url: Option<&str>) -> PresetChoice {
    PresetChoice {
        provider,
        model: "some-model".to_string(),
        base_url: base_url.map(str::to_string),
        effort: "low".to_string(),
        task_limits: limits(),
    }
}

#[test]
fn each_provider_has_its_wire_format_url_and_key_variable() {
    let anthropic = EndpointConfig::preset(&choice(Provider::Anthropic, None)).unwrap();
    assert_eq!(anthropic.wire(), WireSettings::AnthropicMessages);
    assert_eq!(anthropic.base_url().host(), "api.anthropic.com");
    assert!(!anthropic.allow_private_addresses);
    assert!(anthropic.switch_starts_on());

    let openai = EndpointConfig::preset(&choice(Provider::Openai, None)).unwrap();
    assert_eq!(
        openai.wire(),
        WireSettings::OpenaiChat(OpenaiChatSettings {
            key_header: KeyHeader::AuthorizationBearer,
            instruction_role: InstructionRole::Developer,
            send_store_false: true,
            output_cap_field: crate::config::OutputCapField::MaxCompletionTokens,
        })
    );
    assert_eq!(openai.base_url().host(), "api.openai.com");

    let melious = EndpointConfig::preset(&choice(Provider::Melious, None)).unwrap();
    assert_eq!(
        melious.wire(),
        WireSettings::OpenaiChat(OpenaiChatSettings {
            key_header: KeyHeader::AuthorizationBearer,
            instruction_role: InstructionRole::System,
            send_store_false: false,
            output_cap_field: crate::config::OutputCapField::MaxTokens,
        })
    );
    assert_eq!(melious.base_url().host(), "api.melious.ai");
    assert!(!melious.allow_private_addresses);
    assert_eq!(Provider::parse("melious"), Some(Provider::Melious));

    let compatible = EndpointConfig::preset(&choice(
        Provider::OpenaiCompatible,
        Some("https://10.0.0.5:8443/v1"),
    ))
    .unwrap();
    assert!(compatible.allow_private_addresses);
    assert_eq!(compatible.base_url().host(), "10.0.0.5");

    assert_eq!(Provider::Anthropic.key_variable(), "ANTHROPIC_API_KEY");
    assert_eq!(Provider::Openai.key_variable(), "OPENAI_API_KEY");
    assert_eq!(Provider::Melious.key_variable(), "MELIOUS_API_KEY");
    assert_eq!(
        Provider::OpenaiCompatible.key_variable(),
        "SNET_CLOUD_API_KEY"
    );
}

#[test]
fn urls_are_required_or_refused_by_provider() {
    let field = |result: Result<EndpointConfig, ConfigError>| match result.unwrap_err() {
        ConfigError::Invalid { field, .. } => field,
        other => panic!("{other}"),
    };
    assert_eq!(
        field(EndpointConfig::preset(&choice(
            Provider::OpenaiCompatible,
            None
        ))),
        "base_url"
    );
    assert_eq!(
        field(EndpointConfig::preset(&choice(
            Provider::Anthropic,
            Some("https://example.com/v1")
        ))),
        "base_url"
    );
    assert_eq!(
        field(EndpointConfig::preset(&choice(
            Provider::Melious,
            Some("https://example.com/v1")
        ))),
        "base_url"
    );
    assert_eq!(
        field(EndpointConfig::preset(&choice(
            Provider::OpenaiCompatible,
            Some("http://example.com/v1")
        ))),
        "base_url"
    );
}

#[test]
fn effort_models_and_limits_are_checked() {
    let mut minimal = choice(Provider::Anthropic, None);
    minimal.effort = "minimal".to_string();
    assert!(EndpointConfig::preset(&minimal).is_err());
    let mut blank = choice(Provider::Openai, None);
    blank.model = "two words".to_string();
    assert!(EndpointConfig::preset(&blank).is_err());
    let mut no_requests = choice(Provider::Openai, None);
    no_requests.task_limits.requests = 0;
    assert!(EndpointConfig::preset(&no_requests).is_err());
}

#[test]
fn cost_is_not_limited_and_prices_are_zero() {
    let config = EndpointConfig::preset(&choice(Provider::Anthropic, None)).unwrap();
    assert_eq!(config.task_limits().cost_micro, MAX_TASK_COST_MICRO);
    assert_eq!(config.prices().input_micro_per_million_tokens, 0);
    assert_eq!(config.prices().output_micro_per_million_tokens, 0);
}

#[test]
fn provider_names_round_trip() {
    for provider in [
        Provider::Anthropic,
        Provider::Openai,
        Provider::OpenaiCompatible,
    ] {
        assert_eq!(Provider::parse(provider.name()), Some(provider));
    }
    assert_eq!(Provider::parse("other"), None);
}
