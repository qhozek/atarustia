use sha2::{Digest, Sha256};
#[test]
fn legacy_opt_in_frontend_keeps_its_original_attribution_and_assets() {
    for (bytes, hash) in [
        (
            include_bytes!("../web-resources/index.html").as_slice(),
            "e76d1920c5a3062aa1572289140ae500e36b0d3ab47ff7a6d63d32f4dd2215b4",
        ),
        (
            include_bytes!("../web-resources/static/app.css").as_slice(),
            "44f299780ad9da3d2db152ce968b448e2618516bca9b4970299b3363326f5fd0",
        ),
        (
            include_bytes!("../web-resources/static/app.js").as_slice(),
            "41c40ca9cfbf4ea7c00a6f7ec36e25efd5035e9897dde4e17d413cd935b5bb94",
        ),
        (
            include_bytes!("../web-resources/static/metal.js").as_slice(),
            "0902a9586326f537c33e9b436e03c97d47a8d936c119eaa103024d15499070ea",
        ),
        (
            include_bytes!("../web-resources/static/wordmark.png").as_slice(),
            "2778cd12ed97fa066fdde7dfb4d67f9fc37438b939318ba0d35d773d1b48d3fa",
        ),
    ] {
        assert_eq!(hex::encode(Sha256::digest(bytes)), hash);
    }
}
#[test]
fn command_catalogue_contains_exactly_the_original_names() {
    let manifest: serde_json::Value =
        serde_json::from_str(include_str!("../contracts/upstream-manifest.json")).unwrap();
    let expected: std::collections::BTreeSet<_> = manifest["commands"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    let actual: std::collections::BTreeSet<_> = atarustia::commands::BUILTINS
        .iter()
        .chain(atarustia::preferences::COMMANDS)
        .chain(atarustia::security_commands::COMMANDS)
        .map(|(name, _)| *name)
        .collect();
    assert_eq!(actual, expected);
    assert_eq!(actual.len(), 66);
    for name in [
        "modstatus",
        "modpause",
        "modresume",
        "atarustia",
        "ch_atarustia_bot",
        "approve",
        "review",
        "loops",
        "setup",
        "modules",
        "tglogs",
        "importbackup",
        "weburl",
        "suspend",
        "api_fw_protection",
        "suspend_api_protect",
    ] {
        assert!(!atarustia::commands::is_builtin(name));
    }
}
#[test]
fn current_main_locales_and_terminal_artwork_match_the_pinned_source() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/current_main.json")).unwrap();
    assert_eq!(fixture["revision"], atarustia::original::REVISION);
    for (name, bytes) in [
        (
            "en.yml",
            include_bytes!("../locales/original/en.yml").as_slice(),
        ),
        (
            "ru.yml",
            include_bytes!("../locales/original/ru.yml").as_slice(),
        ),
        (
            "banner.txt",
            include_bytes!("../assets/banner.txt").as_slice(),
        ),
        (
            "success.txt",
            include_bytes!("../assets/success.txt").as_slice(),
        ),
        ("2fa.txt", include_bytes!("../assets/2fa.txt").as_slice()),
    ] {
        assert_eq!(
            hex::encode(Sha256::digest(bytes)),
            fixture["sha256"][name],
            "{name}"
        );
    }
    for english in [false, true] {
        for (module, key) in [
            ("ataraxia_accounts", "login_timeout"),
            ("ataraxia_backup", "confirm_restore"),
            ("$inline", "stale_button"),
            ("$main", "started_info"),
        ] {
            assert_ne!(atarustia::original::text(english, module, key), key);
        }
    }
}
#[test]
fn source_strings_are_not_rebranded() {
    let ru = atarustia::original::text(false, "$web", "confirm");
    assert!(ru.contains("Вход в веб-интерфейс ataraxia"));
    assert_eq!(
        atarustia::original::text(false, "$web", "confirm_btn"),
        "🔓 Подтвердить вход"
    );
    assert_eq!(atarustia::info::MODULE, "ataraxiaInfo");
    assert!(atarustia::info::BANNER.starts_with("https://git.break.wtf/xenside/ataraxia/"));
}
