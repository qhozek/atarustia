use atarustia::telegram::validate_proxy;
use grammers_mtsender::mtproxy::Mode;

#[test]
fn proxy_selection_keeps_socks5_and_supports_mtproxy_links() {
    assert!(
        validate_proxy("socks5://user:pass@127.0.0.1:1080")
            .unwrap()
            .is_none()
    );
    for (prefix, mode) in [("", Mode::Intermediate), ("dd", Mode::Padded)] {
        let secret = format!("{prefix}{}", "07".repeat(16));
        let config = validate_proxy(&format!("mtproxy://localhost:443?secret={secret}"))
            .unwrap()
            .unwrap();
        assert_eq!(config.mode, mode);
        assert!(!format!("{config:?}").contains(&secret));
    }
    let secret = format!("ee{}6578616d706c652e636f6d", "07".repeat(16));
    let config = validate_proxy(&format!(
        "https://t.me/proxy?server=localhost&port=443&secret={secret}"
    ))
    .unwrap()
    .unwrap();
    assert_eq!(config.mode, Mode::FakeTls);
    assert_eq!(config.domain.as_deref(), Some("example.com"));
    for bad in [
        "socks5://localhost",
        "socks5://localhost:0",
        "http://localhost:1080",
        "mtproxy://localhost:443?secret=00",
    ] {
        assert!(validate_proxy(bad).is_err());
    }
}

#[test]
fn invalid_proxy_cli_fails_before_data_access_without_echoing_secret() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("unused");
    let secret = "invalid-private-secret";
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_atarustia"))
        .arg("--data-root")
        .arg(&root)
        .arg("--proxy")
        .arg(format!("mtproxy://localhost:443?secret={secret}"))
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!root.exists());
    assert!(!String::from_utf8_lossy(&output.stderr).contains(secret));
    assert!(!String::from_utf8_lossy(&output.stdout).contains(secret));
}
