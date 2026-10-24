//! Account information card, with the original layout and native Rust measurements.
use crate::{database::Database, output::escape};
use anyhow::{Context, Result, ensure};
use grammers_client::{
    Client,
    message::{InputMessage, InputRichMessage, Message},
    tl,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path, time::Duration};

pub const MODULE: &str = "ataraxiaInfo";
pub const BANNER: &str =
    "https://git.break.wtf/xenside/ataraxia/media/branch/main/assets/ataraxia.png";
pub const PLACEHOLDERS: &[&str] = &[
    "me",
    "codename",
    "prefix",
    "upd",
    "uptime",
    "cpu_usage",
    "ram_usage",
    "branch",
    "commit",
    "kernel",
    "platform",
];
const ICONS: &[(i64, &str)] = &[
    (5879770735999717115, "👤"),
    (5877332341331857066, "📁"),
    (5778423822940114949, "🛡"),
    (5877410604225924969, "🔄"),
    (5877396173135811032, "⌨️"),
    (5960751816084820359, "⏲"),
    (5908972566537572230, "♾"),
    (5936130851635990622, "⚡️"),
    (6008104758236156926, "🔥"),
    (5870982283724328568, "⚙"),
];

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub custom_message: Option<String>,
    pub rich_mode: bool,
    pub banner_url: String,
    pub quote_media: bool,
    pub invert_media: bool,
    pub help_banner_url: String,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            custom_message: None,
            rich_mode: false,
            banner_url: BANNER.into(),
            quote_media: true,
            invert_media: true,
            help_banner_url: BANNER.into(),
        }
    }
}
impl Config {
    pub fn load(db: &Database) -> Result<Self> {
        let saved = db.get(MODULE, "__config__");
        let value: Self = if saved.is_null() {
            Self::default()
        } else {
            serde_json::from_value(saved)?
        };
        value.validate()?;
        Ok(value)
    }
    pub fn validate(&self) -> Result<()> {
        for url in [&self.banner_url, &self.help_banner_url] {
            if !url.is_empty() {
                let parsed = reqwest::Url::parse(url)?;
                ensure!(
                    url.len() <= 8192
                        && parsed.scheme() == "https"
                        && parsed.host_str().is_some()
                        && parsed.username().is_empty()
                        && parsed.password().is_none(),
                    "banner must be an HTTPS URL without credentials, or an empty string"
                );
            }
        }
        if let Some(template) = &self.custom_message {
            ensure!(template.len() <= 16 * 1024, "custom message exceeds 16 KiB");
            let values = PLACEHOLDERS
                .iter()
                .map(|name| (*name, String::new()))
                .collect();
            substitute(template, &values)?;
        }
        Ok(())
    }
}

/// Literal placeholders only: no indexing, attribute access, formatting code or recursive expansion.
pub fn substitute(template: &str, values: &BTreeMap<&str, String>) -> Result<String> {
    let mut output = String::new();
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
                output.push('{');
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
                output.push('}');
            }
            '{' => {
                let mut key = String::new();
                loop {
                    match chars.next() {
                        Some('}') => break,
                        Some(c) if c != '{' => key.push(c),
                        _ => anyhow::bail!("unclosed placeholder"),
                    }
                }
                output.push_str(
                    values
                        .get(key.as_str())
                        .with_context(|| format!("unknown info placeholder: {key}"))?,
                );
            }
            '}' => anyhow::bail!("unmatched closing brace"),
            _ => output.push(c),
        }
    }
    Ok(output)
}

pub struct Snapshot {
    pub owner: i64,
    pub name: String,
    pub prefix: String,
    pub version: String,
    pub uptime: u64,
    pub branch: String,
    pub commit: Option<String>,
    pub up_to_date: Option<bool>,
    pub cpu: Option<f32>,
    pub memory: Option<u64>,
    pub kernel: String,
    pub platform: String,
}
pub fn uptime(seconds: u64) -> String {
    let clock = format!(
        "{}:{:02}:{:02}",
        seconds % 86400 / 3600,
        seconds % 3600 / 60,
        seconds % 60
    );
    if seconds >= 86400 {
        format!("{} day(s), {clock}", seconds / 86400)
    } else {
        clock
    }
}
impl Snapshot {
    pub fn render(&self, config: &Config, english: bool, rich: bool) -> Result<String> {
        let prefix = escape(&self.prefix);
        let status = match self.up_to_date {
            Some(true) => {
                if english {
                    "<b>Up-to-date</b>".into()
                } else {
                    "<b>Актуальная версия</b>".into()
                }
            }
            Some(false) => format!(
                "<b>{}</b> <code>{prefix}update</code>",
                if english {
                    "Update required"
                } else {
                    "Требуется обновление"
                }
            ),
            None => {
                if english {
                    "<i>Update status unavailable</i>".into()
                } else {
                    "<i>Статус обновлений недоступен</i>".into()
                }
            }
        };
        let commit = self
            .commit
            .as_ref()
            .filter(|s| s.len() == 40 && s.bytes().all(|c| c.is_ascii_hexdigit()))
            .map(|sha| {
                format!(
                    "<a href=\"https://github.com/qhozek/atarustia/commit/{sha}\">#{}</a>",
                    &sha[..7]
                )
            })
            .unwrap_or_else(|| "—".into());
        let values = BTreeMap::from([
            (
                "me",
                format!(
                    "<b><a href=\"tg://user?id={}\">{}</a></b>",
                    self.owner,
                    escape(&self.name)
                ),
            ),
            ("codename", escape(&self.version)),
            ("prefix", format!("«<code>{prefix}</code>»")),
            ("upd", status),
            ("uptime", uptime(self.uptime)),
            ("branch", escape(&self.branch)),
            ("commit", commit),
            (
                "cpu_usage",
                self.cpu
                    .filter(|n| n.is_finite())
                    .map(|n| format!("{n:.2}"))
                    .unwrap_or_else(|| "—".into()),
            ),
            (
                "ram_usage",
                self.memory
                    .map(|n| format!("{:.1} MB", n as f64 / 1048576.0))
                    .unwrap_or_else(|| "—".into()),
            ),
            ("kernel", escape(&self.kernel)),
            ("platform", escape(&self.platform)),
        ]);
        if let Some(template) = config.custom_message.as_ref().filter(|s| !s.is_empty()) {
            return substitute(template, &values);
        }
        let icon = |i: usize| {
            let (id, alt) = ICONS[i];
            if rich {
                format!("<tg-emoji emoji-id=\"{id}\">{alt}</tg-emoji>")
            } else {
                alt.into()
            }
        };
        let br = if rich { "<br>" } else { "\n" };
        let text = format!(
            "<blockquote>{} <b>Owner:</b> {{me}}</blockquote>\n<blockquote>{} <b>Codename:</b> <i>atarustia · {{codename}}</i>{br}{} <b>Branch:</b> {{branch}} ({{commit}}){br}{} {{upd}}</blockquote>\n<blockquote>{} <b>Prefix: </b>{{prefix}}{br}{} <b>Uptime:</b> {{uptime}}</blockquote>\n<blockquote>{} <b>CPU usage:</b> {{cpu_usage}}{br}{} <b>RAM usage:</b> {{ram_usage}}{br}{} <b>Kernel:</b> {{kernel}}{br}{} <b>Host:</b> {{platform}}</blockquote>",
            icon(0),
            icon(1),
            icon(2),
            icon(3),
            icon(4),
            icon(5),
            icon(6),
            icon(7),
            icon(8),
            icon(9)
        );
        substitute(&text, &values)
    }
}

struct Host {
    cpu: Option<f32>,
    memory: Option<u64>,
    kernel: String,
    platform: String,
}
fn host() -> Host {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
    let mut system = System::new();
    system.refresh_cpu_usage();
    std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
    system.refresh_cpu_usage();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing()
            .with_memory()
            .with_exe(UpdateKind::Never),
    );
    let memory = sysinfo::get_current_pid().ok().and_then(|pid| {
        let mut ids = std::collections::HashSet::from([pid]);
        loop {
            let previous = ids.len();
            for (id, process) in system.processes() {
                if process.parent().is_some_and(|p| ids.contains(&p)) {
                    ids.insert(*id);
                }
            }
            if previous == ids.len() {
                break;
            }
        }
        system.process(pid).map(|_| {
            ids.into_iter()
                .filter_map(|id| system.process(id))
                .map(|p| p.memory())
                .fold(0u64, u64::saturating_add)
        })
    });
    Host {
        cpu: (!system.cpus().is_empty()).then(|| system.global_cpu_usage()),
        memory,
        kernel: System::kernel_version().unwrap_or_else(|| "—".into()),
        platform: System::long_os_version().unwrap_or_else(|| std::env::consts::OS.into()),
    }
}

async fn git(root: &Path, args: &[&str]) -> Option<String> {
    let output = tokio::process::Command::new("git")
        .args(args)
        .current_dir(root)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true)
        .output();
    let output = tokio::time::timeout(Duration::from_secs(3), output)
        .await
        .ok()?
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// Preview and rich content are exclusive. The caller retries only an unsupported rich/emoji variant.
pub fn input(
    snapshot: &Snapshot,
    config: &Config,
    english: bool,
    premium: bool,
    rich: bool,
    reply: Option<i32>,
) -> Result<InputMessage> {
    let html = snapshot.render(config, english, rich)?;
    if rich {
        let html = if config.banner_url.is_empty() {
            html
        } else {
            format!("<img src=\"{}\"/>\n{html}", escape(&config.banner_url))
        };
        return Ok(InputMessage::new()
            .reply_to(reply)
            .rich_message(InputRichMessage::html(html)));
    }
    let value = crate::bot_requests::Text {
        text: html
            .replace("<br>", "\n")
            .replace("<br/>", "\n")
            .replace("<br />", "\n"),
        parse_mode: crate::bot_requests::ParseMode::Html,
        entities: None,
    };
    let limit = if !config.banner_url.is_empty() && !config.quote_media {
        1024
    } else {
        4096
    };
    let (text, mut entities) = std::panic::catch_unwind(|| value.parse(limit, false))
        .map_err(|_| anyhow::anyhow!("invalid HTML in info template"))??;
    if premium && config.custom_message.as_ref().is_none_or(|s| s.is_empty()) {
        let mut start = 0;
        for (id, alt) in ICONS {
            if let Some(offset) = text[start..].find(alt) {
                let byte = start + offset;
                entities.get_or_insert_with(Vec::new).push(
                    tl::types::MessageEntityCustomEmoji {
                        offset: text[..byte].encode_utf16().count() as i32,
                        length: alt.encode_utf16().count() as i32,
                        document_id: *id,
                    }
                    .into(),
                );
                start = byte + alt.len();
            }
        }
    }
    let mut result = InputMessage::new()
        .text(text)
        .fmt_entities(entities.unwrap_or_default())
        .invert_media(config.invert_media)
        .reply_to(reply)
        .link_preview(!config.banner_url.is_empty());
    if !config.banner_url.is_empty() {
        result = if config.quote_media {
            result.media(tl::types::InputMediaWebPage {
                force_large_media: true,
                force_small_media: false,
                optional: true,
                url: config.banner_url.clone(),
            })
        } else {
            result.photo_url(&config.banner_url)
        };
    }
    Ok(result)
}

async fn deliver(
    client: &Client,
    message: &Message,
    input: InputMessage,
    photo: bool,
) -> Result<()> {
    if message.outgoing() && !photo {
        message.edit(input).await?;
    } else {
        client
            .send_message(crate::telegram::peer(message).await?, input)
            .await?;
        if message.outgoing() {
            // Telegram cannot turn an ordinary text command into photo media by editing it.
            // Delete only after the replacement has actually been sent.
            message.delete().await?;
        }
    }
    Ok(())
}

pub async fn send(ctx: &crate::commands::Context, message: &Message) -> Result<()> {
    let config = Config::load(&ctx.db)?;
    let (me, host, branch, commit, update) = tokio::join!(
        tokio::time::timeout(Duration::from_secs(15), ctx.client.get_me()),
        tokio::task::spawn_blocking(host),
        git(
            Path::new(env!("CARGO_MANIFEST_DIR")),
            &["branch", "--show-current"]
        ),
        git(
            Path::new(env!("CARGO_MANIFEST_DIR")),
            &["rev-parse", "HEAD"]
        ),
        tokio::time::timeout(
            Duration::from_secs(5),
            crate::updater::check(Path::new(env!("CARGO_MANIFEST_DIR")))
        )
    );
    let me = me??;
    let host = host?;
    let snapshot = Snapshot {
        owner: ctx.owner,
        name: [me.first_name(), me.last_name()]
            .into_iter()
            .flatten()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" "),
        prefix: ctx.db.prefix(),
        version: env!("CARGO_PKG_VERSION").into(),
        uptime: ctx.start.elapsed().as_secs(),
        branch: branch
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "—".into()),
        commit,
        up_to_date: update
            .ok()
            .and_then(Result::ok)
            .map(|r| r.current == r.incoming),
        cpu: host.cpu,
        memory: host.memory,
        kernel: host.kernel,
        platform: host.platform,
    };
    let english = ctx.db.language().split_whitespace().next() == Some("en");
    let reply = match message.reply_header() {
        Some(tl::enums::MessageReplyHeader::Header(h)) => h.reply_to_msg_id,
        _ => None,
    }
    .or_else(|| crate::telegram::topic(message))
    .or_else(|| (!message.outgoing()).then_some(message.id()));
    let premium = matches!(&me.raw, tl::enums::User::User(user) if user.premium);
    if config.rich_mode && premium {
        let rich = snapshot.render(&config, english, true)?;
        let source = if config.banner_url.is_empty() {
            rich
        } else {
            format!("<img src=\"{}\"/>\n{rich}", escape(&config.banner_url))
        };
        let result = if message.outgoing() {
            ctx.client
                .invoke(&crate::bot_edits::rich(
                    crate::telegram::peer(message).await?,
                    message.id(),
                    &crate::bot_edits::RichText::Html { source },
                    None,
                )?)
                .await
                .map(|_| ())
        } else {
            ctx.client
                .send_message(
                    crate::telegram::peer(message).await?,
                    input(&snapshot, &config, english, true, true, reply)?,
                )
                .await
                .map(|_| ())
        };
        match result {
            Ok(()) => return Ok(()),
            Err(e) if unsupported(&e, true) => {}
            Err(e) => return Err(e.into()),
        }
    }
    let mut config = config;
    let mut emoji = premium;
    loop {
        let input_value = input(&snapshot, &config, english, emoji, false, reply)?;
        let photo = !config.banner_url.is_empty() && !config.quote_media;
        match deliver(&ctx.client, message, input_value, photo).await {
            Ok(()) => return Ok(()),
            Err(error) => {
                let rpc = error.downcast_ref::<grammers_client::InvocationError>();
                if !config.banner_url.is_empty() && rpc.is_some_and(|e| matches!(e, grammers_client::InvocationError::Rpc(r) if r.name == "WEBPAGE_MEDIA_EMPTY")) {
                    config.banner_url.clear();
                } else if emoji && rpc.is_some_and(|e| unsupported(e, false)) {
                    emoji = false;
                } else { return Err(error); }
            }
        }
    }
}
fn unsupported(error: &grammers_client::InvocationError, rich: bool) -> bool {
    matches!(error, grammers_client::InvocationError::Rpc(e) if matches!(e.name.as_str(), "PREMIUM_ACCOUNT_REQUIRED" | "CUSTOM_EMOJI_INVALID") || rich && matches!(e.name.as_str(), "RICH_MESSAGE_INVALID" | "RICH_MESSAGE_NOT_ALLOWED"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_host_measurements_are_available_without_telegram() {
        let value = host();
        assert!(!value.kernel.is_empty());
        assert!(!value.platform.is_empty());
        if sysinfo::IS_SUPPORTED_SYSTEM {
            assert!(value.memory.is_some_and(|n| n > 0));
            assert!(
                value
                    .cpu
                    .is_some_and(|n| n.is_finite() && (0.0..=100.0).contains(&n))
            );
        }
    }
}
