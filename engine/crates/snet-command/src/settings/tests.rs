use std::path::PathBuf;

use snet_local::Backend;

use super::*;
use crate::exit::Exit;
use crate::test_support::FixedEnvironment;

fn environment(variables: &[(&str, &str)]) -> FixedEnvironment {
    FixedEnvironment::new(variables)
}

fn arguments() -> AskArguments {
    AskArguments {
        question: "Who?".to_string(),
        files: vec!["a.txt".to_string()],
        ..AskArguments::default()
    }
}

fn error(arguments: &AskArguments, environment: &FixedEnvironment) -> CommandError {
    match resolve(arguments, environment) {
        Ok(_) => panic!("resolved"),
        Err(error) => error,
    }
}

#[test]
fn one_key_is_enough_and_everything_else_has_a_default() {
    let settings = resolve(&arguments(), &environment(&[("ANTHROPIC_API_KEY", "sk-1")])).unwrap();
    assert_eq!(settings.cloud.provider, Provider::Anthropic);
    assert_eq!(settings.cloud.model, "claude-sonnet-5-5");
    assert_eq!(settings.cloud.effort, "low");
    assert_eq!(settings.cloud.base_url, None);
    assert_eq!(settings.cloud.task_limits, TASK_LIMITS);
    assert_eq!(settings.local.model(), "qwen3:4b");
    assert_eq!(
        settings.local.backend(),
        &Backend::Ollama {
            address: "127.0.0.1:11434".parse().unwrap()
        }
    );
    assert_eq!(settings.local.bounds().context_tokens, 8_192);
    assert_eq!(settings.home, PathBuf::from("/home/someone/.snet"));
    assert_eq!(settings.conversation.rounds, 4);
    assert_eq!(settings.conversation.seconds, 600);
    assert_eq!(settings.fan_out.plan_rounds, 2);
    assert_eq!(settings.fan_out.seconds, 3_600);
    assert_eq!(settings.fan_out.local_requests, 512);
    assert!(!settings.fanout);
}

#[test]
fn melious_needs_only_its_key_and_a_model() {
    let with_melious = AskArguments {
        cloud: Some("melious:glm-5.3".to_string()),
        ..arguments()
    };
    let settings = resolve(
        &with_melious,
        &environment(&[("MELIOUS_API_KEY", "sk-mel-1")]),
    )
    .unwrap();
    assert_eq!(settings.cloud.provider, Provider::Melious);
    assert_eq!(settings.cloud.model, "glm-5.3");
    assert_eq!(settings.cloud.base_url, None);
    assert_eq!(settings.cloud.effort, "low");
}

#[test]
fn a_missing_key_names_the_variable_to_set() {
    for (cloud, variable) in [
        ("anthropic:m", "ANTHROPIC_API_KEY"),
        ("openai:m", "OPENAI_API_KEY"),
        ("melious:m", "MELIOUS_API_KEY"),
        ("openai-compatible:m", "SNET_CLOUD_API_KEY"),
    ] {
        let with_cloud = AskArguments {
            cloud: Some(cloud.to_string()),
            cloud_url: cloud
                .starts_with("openai-compatible")
                .then(|| "https://host/v1".to_string()),
            ..arguments()
        };
        let empty = environment(&[("ANTHROPIC_API_KEY", ""), ("OPENAI_API_KEY", "")]);
        let refused = error(&with_cloud, &empty);
        assert_eq!(refused.exit, Exit::Configuration);
        assert!(refused.message.contains(variable), "{}", refused.message);
    }
}

#[test]
fn the_environment_chooses_models_hosts_and_home_when_flags_do_not() {
    let settings = resolve(
        &arguments(),
        &environment(&[
            ("SNET_CLOUD", "openai-compatible:llama-70b"),
            ("SNET_CLOUD_URL", "https://10.1.1.1/v1"),
            ("SNET_CLOUD_API_KEY", "key"),
            ("SNET_LOCAL", "ollama:llama3.2:3b"),
            ("OLLAMA_HOST", "0.0.0.0:9999"),
            ("SNET_HOME", "/data/snet"),
        ]),
    )
    .unwrap();
    assert_eq!(settings.cloud.provider, Provider::OpenaiCompatible);
    assert_eq!(settings.cloud.model, "llama-70b");
    assert_eq!(
        settings.cloud.base_url.as_deref(),
        Some("https://10.1.1.1/v1")
    );
    assert_eq!(settings.cloud.effort, "model_default");
    assert_eq!(settings.local.model(), "llama3.2:3b");
    assert_eq!(
        settings.local.backend(),
        &Backend::Ollama {
            address: "127.0.0.1:9999".parse().unwrap()
        }
    );
    assert_eq!(settings.home, PathBuf::from("/data/snet"));
}

#[test]
fn flags_win_over_the_environment() {
    let flagged = AskArguments {
        cloud: Some("openai:gpt-x".to_string()),
        local: Some("ollama:phi4".to_string()),
        effort: Some("high".to_string()),
        context: Some(32_768),
        rounds: Some(6),
        ..arguments()
    };
    let settings = resolve(
        &flagged,
        &environment(&[
            ("SNET_CLOUD", "anthropic:other"),
            ("SNET_LOCAL", "ollama:other"),
            ("OPENAI_API_KEY", "key"),
        ]),
    )
    .unwrap();
    assert_eq!(settings.cloud.model, "gpt-x");
    assert_eq!(settings.cloud.effort, "high");
    assert_eq!(settings.local.model(), "phi4");
    assert_eq!(settings.local.bounds().context_tokens, 32_768);
    assert_eq!(settings.conversation.rounds, 6);
}

#[test]
fn malformed_choices_are_refused() {
    let key = environment(&[("ANTHROPIC_API_KEY", "sk-1")]);
    let cases = [
        (
            AskArguments {
                cloud: Some("claude".to_string()),
                ..arguments()
            },
            "provider:model",
        ),
        (
            AskArguments {
                cloud: Some("azure:m".to_string()),
                ..arguments()
            },
            "is not anthropic, openai, melious or openai-compatible",
        ),
        (
            AskArguments {
                local: Some("llama3".to_string()),
                ..arguments()
            },
            "must be ollama:<model> or gguf:<file>",
        ),
        (
            AskArguments {
                rounds: Some(9),
                ..arguments()
            },
            "--rounds must be 2 to 8",
        ),
        (
            AskArguments {
                plan_rounds: Some(0),
                ..arguments()
            },
            "--plan-rounds must be 1 to 4",
        ),
        (
            AskArguments {
                context: Some(100),
                ..arguments()
            },
            "context_tokens",
        ),
    ];
    for (bad, expected) in cases {
        let refused = error(&bad, &key);
        assert!(refused.message.contains(expected), "{}", refused.message);
    }
    let remote = environment(&[("ANTHROPIC_API_KEY", "sk-1"), ("OLLAMA_HOST", "10.0.0.2")]);
    assert!(
        error(&arguments(), &remote)
            .message
            .contains("is not this machine")
    );
    let homeless = FixedEnvironment {
        home: None,
        ..FixedEnvironment::new(&[("ANTHROPIC_API_KEY", "sk-1")])
    };
    assert!(error(&arguments(), &homeless).message.contains("SNET_HOME"));
}

#[cfg(not(feature = "gguf"))]
#[test]
fn a_gguf_model_names_the_feature_to_build_with() {
    let gguf = AskArguments {
        local: Some("gguf:/models/small.gguf".to_string()),
        ..arguments()
    };
    let refused = error(&gguf, &environment(&[("ANTHROPIC_API_KEY", "sk-1")]));
    assert_eq!(refused.exit, Exit::Usage);
    assert!(
        refused.message.contains("--features gguf"),
        "{}",
        refused.message
    );
}

#[cfg(feature = "gguf")]
#[test]
fn a_gguf_model_is_a_file_checked_before_anything_runs() {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("small.gguf");
    std::fs::write(&file, b"GGUF").unwrap();
    let gguf = AskArguments {
        local: Some(format!("gguf:{}", file.display())),
        ..arguments()
    };
    let key = environment(&[("ANTHROPIC_API_KEY", "sk-1")]);
    let settings = resolve(&gguf, &key).unwrap();
    assert!(matches!(settings.local.backend(), Backend::Gguf { .. }));
    assert_eq!(settings.local.max_concurrent_requests(), 1);
    let missing = AskArguments {
        local: Some("gguf:/no/such/model.gguf".to_string()),
        ..arguments()
    };
    assert_eq!(error(&missing, &key).exit, Exit::Configuration);
}
