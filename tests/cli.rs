use std::process::Command;

#[test]
fn help_and_version_do_not_require_credentials() {
    let binary = env!("CARGO_BIN_EXE_atarustia");
    let help = Command::new(binary).arg("--help").output().unwrap();
    assert!(help.status.success());
    let text = String::from_utf8(help.stdout).unwrap();
    for option in [
        "--data-root",
        "--no-web",
        "--account",
        "--no-auth",
        "--allow-shell",
    ] {
        assert!(text.contains(option));
    }
    let version = Command::new(binary).arg("--version").output().unwrap();
    assert!(version.status.success());
}
#[test]
fn renamed_binary_and_env_names_are_used() {
    let output = Command::new(env!("CARGO_BIN_EXE_atarustia"))
        .arg("--help")
        .output()
        .unwrap();
    let help = String::from_utf8(output.stdout).unwrap();
    assert!(help.contains("atarustia"));
    assert!(help.contains("ATARUSTIA_API_ID"));
    assert!(help.contains("ask, tdesktop, custom"));
}
#[test]
fn invalid_account_is_rejected_before_data_access() {
    let binary = env!("CARGO_BIN_EXE_atarustia");
    let dir = tempfile::tempdir().unwrap();
    let output = Command::new(binary)
        .args(["--no-auth", "--account", "../escape", "--data-root"])
        .arg(dir.path().join("unused"))
        .env_remove("ATARUSTIA_API_ID")
        .env_remove("ATARUSTIA_API_HASH")
        .env_remove("ATARUSTIA_BOT_TOKEN")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!dir.path().join("unused").exists());
}
#[test]
fn no_auth_never_prompts_when_credentials_missing() {
    let dir = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_atarustia"))
        .args(["--no-auth", "--no-web", "--data-root"])
        .arg(dir.path())
        .env_remove("ATARUSTIA_API_ID")
        .env_remove("ATARUSTIA_API_HASH")
        .env_remove("ATARUSTIA_BOT_TOKEN")
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("credentials missing"));
}
#[test]
fn import_mode_rejects_missing_python_data_without_prompting() {
    let dir = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_atarustia"))
        .args(["--no-auth", "--data-root"])
        .arg(dir.path().join("rust"))
        .arg("--import-python")
        .arg(dir.path().join("missing-python"))
        .env_remove("ATARUSTIA_API_ID")
        .env_remove("ATARUSTIA_API_HASH")
        .env_remove("ATARUSTIA_BOT_TOKEN")
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("Python data directory does not exist")
    );
    assert!(!dir.path().join("rust/config.json").exists());
}
#[test]
fn legacy_backup_cli_is_offline_and_preserves_input() {
    use base64::Engine as _;
    let dir = tempfile::tempdir().unwrap();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(include_str!("fixtures/python_backup.base64").trim())
        .unwrap();
    let source = dir.path().join("python.backup");
    std::fs::write(&source, &bytes).unwrap();
    let destination = dir.path().join("rust");
    let invoke = || {
        Command::new(env!("CARGO_BIN_EXE_atarustia"))
            .args(["--no-auth", "--data-root"])
            .arg(&destination)
            .arg("--import-backup")
            .arg(&source)
            .args(["--import-owner", "123"])
            .env_remove("ATARUSTIA_API_ID")
            .env_remove("ATARUSTIA_API_HASH")
            .env_remove("ATARUSTIA_BOT_TOKEN")
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap()
    };
    let first = invoke();
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    assert_eq!(std::fs::read(&source).unwrap(), bytes);
    assert!(!destination.join("config.json").exists());
    assert!(!destination.join("config-123.json").exists());
    assert!(!destination.join("atarustia.key").exists());
    let hash = atarustia::modules::digest(&bytes);
    let staged = destination.join("legacy_imports/123").join(hash);
    assert!(staged.join("report.json").is_file());
    assert!(staged.join("WeatherMod_123.py.txt").is_file());
    assert!(!staged.join("WeatherMod_123.py").exists());
    let converted = atarustia::backup::parse(
        &std::fs::read(staged.join("converted.backup.json")).unwrap(),
        123,
    )
    .unwrap();
    assert_eq!(converted.database["atarustia.main"]["command_prefix"], "!!");
    assert!(converted.database.get("atarustia.security").is_none());
    assert!(!invoke().status.success());
    assert_eq!(std::fs::read(source).unwrap(), bytes);
}
#[test]
fn legacy_backup_cli_requires_explicit_owner() {
    let output = Command::new(env!("CARGO_BIN_EXE_atarustia"))
        .args(["--import-backup", "anything.backup"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--import-owner"));
}

#[test]
fn sqlite_directory_import_is_offline_and_needs_no_python_master_key() {
    use base64::Engine as _;
    use std::io::{Cursor, Read};
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("python");
    std::fs::create_dir(&source).unwrap();
    let archive = base64::engine::general_purpose::STANDARD
        .decode(include_str!("fixtures/python_sqlite.zip.base64").trim())
        .unwrap();
    let mut zip = zip::ZipArchive::new(Cursor::new(archive)).unwrap();
    let mut bytes = vec![];
    zip.by_name("synthetic.session")
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    std::fs::write(source.join("atarustia-123.session"), &bytes).unwrap();
    std::fs::write(
        source.join("config.json"),
        br#"{"api_id":123,"api_hash":"00000000000000000000000000000000"}"#,
    )
    .unwrap();
    let destination = dir.path().join("rust");
    let output = Command::new(env!("CARGO_BIN_EXE_atarustia"))
        .args(["--no-auth", "--data-root"])
        .arg(&destination)
        .arg("--import-python")
        .arg(&source)
        .env_remove("ATARUSTIA_API_ID")
        .env_remove("ATARUSTIA_API_HASH")
        .env_remove("ATARUSTIA_BOT_TOKEN")
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        destination
            .join("atarustia-user-python-123.atsession")
            .is_file()
    );
    assert_eq!(
        std::fs::read(source.join("atarustia-123.session")).unwrap(),
        bytes
    );
    assert!(!source.join("atarustia.key").exists());
    assert!(!source.join("atarustia.lock").exists());
    assert!(!String::from_utf8_lossy(&output.stdout).contains(&hex::encode([7; 256])));
}
#[test]
fn same_import_directory_does_not_create_source_lock_or_key() {
    let source = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_atarustia"))
        .arg("--data-root")
        .arg(source.path())
        .arg("--import-python")
        .arg(source.path())
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!source.path().join("atarustia.lock").exists());
    assert!(!source.path().join("atarustia.key").exists());
}
