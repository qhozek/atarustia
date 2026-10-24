//! Updater screens. Text and opaque callbacks are built independently of Telegram I/O.
use crate::{
    forms::{Action, Key, Screen},
    markup::{Appearance, Style},
    output::escape,
    updater::{Changes, Report},
};
use serde::{Deserialize, Serialize};

pub const MODULE: &str = "Updater";
pub const PREMIUM_LOGO: &str = "<tg-emoji emoji-id=5404651863054260920>⛈️</tg-emoji><tg-emoji emoji-id=5404474175962258931>⛈️</tg-emoji><tg-emoji emoji-id=5404590359122581779>⛈️</tg-emoji><tg-emoji emoji-id=5404537174542560251>⛈️</tg-emoji>";

/// Preserve UTF-16 entity positions by letting the HTML parser track an intermediate text URL.
pub fn user_body(
    screen: &Screen,
    premium: bool,
) -> anyhow::Result<grammers_client::message::InputMessage> {
    use grammers_client::{
        message::{InputMessage, ReplyMarkup},
        tl,
    };
    let mut html = screen.text.clone();
    if premium {
        html = html.replace("<b>ataraxia</b>", PREMIUM_LOGO);
        if html.starts_with("⛈️ ") {
            html = format!("{PREMIUM_LOGO} {}", &html["⛈️ ".len()..]);
        }
    }
    let mut source = String::new();
    let mut rest = html.as_str();
    while let Some(start) = rest.find("<tg-emoji emoji-id=") {
        source.push_str(&rest[..start]);
        rest = &rest[start + "<tg-emoji emoji-id=".len()..];
        let end = rest
            .find('>')
            .ok_or_else(|| anyhow::anyhow!("invalid emoji tag"))?;
        let id = rest[..end].trim_matches(['\"', '\'']).parse::<i64>()?;
        anyhow::ensure!(id > 0, "invalid emoji ID");
        rest = &rest[end + 1..];
        let end = rest
            .find("</tg-emoji>")
            .ok_or_else(|| anyhow::anyhow!("unclosed emoji tag"))?;
        if premium {
            source += &format!("<a href=\"atarustia-emoji:{id}\">{}</a>", &rest[..end]);
        } else {
            source.push_str(&rest[..end]);
        }
        rest = &rest[end + "</tg-emoji>".len()..];
    }
    source.push_str(rest);
    let source = normalize_time(&source)?;
    let (text, entities) = crate::bot_requests::Text {
        text: source,
        parse_mode: crate::bot_requests::ParseMode::Html,
        entities: None,
    }
    .parse(4096, false)?;
    let entities = entities
        .unwrap_or_default()
        .into_iter()
        .map(|entity| match entity {
            tl::enums::MessageEntity::TextUrl(url) if url.url.starts_with("atarustia-emoji:") => {
                Ok(tl::types::MessageEntityCustomEmoji {
                    offset: url.offset,
                    length: url.length,
                    document_id: url.url["atarustia-emoji:".len()..].parse()?,
                }
                .into())
            }
            other => Ok(other),
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    Ok(InputMessage::new()
        .text(text)
        .fmt_entities(entities)
        .link_preview(false)
        .reply_markup(ReplyMarkup::from_buttons(&[])))
}
pub fn normalize_time(html: &str) -> anyhow::Result<String> {
    let mut result = String::new();
    let mut rest = html;
    while let Some(start) = rest.find("<tg-time unix=\"") {
        result.push_str(&rest[..start]);
        rest = &rest[start + "<tg-time unix=\"".len()..];
        let end = rest
            .find('"')
            .ok_or_else(|| anyhow::anyhow!("invalid time tag"))?;
        let unix = rest[..end].parse::<i64>()?;
        let time = chrono::DateTime::from_timestamp(unix, 0)
            .ok_or_else(|| anyhow::anyhow!("invalid timestamp"))?;
        let end = rest
            .find('>')
            .ok_or_else(|| anyhow::anyhow!("unclosed time tag"))?;
        rest = &rest[end + 1..];
        result.push_str(&format!(
            "<time datetime=\"{}\" data-format=\"r\">",
            time.to_rfc3339()
        ));
    }
    result.push_str(rest);
    Ok(result.replace("</tg-time>", "</time>"))
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub update_notifications: bool,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            update_notifications: true,
        }
    }
}
impl Config {
    pub fn load(db: &crate::database::Database) -> anyhow::Result<Self> {
        let value = db.get(MODULE, "__config__");
        Ok(if value.is_null() {
            Self::default()
        } else {
            serde_json::from_value(value)?
        })
    }
}
#[derive(Clone, Copy)]
pub struct Ui {
    pub english: bool,
}
impl Ui {
    pub fn new(db: &crate::database::Database) -> Self {
        Self {
            english: db.language().split_whitespace().next() == Some("en"),
        }
    }
    pub fn text(&self, key: &str, values: &[(&str, String)]) -> String {
        if key != "sync_failed" {
            let positional_names: &[&str] = match key {
                "more_commits" => &["count"],
                "update_failed" => &["error"],
                "modules_failed" => &["names"],
                "restart_success" => &["took"],
                "restart_fail" => &["took", "failed"],
                _ => &[],
            };
            let positional = positional_names
                .iter()
                .map(|name| {
                    values
                        .iter()
                        .find(|(k, _)| k == name)
                        .map(|(_, v)| v.clone())
                        .unwrap_or_default()
                })
                .collect::<Vec<_>>();
            return crate::original::message(self.english, "updater", key, &positional, values);
        }
        let pack: serde_json::Value = serde_json::from_str(if self.english {
            include_str!("../locales/updater.en.json")
        } else {
            include_str!("../locales/updater.ru.json")
        })
        .expect("embedded updater locale");
        let source = pack[key].as_str().unwrap_or(key);
        let mut result = String::new();
        let mut rest = source;
        // Substitute in the source only: Git subjects and error messages are never reinterpreted.
        while let Some(start) = rest.find('{') {
            result.push_str(&rest[..start]);
            rest = &rest[start..];
            if let Some(end) = rest.find('}') {
                let name = &rest[1..end];
                result.push_str(
                    values
                        .iter()
                        .find(|(k, _)| *k == name)
                        .map(|(_, v)| v.as_str())
                        .unwrap_or(&rest[..=end]),
                );
                rest = &rest[end + 1..];
            } else {
                break;
            }
        }
        result.push_str(rest);
        result
    }
    pub fn plural(&self, count: u64, kind: &str) -> String {
        let word = if self.english {
            match (kind, count == 1) {
                ("commits", true) => "commit",
                ("commits", false) => "commits",
                ("files", true) => "file",
                ("files", false) => "files",
                (_, true) => "module",
                (_, false) => "modules",
            }
        } else {
            let forms = match kind {
                "commits" => ["коммит", "коммита", "коммитов"],
                "files" => ["файл", "файла", "файлов"],
                _ => ["модуль", "модуля", "модулей"],
            };
            forms[if (11..=14).contains(&(count % 100)) {
                2
            } else {
                match count % 10 {
                    1 => 0,
                    2..=4 => 1,
                    _ => 2,
                }
            }]
        };
        format!("{count} {word}")
    }
    pub fn duration(&self, millis: u64) -> String {
        if millis < 1000 {
            format!("{millis} {}", if self.english { "ms" } else { "мс" })
        } else {
            format!(
                "{:.1} {}",
                millis as f64 / 1000.0,
                if self.english { "s" } else { "с" }
            )
        }
    }
    pub fn commits(&self, changes: &Changes) -> String {
        if changes.commits.is_empty() {
            return String::new();
        }
        let mut lines = changes
            .commits
            .iter()
            .take(15)
            .map(|c| {
                self.text(
                    "commit_line",
                    &[
                        (
                            "url",
                            format!(
                                "https://github.com/qhozek/atarustia/commit/{}",
                                escape(&c.hash)
                            ),
                        ),
                        (
                            "commit",
                            escape(&c.hash.chars().take(7).collect::<String>()),
                        ),
                        (
                            "subject",
                            escape(&c.subject.chars().take(80).collect::<String>()),
                        ),
                    ],
                )
            })
            .collect::<Vec<_>>();
        if changes.total > lines.len() as u64 {
            lines.push(self.text(
                "more_commits",
                &[("count", (changes.total - lines.len() as u64).to_string())],
            ));
        }
        format!("\n<blockquote expandable>{}</blockquote>", lines.join("\n"))
    }
    pub fn card(&self, report: &Report, notification: bool) -> Screen {
        let mut lines = vec![
            self.text(
                "update_build",
                &[
                    ("branch", escape(&report.branch)),
                    (
                        "old",
                        escape(&report.current.chars().take(7).collect::<String>()),
                    ),
                    (
                        "new",
                        escape(&report.incoming.chars().take(7).collect::<String>()),
                    ),
                ],
            ),
            self.text(
                "update_stats",
                &[
                    ("commits", self.plural(report.changes.total, "commits")),
                    ("files", self.plural(report.changes.files, "files")),
                    ("plus", report.changes.plus.to_string()),
                    ("minus", report.changes.minus.to_string()),
                ],
            ),
        ];
        if let Some(date) = chrono::DateTime::from_timestamp(report.changes.date, 0)
            .filter(|_| report.changes.date > 0)
        {
            lines.push(self.text(
                "update_date",
                &[
                    ("date", report.changes.date.to_string()),
                    ("text", date.format("%d.%m.%Y %H:%M UTC").to_string()),
                ],
            ));
        }
        let html = format!(
            "{}\n<blockquote>{}</blockquote>{}",
            self.text("update_title", &[("logo", "⛈️".into())]),
            lines.join("\n"),
            self.commits(&report.changes)
        );
        let mut screen = screen(html);
        if notification {
            screen.ttl_seconds = 86400;
        }
        screen.buttons = vec![
            vec![
                key(
                    self.text("btn_update", &[]),
                    Action::Update {
                        current: report.current.clone(),
                        incoming: report.incoming.clone(),
                        retry: false,
                    },
                ),
                Key {
                    text: self.text("btn_changes", &[]),
                    action: None,
                    url: Some(format!(
                        "https://github.com/qhozek/atarustia/compare/{}...{}",
                        report.current, report.incoming
                    )),
                },
            ],
            vec![key(
                self.text(
                    if notification {
                        "btn_dismiss"
                    } else {
                        "cancel"
                    },
                    &[],
                ),
                Action::UpdateDismiss,
            )],
        ];
        screen.button_appearance = vec![
            vec![
                appearance(Style::Success, 5807601906603792598),
                appearance(Style::Default, 6021452855167164269),
            ],
            vec![appearance(Style::Danger, 5807692706507399432)],
        ];
        screen
    }
    pub fn current(&self, report: &Report) -> Screen {
        screen(self.text(
            "no_update",
            &[
                ("branch", escape(&report.branch)),
                (
                    "commit",
                    escape(&report.current.chars().take(7).collect::<String>()),
                ),
            ],
        ))
    }
    pub fn restart(&self) -> Screen {
        let mut result =
            screen(self.text("restart_confirm", &[("logo", "<b>ataraxia</b>".into())]));
        result.buttons = vec![vec![
            key(
                format!("🔄 {}", self.text("btn_restart", &[])),
                Action::Restart,
            ),
            key(
                format!("❌ {}", self.text("cancel", &[])),
                Action::UpdateDismiss,
            ),
        ]];
        result.button_appearance = vec![vec![
            appearance(Style::Primary, 5807900819147724161),
            appearance(Style::Danger, 5807692706507399432),
        ]];
        result
    }
    pub fn progress(&self, done: &[(usize, u64)], active: usize) -> Screen {
        let names = ["step_download", "step_deps", "step_restart"];
        let steps = names
            .iter()
            .enumerate()
            .map(|(i, key)| {
                if let Some((_, took)) = done.iter().find(|(index, _)| *index == i) {
                    self.text(
                        "step_done",
                        &[
                            ("name", self.text(key, &[])),
                            ("took", self.duration(*took)),
                        ],
                    )
                } else {
                    self.text(
                        if i == active {
                            "step_active"
                        } else {
                            "step_todo"
                        },
                        &[("name", self.text(key, &[]))],
                    )
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        screen(self.text(
            "update_progress",
            &[("logo", "⛈️".into()), ("steps", steps)],
        ))
    }
    pub fn failed(&self, error: &str, sync: Option<(&str, &str)>) -> Screen {
        let tail = error
            .chars()
            .rev()
            .take(1200)
            .collect::<String>()
            .chars()
            .rev()
            .collect::<String>();
        let mut result = screen(self.text(
            if sync.is_some() {
                "sync_failed"
            } else {
                "update_failed"
            },
            &[("error", escape(&tail))],
        ));
        if let Some((current, incoming)) = sync {
            result.buttons = vec![vec![
                key(
                    self.text("btn_retry", &[]),
                    Action::Update {
                        current: current.into(),
                        incoming: incoming.into(),
                        retry: true,
                    },
                ),
                key(self.text("cancel", &[]), Action::UpdateDismiss),
            ]];
            result.button_appearance = vec![vec![
                appearance(Style::Primary, 5807601906603792598),
                appearance(Style::Danger, 5807692706507399432),
            ]];
        }
        result
    }
    pub fn restarting(&self) -> Screen {
        screen(self.text("restarting_caption", &[("logo", "<b>ataraxia</b>".into())]))
    }
    pub fn completed(
        &self,
        update: bool,
        commit: &str,
        took: u64,
        loaded: usize,
        failed: &[String],
        changes: Option<&Changes>,
    ) -> Screen {
        let failed_text = if failed.is_empty() {
            String::new()
        } else {
            self.text(
                "modules_failed",
                &[(
                    "names",
                    failed
                        .iter()
                        .map(|n| format!("<code>{}</code>", escape(n)))
                        .collect::<Vec<_>>()
                        .join(", "),
                )],
            )
        };
        let values = [
            ("logo", "<b>ataraxia</b>".into()),
            (
                "url",
                format!(
                    "https://github.com/qhozek/atarustia/commit/{}",
                    escape(commit)
                ),
            ),
            (
                "commit",
                escape(&commit.chars().take(7).collect::<String>()),
            ),
            ("took", self.duration(took)),
            ("modules", self.plural(loaded as u64, "modules")),
            ("failed", failed_text.clone()),
        ];
        let mut html = self.text(
            if update {
                "update_success"
            } else if failed.is_empty() {
                "restart_success"
            } else {
                "restart_fail"
            },
            &values,
        );
        if update && !failed.is_empty() {
            html += &format!("\n<blockquote>{failed_text}</blockquote>");
        }
        if let Some(changes) = changes {
            html += &self.commits(changes);
        }
        screen(html)
    }
}
fn key(text: String, action: Action) -> Key {
    Key {
        text,
        action: Some(action),
        url: None,
    }
}
fn appearance(style: Style, id: i64) -> Appearance {
    Appearance {
        style,
        emoji_id: Some(id),
    }
}
/// The ordinary MTProto HTML parser ignores tg-emoji tags, retaining their visible alt text.
pub fn screen(html: String) -> Screen {
    let html = normalize_time(&html).unwrap_or(html);
    Screen {
        text: html.clone(),
        pages: vec![html],
        html: true,
        close_button: false,
        link_preview: false,
        ttl_seconds: 1800,
        ..Default::default()
    }
}
