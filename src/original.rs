//! Source strings are embedded unchanged, not rewritten for the Rust implementation.
use anyhow::Result;
use serde_json::Value;
use std::sync::OnceLock;
pub const REVISION: &str = "a58e7019690abfec05e06b7383538fad39dc2d01";
pub const NAME: &str = "ataraxia";
pub const CODENAME: &str = "Rise";
pub const BOT_NAME: &str = "⛈️ ataraxia";
pub const INLINE_PLACEHOLDER: &str = "user@ataraxia:~$";
pub const FORUM_TITLE: &str = "ataraxia-userbot";
pub const FORUM_DESCRIPTION: &str = "⛈️ Content related to ataraxia will be here";
pub fn canonical_module(name: &str) -> &str {
    match name {
        "atarustiaSecurity" => "ataraxiaSecurity",
        "atarustiaBackup" => "ataraxiaBackup",
        "atarustiaConfig" => "ataraxiaConfig",
        "atarustiaWeb" => "ataraxiaWeb",
        "APIRatelimiter" => "APILimiter",
        _ => name,
    }
}
pub fn module_key(name: &str) -> &str {
    match canonical_module(name) {
        "ataraxiaInfo" => "ataraxia_info",
        "Updater" => "updater",
        "Loader" => "loader",
        "ataraxiaSecurity" => "ataraxia_security",
        "ataraxiaBackup" => "ataraxia_backup",
        "ataraxiaConfig" => "ataraxia_config",
        "ataraxiaWeb" => "ataraxia_web",
        "InlineStuff" => "inline_stuff",
        "Quickstart" => "quickstart",
        "Executor" => "executor",
        "Translations" => "translations",
        "Tester" => "test",
        "APILimiter" => "api_protection",
        _ => "settings",
    }
}
pub fn module_description(english: bool, name: &str) -> String {
    let translated = text(english, module_key(name), "_cls_doc");
    if translated != "_cls_doc" {
        return translated;
    }
    match canonical_module(name) {
        "ataraxiaWeb" => "Web/Inline mode add account",
        "ataraxiaBackup" => "Handles database and modules backups",
        "ataraxiaInfo" => "Userbot info and help",
        "ataraxiaConfig" => "Interactive configurator for ataraxia Userbot",
        "ataraxiaSecurity" => "Control security settings",
        "InlineStuff" => "Provides support for inline stuff",
        "Quickstart" => "Notifies user about userbot installation",
        "APILimiter" => "Helps userbot avoid spamming Telegram API",
        "Executor" => "Execute code and shell commands",
        "Loader" => "Loads modules",
        "Settings" => "Userbot settings: prefix, aliases, watchers, NoNick",
        "Tester" => "Perform operations based on userbot self-testing",
        "Translations" => "Processes internal translations",
        "Updater" => "Updates, restarts, and tracks latest ataraxia releases",
        _ => "",
    }
    .into()
}
/// Substitute only the source, never expand placeholders introduced by user data.
pub fn format(source: &str, positional: &[String], named: &[(&str, String)]) -> String {
    let mut result = String::new();
    let mut rest = source;
    let mut index = 0;
    while let Some(start) = rest.find('{') {
        result.push_str(&rest[..start]);
        rest = &rest[start..];
        let Some(end) = rest.find('}') else { break };
        let key = &rest[1..end];
        let value = if key.is_empty() {
            let value = positional.get(index);
            index += 1;
            value
        } else {
            named.iter().find(|(name, _)| *name == key).map(|(_, v)| v)
        };
        result.push_str(value.map(String::as_str).unwrap_or(&rest[..=end]));
        rest = &rest[end + 1..];
    }
    result.push_str(rest);
    result
}
pub fn message(
    english: bool,
    module: &str,
    key: &str,
    positional: &[String],
    named: &[(&str, String)],
) -> String {
    format(&text(english, module, key), positional, named)
}
/// The old <emoji document_id> spelling is not understood by grammers' HTML parser.
pub fn emoji_html(source: &str, premium: bool) -> String {
    let re=regex::Regex::new(r#"<(?:tg-emoji|emoji) (?:emoji-id|document_id)=["']?(\d+)["']?>(.*?)</(?:tg-emoji|emoji)>"#).unwrap();
    re.replace_all(source, |c: &regex::Captures<'_>| {
        if premium {
            format!("<tg-emoji emoji-id=\"{}\">{}</tg-emoji>", &c[1], &c[2])
        } else {
            c[2].into()
        }
    })
    .into_owned()
}
pub fn bot_username() -> String {
    use rand::{Rng, distributions::Alphanumeric};
    const WORDS: &[&str] = &[
        "Amor", "Arbor", "Astra", "Aurum", "Bellum", "Caelum", "Calor", "Candor", "Carpe", "Celer",
        "Certo", "Cibus", "Vita", "Vox", "Vultus", "Zephyrus", "Bimbalas", "Nywuctuu", "Anyone",
        "Draher", "Hackimo", "Silvyr",
    ];
    let mut rng = rand::thread_rng();
    let word = WORDS[rng.gen_range(0..WORDS.len())];
    let suffix: String = (&mut rng)
        .sample_iter(Alphanumeric)
        .take(6)
        .map(char::from)
        .collect();
    format!("{word}_{suffix}_bot")
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
    let value = pack
        .get(module)
        .and_then(|m| m.get(key))
        .and_then(Value::as_str)
        .map(str::to_owned);
    value.unwrap_or_else(|| {
        if english {
            key.into()
        } else {
            text(true, module, key)
        }
    })
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
        ("version", CODENAME.into()),
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
    let input = crate::update_ui::user_body(
        &crate::forms::Screen {
            text,
            ..Default::default()
        },
        premium,
    )?
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
