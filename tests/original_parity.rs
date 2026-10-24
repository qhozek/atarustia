use sha2::{Digest, Sha256};
#[test]
fn frontend_is_the_unmodified_original_snapshot() {
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
    let expected = "info help helphide ataraxia dlm lm ulm ml clearmodules config fconfig settings setprefix setlang aliases addalias delalias watchers watcher watcherbl nonickcmd nonickuser nonickchat nonickcmds nonickusers nonickchats e terminal terminate ec ecpp enode security inlinesec owneradd ownerrm ownerlist newsgroup sgroups sgroup delsgroup sgroupadd sgroupdel tsec tsecrm tsecclr querysec backupall restoreall backupdb restoredb backupmods restoremods set_backup_period update restart source ping logs clearlogs suspend weburl addacc ch_ataraxia_bot ch_bot_token suspend_api_protect api_fw_protection dllangpack cleardb clearmodule";
    let expected: std::collections::BTreeSet<_> = expected.split_whitespace().collect();
    let actual: std::collections::BTreeSet<_> = atarustia::commands::BUILTINS
        .iter()
        .chain(atarustia::preferences::COMMANDS)
        .chain(atarustia::security_commands::COMMANDS)
        .map(|(name, _)| *name)
        .collect();
    assert_eq!(actual, expected);
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
    ] {
        assert!(!atarustia::commands::is_builtin(name));
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
