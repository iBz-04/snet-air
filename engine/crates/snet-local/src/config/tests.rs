use std::net::{IpAddr, Ipv6Addr, SocketAddr};

use super::*;

fn address(host: &str) -> Result<SocketAddr, ConfigError> {
    LocalConfig::ollama("qwen3:4b", Some(host)).map(|config| config.address())
}

#[test]
fn the_default_is_ollamas_own_address() {
    let config = LocalConfig::ollama("qwen3:4b", None).unwrap();
    assert_eq!(config.address(), DEFAULT_OLLAMA_ADDRESS);
    assert_eq!(config.address().to_string(), "127.0.0.1:11434");
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
