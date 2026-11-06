use std::net::{IpAddr, Ipv6Addr, SocketAddr};

use super::*;

fn address(host: &str) -> Result<SocketAddr, ConfigError> {
    LocalConfig::ollama("qwen3:4b", Some(host)).map(|config| match config.backend() {
        Backend::Ollama { address } => *address,
        #[cfg(feature = "gguf")]
        Backend::Gguf { .. } => panic!("an Ollama configuration"),
    })
}

#[test]
fn the_default_is_ollamas_own_address() {
    let config = LocalConfig::ollama("qwen3:4b", None).unwrap();
    assert_eq!(
        config.backend(),
        &Backend::Ollama {
            address: DEFAULT_OLLAMA_ADDRESS
        }
    );
    assert_eq!(DEFAULT_OLLAMA_ADDRESS.to_string(), "127.0.0.1:11434");
    assert_eq!(config.model(), "qwen3:4b");
}

#[test]
fn every_way_of_naming_this_machine_is_accepted() {
    for (host, expected) in [
        ("127.0.0.1", "127.0.0.1:11434"),
        ("127.0.0.1:9000", "127.0.0.1:9000"),
        ("http://localhost:11500", "127.0.0.1:11500"),
        ("http://127.0.0.1:11434/", "127.0.0.1:11434"),
        ("0.0.0.0", "127.0.0.1:11434"),
        ("0.0.0.0:8080", "127.0.0.1:8080"),
        ("localhost", "127.0.0.1:11434"),
    ] {
        assert_eq!(address(host).unwrap().to_string(), expected, "{host}");
    }
    assert_eq!(
        address("[::1]:9000").unwrap(),
        SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), 9000)
    );
    assert_eq!(
        address("::1").unwrap(),
        SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), 11434)
    );
}

#[test]
fn hosts_that_are_not_this_machine_are_refused() {
    for host in [
        "192.168.1.20",
        "10.0.0.5:11434",
        "http://example.com:11434",
        "ollama.internal",
        "https://127.0.0.1:11434",
        "127.0.0.1:11434/api",
        "user@127.0.0.1",
        "127.0.0.1:port",
        "",
    ] {
        assert!(
            matches!(
                address(host),
                Err(ConfigError::Invalid {
                    field: "OLLAMA_HOST",
                    ..
                })
            ),
            "{host}"
        );
    }
}

#[test]
fn model_names_are_plain() {
    for model in ["qwen3", "qwen3:4b", "library/llama3.2:3b-instruct-q4_K_M"] {
        assert!(LocalConfig::ollama(model, None).is_ok(), "{model}");
    }
    for model in ["", "two words", "a;b", &"x".repeat(129)] {
        assert!(LocalConfig::ollama(model, None).is_err(), "{model}");
    }
}

#[test]
fn bounds_can_be_changed_within_their_ranges() {
    let config = LocalConfig::ollama("qwen3:4b", None).unwrap();
    let wider = LocalBounds {
        context_tokens: 32_768,
        ..config.bounds()
    };
    assert_eq!(config.clone().with_bounds(wider).unwrap().bounds(), wider);
    for bounds in [
        LocalBounds {
            context_tokens: 100,
            ..config.bounds()
        },
        LocalBounds {
            max_output_tokens: 0,
            ..config.bounds()
        },
        LocalBounds {
            total_timeout_seconds: 0,
            ..config.bounds()
        },
        LocalBounds {
            max_concurrent_requests: 17,
            ..config.bounds()
        },
    ] {
        assert!(config.clone().with_bounds(bounds).is_err(), "{bounds:?}");
    }
}

#[cfg(feature = "gguf")]
#[test]
fn a_gguf_path_must_be_an_existing_gguf_file_and_becomes_absolute() {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("Small-Model.GGUF");
    std::fs::write(&file, b"GGUF").unwrap();
    let config = LocalConfig::gguf(&file).unwrap();
    match config.backend() {
        Backend::Gguf { path } => assert!(path.is_absolute()),
        Backend::Ollama { .. } => panic!("a GGUF configuration"),
    }
    assert_eq!(config.model(), "Small-Model.GGUF");
    assert_eq!(config.max_concurrent_requests(), 1);

    let other = directory.path().join("model.bin");
    std::fs::write(&other, b"GGUF").unwrap();
    for refused in [
        other,
        directory.path().join("missing.gguf"),
        directory.path().to_path_buf(),
    ] {
        assert!(
            LocalConfig::gguf(&refused).is_err(),
            "{}",
            refused.display()
        );
    }
    let folder = directory.path().join("folder.gguf");
    std::fs::create_dir(&folder).unwrap();
    assert!(LocalConfig::gguf(&folder).is_err());
}
