//! Source strings are embedded unchanged, not rewritten for the Rust implementation.
use anyhow::Result;
use serde_json::Value;
use std::sync::OnceLock;
pub const REVISION: &str = "a58e7019690abfec05e06b7383538fad39dc2d01";
pub fn module_key(name: &str) -> &str {
    match name {
        "ataraxiaInfo" => "ataraxia_info",
        "Updater" => "updater",
        "Loader" => "loader",
        "atarustiaSecurity" => "ataraxia_security",
        "atarustiaBackup" => "ataraxia_backup",
        "atarustiaConfig" => "ataraxia_config",
        "atarustiaWeb" => "ataraxia_web",
        "Executor" => "executor",
        "Translations" => "translations",
        "Tester" => "test",
        "APIRatelimiter" => "api_protection",
        _ => "settings",
    }
}
pub fn text(english: bool, module: &str, key: &str) -> String {
    static EN: OnceLock<Value> = OnceLock::new();
    static RU: OnceLock<Value> = OnceLock::new();
    let pack = if english {
        EN.get_or_init(|| {
            serde_yaml::from_str(include_str!("../locales/original/en.yml"))
                .expect("original English strings")
        })
    } else {
        RU.get_or_init(|| {
            serde_yaml::from_str(include_str!("../locales/original/ru.yml"))
                .expect("original Russian strings")
        })
    };
    pack.get(module)
        .and_then(|m| m.get(key))
        .and_then(Value::as_str)
        .unwrap_or(key)
        .into()
}
pub async fn about(
    context: &crate::commands::Context,
    message: &grammers_client::message::Message,
) -> Result<Option<String>> {
    let english = context.db.language().starts_with("en");
    let mut text = text(english, "settings", "ataraxia");
    let premium = context
        .db
        .get("RustRuntime", "premium")
        .as_bool()
        .unwrap_or(false);
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let commit = crate::updater::head(root)
        .await
        .unwrap_or_else(|_| "unknown".into());
    let output = tokio::process::Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(root)
        .output();
    let branch = tokio::time::timeout(std::time::Duration::from_secs(3), output)
        .await
        .ok()
        .and_then(Result::ok)
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_else(|| "unknown".into());
    for (key, value) in [
        (
            "logo",
            if premium {
                crate::update_ui::PREMIUM_LOGO.into()
            } else {
                "⛈️ <b>ataraxia</b>".into()
            },
        ),
        ("version", "Rise".into()),
        ("tl_url", "https://git.break.wtf/xenside/ataraxia-tl".into()),
        // The transport implementation must be truthful even while the surrounding layout is copied.
        ("tl_version", "Rust / grammers 0.11.0".into()),
        ("layer", grammers_client::tl::LAYER.to_string()),
        ("branch", crate::output::escape(&branch)),
        ("commit", commit.chars().take(7).collect()),
        (
            "commit_url",
            format!("https://github.com/qhozek/atarustia/commit/{commit}"),
        ),
        ("repo_url", "https://github.com/qhozek/atarustia".into()),
    ] {
        text = text.replace(&format!("{{{key}}}"), &value);
    }
    let input = grammers_client::message::InputMessage::new()
        .html(text)
        .invert_media(true)
        .media(grammers_client::tl::types::InputMediaWebPage {
            url: crate::info::BANNER.into(),
            force_large_media: false,
            force_small_media: false,
            optional: true,
        });
    if message.outgoing() {
        message.edit(input).await?;
    } else {
        message.reply(input).await?;
    }
    Ok(None)
}
