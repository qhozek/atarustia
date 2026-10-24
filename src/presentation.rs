//! User-facing Ataraxia cards. Transport/build identity is deliberately kept separate.
use crate::{
    commands::Context,
    markup::{Key, Kind, Style},
};
use anyhow::Result;
use grammers_client::message::{InputMessage, Message};
use serde::{Deserialize, Serialize};

pub const PING_TEMPLATE: &str = "<blockquote><emoji document_id=5920515922505765329>⚡</emoji> <b>Ping:</b> {ping}</blockquote>\n<blockquote><emoji document_id=5900104897885376843>🕓</emoji> <b>Uptime:</b> {uptime}</blockquote>";
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PingConfig {
    pub custom_message: String,
    pub rich_mode: bool,
    pub hint: Option<String>,
    pub ping_emoji: String,
    pub banner_url: Option<String>,
    pub quote_media: bool,
    pub invert_media: bool,
}
impl Default for PingConfig {
    fn default() -> Self {
        Self {
            custom_message: PING_TEMPLATE.into(),
            rich_mode: false,
            hint: None,
            ping_emoji: "⛈️".into(),
            banner_url: None,
            quote_media: false,
            invert_media: false,
        }
    }
}
impl PingConfig {
    pub fn load(db: &crate::database::Database) -> Result<Self> {
        let saved = db.get("Tester", "__config__");
        let config = if saved.is_null() {
            Self::default()
        } else {
            serde_json::from_value(saved)?
        };
        Ok(config)
    }
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            self.custom_message.len() <= 16384 && self.ping_emoji.len() <= 256,
            "ping template too large"
        );
        if let Some(url) = self.banner_url.as_ref().filter(|s| !s.is_empty()) {
            let url = reqwest::Url::parse(url)?;
            anyhow::ensure!(
                matches!(url.scheme(), "https" | "http"),
                "banner must be HTTP(S)"
            );
        }
        Ok(())
    }
}
pub fn ping_text(config: &PingConfig, ping: f64, uptime: u64, rich: bool, hint: bool) -> String {
    ping_render(config, ping, uptime, rich, hint, &[])
}
pub fn ping_render(
    config: &PingConfig,
    ping: f64,
    uptime: u64,
    rich: bool,
    hint: bool,
    extra: &[(&str, String)],
) -> String {
    let mut values = std::collections::BTreeMap::from([
        ("ping", ping.to_string()),
        ("uptime", crate::info::uptime(uptime)),
        (
            "ping_hint",
            if hint {
                config.hint.clone().unwrap_or_default()
            } else {
                String::new()
            },
        ),
    ]);
    for (k, v) in extra {
        values.insert(*k, v.clone());
    }
    let source = crate::info::substitute(&config.custom_message, &values)
        .unwrap_or_else(|_| "<tg-emoji emoji-id=5210952531676504517>🚫</tg-emoji>".into());
    crate::original::emoji_html(&source, rich)
}
pub async fn ping(ctx: &Context, message: &Message) -> Result<()> {
    let config = PingConfig::load(&ctx.db)?;
    config.validate()?;
    let start = std::time::Instant::now();
    let shown = if message.outgoing() {
        message.edit(&config.ping_emoji).await?;
        message.clone()
    } else {
        message.reply(&config.ping_emoji).await?
    };
    let elapsed = (start.elapsed().as_secs_f64() * 1e6).round() / 1000.;
    let premium = ctx
        .db
        .get("RustRuntime", "premium")
        .as_bool()
        .unwrap_or(false);
    let mut extra = vec![];
    if [
        "hostname",
        "user",
        "me",
        "prefix",
        "codename",
        "kernel",
        "platform",
        "cpu_usage",
        "ram_usage",
        "branch",
        "commit",
        "upd",
    ]
    .iter()
    .any(|key| config.custom_message.contains(&format!("{{{key}}}")))
    {
        // These are real account/host values, never invented to make a template look complete.
        let (snapshot, _) = crate::info::snapshot(ctx).await?;
        extra = vec![
            ("hostname", sysinfo::System::host_name().unwrap_or_default()),
            (
                "user",
                std::env::var("USER")
                    .or_else(|_| std::env::var("USERNAME"))
                    .unwrap_or_default(),
            ),
            (
                "me",
                format!(
                    "<b><a href=\"tg://user?id={}\">{}</a></b>",
                    ctx.owner,
                    crate::output::escape(&snapshot.name)
                ),
            ),
            (
                "prefix",
                format!("«<code>{}</code>»", crate::output::escape(&ctx.db.prefix())),
            ),
            ("codename", crate::original::CODENAME.into()),
            (
                "cpu_usage",
                snapshot
                    .cpu
                    .map(|n| format!("{n:.2}"))
                    .unwrap_or_else(|| "—".into()),
            ),
            (
                "ram_usage",
                snapshot
                    .memory
                    .map(|n| format!("{:.1} MB", n as f64 / 1048576.))
                    .unwrap_or_else(|| "—".into()),
            ),
            ("branch", snapshot.branch),
            (
                "commit",
                snapshot
                    .commit
                    .map(|sha| {
                        format!(
                            "<a href=\"https://github.com/qhozek/atarustia/commit/{sha}\">#{}</a>",
                            &sha[..7]
                        )
                    })
                    .unwrap_or_else(|| "—".into()),
            ),
            (
                "upd",
                match snapshot.up_to_date {
                    Some(true) => ctx.original("ataraxia_info", "up-to-date", &[]),
                    Some(false) => crate::original::message(
                        ctx.db.language().starts_with("en"),
                        "ataraxia_info",
                        "update_required",
                        &[],
                        &[("prefix", crate::output::escape(&ctx.db.prefix()))],
                    ),
                    None => String::new(),
                },
            ),
            (
                "kernel",
                sysinfo::System::kernel_version().unwrap_or_default(),
            ),
            (
                "platform",
                sysinfo::System::long_os_version().unwrap_or_default(),
            ),
        ];
    }
    let source = ping_render(
        &config,
        elapsed,
        ctx.start.elapsed().as_secs(),
        true,
        rand::random::<u8>().is_multiple_of(3),
        &extra,
    );
    if config.rich_mode && premium {
        let html = if let Some(url) = config.banner_url.as_ref().filter(|s| !s.is_empty()) {
            format!("<img src=\"{}\"/>\n{source}", crate::output::escape(url))
        } else {
            source.clone()
        };
        let request = crate::bot_edits::rich(
            crate::telegram::peer(&shown).await?,
            shown.id(),
            &crate::bot_edits::RichText::Html { source: html },
            None,
        )?;
        match ctx.client.invoke(&request).await {
            Ok(_) => return Ok(()),
            Err(grammers_client::InvocationError::Rpc(e))
                if e.name == "PREMIUM_ACCOUNT_REQUIRED" => {}
            Err(e) => return Err(e.into()),
        }
    }
    let body = crate::update_ui::user_body(
        &crate::forms::Screen {
            text: source,
            ..Default::default()
        },
        premium,
    )?
    .invert_media(config.invert_media);
    let body = if let Some(url) = config.banner_url.as_ref().filter(|s| !s.is_empty()) {
        if config.quote_media {
            body.media(grammers_client::tl::types::InputMediaWebPage {
                url: url.into(),
                force_large_media: false,
                force_small_media: false,
                optional: true,
            })
        } else {
            body.photo_url(url)
        }
    } else {
        body
    };
    shown.edit(body).await?;
    Ok(())
}
fn url(english: bool, module: &str, label: &str, target: &str, emoji: i64, style: Style) -> Key {
    Key {
        text: crate::original::text(english, module, label),
        kind: Kind::Url { url: target.into() },
        style,
        emoji_id: Some(emoji),
    }
}
pub fn community(english: bool, module: &str) -> Vec<Key> {
    vec![
        url(
            english,
            module,
            "btn_channel",
            "https://t.me/ataraxia_ub",
            6021418126061605425,
            Style::Primary,
        ),
        url(
            english,
            module,
            "btn_chat",
            "https://t.me/ataraxia_talks",
            6032609071373226027,
            Style::Primary,
        ),
    ]
}
pub fn start_card(english: bool) -> Result<InputMessage> {
    let text = crate::original::message(
        english,
        "inline_stuff",
        "this_is_ataraxia",
        &["⛈️".into(), "⛈️".into()],
        &[],
    );
    let rows = vec![
        community(english, "inline_stuff"),
        vec![url(
            english,
            "inline_stuff",
            "btn_source",
            "https://git.break.wtf/xenside/ataraxia",
            6039630677182254664,
            Style::Default,
        )],
    ];
    Ok(InputMessage::new()
        .html(crate::original::emoji_html(&text, false))
        .link_preview(false)
        .reply_markup(crate::markup::keyboard(&rows)?))
}
pub fn quickstart(english: bool, premium: bool) -> Result<InputMessage> {
    let text = crate::original::message(
        english,
        "quickstart",
        "base",
        &[if premium {
            crate::update_ui::PREMIUM_LOGO.into()
        } else {
            "ataraxia".into()
        }],
        &[],
    );
    let languages = vec![
        Key {
            text: crate::original::text(true, "quickstart", "language"),
            kind: Kind::Callback {
                data: "ataraxia/lang/en".into(),
            },
            style: Style::Default,
            emoji_id: None,
        },
        Key {
            text: crate::original::text(false, "quickstart", "language"),
            kind: Kind::Callback {
                data: "ataraxia/lang/ru".into(),
            },
            style: Style::Default,
            emoji_id: None,
        },
    ];
    Ok(InputMessage::new()
        .html(crate::original::emoji_html(&text, false))
        .link_preview(false)
        .reply_markup(crate::markup::keyboard(&[
            languages,
            community(english, "quickstart"),
        ])?))
}
pub async fn greet(ctx: &Context, bot: &crate::bot::Bot) -> Result<()> {
    if ctx
        .db
        .get("Quickstart", "no_msg")
        .as_bool()
        .unwrap_or(false)
    {
        return Ok(());
    }
    let english = ctx.db.language().starts_with("en");
    let premium = ctx
        .db
        .get("RustRuntime", "premium")
        .as_bool()
        .unwrap_or(false);
    bot.send_message(
        grammers_session::types::PeerId::user(ctx.owner)
            .unwrap()
            .to_ambient_ref(),
        quickstart(english, premium)?,
    )
    .await?;
    ctx.db.set("Quickstart", "no_msg", true)?;
    Ok(())
}
pub fn no_inline_commands(english: bool) -> grammers_client::update::Article {
    use grammers_client::update::Article;
    Article::new(
        crate::original::text(english, "$inline", "show_inline_cmds"),
        InputMessage::new()
            .html(crate::original::text(
                english,
                "$inline",
                "no_inline_cmds_msg",
            ))
            .link_preview(false),
    )
    .description(crate::original::text(english, "$inline", "no_inline_cmds"))
}
pub async fn inline_catalog(
    ctx: &Context,
    user: i64,
) -> Result<Vec<grammers_client::update::Article>> {
    use grammers_client::{
        message::{Button, ReplyMarkup},
        update::Article,
    };
    let english = ctx.db.language().starts_with("en");
    let packages = ctx.modules.lock().unwrap().list();
    let mut commands = vec![];
    for module in packages {
        for c in module.commands {
            if c.inline
                && crate::security_commands::inline_allowed(&ctx.db, ctx.owner, user, &c.name)
            {
                commands.push(c);
            }
        }
    }
    if commands.is_empty() {
        return Ok(vec![no_inline_commands(english)]);
    }
    let bot = ctx
        .bot
        .read()
        .await
        .as_ref()
        .map(|b| b.bot.username.clone())
        .unwrap_or_default();
    let mut results = vec![];
    let mut lines = vec![];
    for c in commands.into_iter().take(49) {
        let body = crate::original::message(
            english,
            "$inline",
            "command_msg",
            &[
                crate::output::escape(&c.name),
                crate::output::escape(&c.description),
            ],
            &[],
        );
        let input = InputMessage::new()
            .html(body)
            .link_preview(false)
            .reply_markup(ReplyMarkup::from_buttons(&[vec![Button::switch(
                crate::original::text(english, "$inline", "run_command"),
                format!("{} ", c.name),
            )]]));
        results.push(
            Article::new(
                crate::original::message(
                    english,
                    "$inline",
                    "command",
                    std::slice::from_ref(&c.name),
                    &[],
                ),
                input,
            )
            .description(&c.description),
        );
        lines.push(format!(
            "🎹 <code>@{bot} {}</code> - {}\n",
            crate::output::escape(&c.name),
            crate::output::escape(&c.description)
        ));
    }
    let summary = Article::new(
        crate::original::text(english, "$inline", "show_inline_cmds"),
        InputMessage::new()
            .html(crate::original::message(
                english,
                "$inline",
                "inline_cmds_msg",
                &[lines.join("\n")],
                &[],
            ))
            .link_preview(false),
    )
    .description(crate::original::message(
        english,
        "$inline",
        "inline_cmds",
        &[results.len().to_string()],
        &[],
    ));
    results.insert(0, summary);
    Ok(results)
}
