//! Reusable typed keyboard builder: callbacks, copy/switch, web apps, style and custom emoji.
use anyhow::{Result, ensure};
use grammers_client::{
    message::{Button, ReplyMarkup},
    tl,
};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Kind {
    Callback {
        data: String,
    },
    Url {
        url: String,
    },
    Copy {
        text: String,
    },
    SwitchInline {
        query: String,
        #[serde(default)]
        same_chat: bool,
    },
    WebApp {
        url: String,
    },
    Game,
    Pay,
    UserProfile {
        user_id: i64,
    },
    LoginUrl {
        url: String,
        #[serde(default)]
        forward_text: Option<String>,
        #[serde(default)]
        request_write_access: bool,
    },
    Disabled,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Style {
    #[default]
    Default,
    Primary,
    Success,
    Danger,
    Link,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Appearance {
    pub style: Style,
    pub emoji_id: Option<i64>,
}
impl Appearance {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.emoji_id.is_none_or(|id| id > 0),
            "invalid custom emoji ID"
        );
        Ok(())
    }
    pub fn to_tl(&self) -> Result<Option<tl::enums::KeyboardButtonStyle>> {
        self.validate()?;
        Ok(
            if !matches!(self.style, Style::Default | Style::Link) || self.emoji_id.is_some() {
                Some(
                    tl::types::KeyboardButtonStyle {
                        bg_primary: matches!(self.style, Style::Primary),
                        bg_success: matches!(self.style, Style::Success),
                        bg_danger: matches!(self.style, Style::Danger),
                        icon: self.emoji_id,
                    }
                    .into(),
                )
            } else {
                None
            },
        )
    }
    pub fn apply(&self, button: &mut Button) -> Result<()> {
        let tl::enums::KeyboardInlineButton::Button(button) = &mut button.raw;
        button.style = self.to_tl()?;
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Key {
    pub text: String,
    pub kind: Kind,
    #[serde(default)]
    pub style: Style,
    #[serde(default)]
    pub emoji_id: Option<i64>,
}
impl Key {
    pub fn button(&self) -> Result<Button> {
        ensure!(
            !self.text.is_empty() && self.text.chars().count() <= 64,
            "button text must contain 1–64 characters"
        );
        ensure!(
            self.emoji_id.is_none_or(|id| id > 0),
            "invalid custom emoji ID"
        );
        let kind: tl::enums::InlineButtonType = match &self.kind {
            Kind::Game => tl::enums::InlineButtonType::Game,
            Kind::Pay => tl::enums::InlineButtonType::Buy,
            Kind::UserProfile { user_id } => {
                ensure!(
                    grammers_session::types::PeerId::user(*user_id).is_some(),
                    "invalid profile user ID"
                );
                tl::types::InlineButtonTypeUrl {
                    url: format!("tg://user?id={user_id}"),
                }
                .into()
            }
            Kind::LoginUrl {
                url,
                forward_text,
                request_write_access,
            } => {
                let parsed = reqwest::Url::parse(url)?;
                ensure!(
                    parsed.scheme() == "https" && parsed.host_str().is_some() && url.len() <= 8192,
                    "login URL must use HTTPS"
                );
                ensure!(
                    forward_text
                        .as_ref()
                        .is_none_or(|t| t.chars().count() <= 64),
                    "login forward text too long"
                );
                tl::types::InputInlineButtonTypeUrlAuth {
                    url: url.clone(),
                    fwd_text: forward_text.clone(),
                    request_write_access: *request_write_access,
                    bot: None,
                }
                .into()
            }
            Kind::Callback { data } => {
                ensure!(
                    !data.is_empty() && data.len() <= 64,
                    "callback data must contain 1–64 bytes"
                );
                tl::types::InlineButtonTypeCallback {
                    data: data.as_bytes().into(),
                    requires_password: false,
                }
                .into()
            }
            Kind::Url { url } | Kind::WebApp { url } => {
                let parsed = reqwest::Url::parse(url)?;
                ensure!(
                    matches!(parsed.scheme(), "https" | "tg"),
                    "invalid button URL"
                );
                if matches!(self.kind, Kind::WebApp { .. }) {
                    ensure!(parsed.scheme() == "https", "web app requires HTTPS");
                    tl::types::InlineButtonTypeWebView { url: url.clone() }.into()
                } else {
                    tl::types::InlineButtonTypeUrl { url: url.clone() }.into()
                }
            }
            Kind::Copy { text } => {
                ensure!(
                    !text.is_empty() && text.chars().count() <= 256,
                    "copy text must contain 1–256 characters"
                );
                tl::types::InlineButtonTypeCopy {
                    copy_text: text.clone(),
                }
                .into()
            }
            Kind::SwitchInline { query, same_chat } => {
                ensure!(query.len() <= 1024, "inline query too long");
                tl::types::InlineButtonTypeSwitchInline {
                    query: query.clone(),
                    same_peer: *same_chat,
                    peer_types: None,
                }
                .into()
            }
            Kind::Disabled => tl::enums::InlineButtonType::Disabled,
        };
        let style =
            if !matches!(self.style, Style::Default | Style::Link) || self.emoji_id.is_some() {
                Some(
                    tl::types::KeyboardButtonStyle {
                        bg_primary: matches!(self.style, Style::Primary),
                        bg_success: matches!(self.style, Style::Success),
                        bg_danger: matches!(self.style, Style::Danger),
                        icon: self.emoji_id,
                    }
                    .into(),
                )
            } else {
                None
            };
        Ok(Button {
            raw: tl::types::KeyboardInlineButton {
                text: self.text.clone(),
                r#type: kind,
                style,
            }
            .into(),
        })
    }
}
pub fn keyboard(rows: &[Vec<Key>]) -> Result<ReplyMarkup> {
    ensure!(
        rows.iter().all(|r| r.len() <= 8) && rows.iter().map(Vec::len).sum::<usize>() <= 100,
        "keyboard exceeds Telegram limits"
    );
    for (row_index, row) in rows.iter().enumerate() {
        for (column_index, key) in row.iter().enumerate() {
            ensure!(
                !matches!(key.kind, Kind::Game | Kind::Pay)
                    || (row_index == 0 && column_index == 0),
                "game/pay button must be first in the first row"
            );
        }
    }
    let rows = rows
        .iter()
        .map(|r| r.iter().map(Key::button).collect::<Result<Vec<_>>>())
        .collect::<Result<Vec<_>>>()?;
    Ok(ReplyMarkup::from_buttons(&rows))
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReplyKeyKind {
    Text,
    RequestContact,
    RequestLocation,
    RequestPoll {
        quiz: Option<bool>,
    },
    WebApp {
        url: String,
    },
    RequestPeer {
        button_id: i32,
        criteria: Box<crate::request_peer::Criteria>,
        max_quantity: i32,
        #[serde(default)]
        name_requested: bool,
        #[serde(default)]
        username_requested: bool,
        #[serde(default)]
        photo_requested: bool,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplyKey {
    pub text: String,
    pub kind: ReplyKeyKind,
    #[serde(default)]
    pub appearance: Appearance,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Keyboard {
    Inline {
        rows: Vec<Vec<Key>>,
    },
    Reply {
        rows: Vec<Vec<ReplyKey>>,
        #[serde(default)]
        resize: bool,
        #[serde(default)]
        one_time: bool,
        #[serde(default)]
        selective: bool,
        #[serde(default)]
        persistent: bool,
        #[serde(default)]
        placeholder: Option<String>,
    },
    Remove {
        #[serde(default)]
        selective: bool,
    },
    ForceReply {
        #[serde(default)]
        selective: bool,
        #[serde(default)]
        one_time: bool,
        #[serde(default)]
        placeholder: Option<String>,
    },
}
impl Keyboard {
    pub fn to_tl(&self) -> Result<tl::enums::ReplyMarkup> {
        let placeholder = match self {
            Self::Reply { placeholder, .. } | Self::ForceReply { placeholder, .. } => {
                placeholder.as_deref()
            }
            _ => None,
        };
        ensure!(
            placeholder.is_none_or(|s| !s.is_empty() && s.encode_utf16().count() <= 64),
            "keyboard placeholder must contain 1–64 UTF-16 units"
        );
        Ok(match self {
            Self::Inline { rows } => keyboard(rows)?.raw,
            Self::Remove { selective } => tl::types::ReplyKeyboardHide {
                selective: *selective,
            }
            .into(),
            Self::ForceReply {
                selective,
                one_time,
                placeholder,
            } => tl::types::ReplyKeyboardForceReply {
                selective: *selective,
                single_use: *one_time,
                placeholder: placeholder.clone(),
            }
            .into(),
            Self::Reply {
                rows,
                resize,
                one_time,
                selective,
                persistent,
                placeholder,
            } => {
                ensure!(
                    rows.len() <= 100
                        && rows.iter().all(|r| !r.is_empty() && r.len() <= 8)
                        && rows.iter().map(Vec::len).sum::<usize>() <= 100,
                    "reply keyboard too large or empty row"
                );
                let rows = rows
                    .iter()
                    .map(|row| {
                        let buttons = row
                            .iter()
                            .map(|key| {
                                ensure!(
                                    !key.text.is_empty() && key.text.encode_utf16().count() <= 64,
                                    "reply button label too long"
                                );
                                let kind = match &key.kind {
                                    ReplyKeyKind::RequestPeer {
                                        button_id,
                                        criteria,
                                        max_quantity,
                                        name_requested,
                                        username_requested,
                                        photo_requested,
                                    } => {
                                        ensure!(
                                            (1..=10).contains(max_quantity),
                                            "request peer max_quantity must be 1–10"
                                        );
                                        tl::types::InputButtonTypeRequestPeer {
                                            button_id: *button_id,
                                            peer_type: criteria.to_tl()?,
                                            max_quantity: *max_quantity,
                                            name_requested: *name_requested,
                                            username_requested: *username_requested,
                                            photo_requested: *photo_requested,
                                        }
                                        .into()
                                    }
                                    ReplyKeyKind::Text => tl::enums::ButtonType::Default,
                                    ReplyKeyKind::RequestContact => {
                                        tl::enums::ButtonType::RequestPhone
                                    }
                                    ReplyKeyKind::RequestLocation => {
                                        tl::enums::ButtonType::RequestGeoLocation
                                    }
                                    ReplyKeyKind::RequestPoll { quiz } => {
                                        tl::types::ButtonTypeRequestPoll { quiz: *quiz }.into()
                                    }
                                    ReplyKeyKind::WebApp { url } => {
                                        let parsed = reqwest::Url::parse(url)?;
                                        ensure!(
                                            url.len() <= 8192
                                                && parsed.scheme() == "https"
                                                && parsed.host_str().is_some(),
                                            "reply web app URL must use HTTPS"
                                        );
                                        tl::types::ButtonTypeSimpleWebView { url: url.clone() }
                                            .into()
                                    }
                                };
                                Ok(tl::types::KeyboardButton {
                                    style: key.appearance.to_tl()?,
                                    text: key.text.clone(),
                                    r#type: kind,
                                }
                                .into())
                            })
                            .collect::<Result<Vec<_>>>()?;
                        Ok(tl::types::KeyboardButtonRow { buttons }.into())
                    })
                    .collect::<Result<Vec<_>>>()?;
                tl::types::ReplyKeyboardMarkup {
                    resize: *resize,
                    single_use: *one_time,
                    selective: *selective,
                    persistent: *persistent,
                    force_reply: false,
                    rows,
                    placeholder: placeholder.clone(),
                }
                .into()
            }
        })
    }
    pub fn markup(&self) -> Result<ReplyMarkup> {
        Ok(ReplyMarkup { raw: self.to_tl()? })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn style_copy_and_callback_limits() {
        let key = Key {
            text: "Copy".into(),
            kind: Kind::Copy {
                text: "hello".into(),
            },
            style: Style::Success,
            emoji_id: Some(123),
        };
        let tl::enums::KeyboardInlineButton::Button(button) = key.button().unwrap().raw;
        let Some(tl::enums::KeyboardButtonStyle::Style(style)) = button.style else {
            panic!("style missing")
        };
        assert!(style.bg_success);
        assert_eq!(style.icon, Some(123));
        assert!(
            matches!(button.r#type,tl::enums::InlineButtonType::Copy(c) if c.copy_text=="hello")
        );
        let bad = Key {
            kind: Kind::Callback {
                data: "x".repeat(65),
            },
            ..key
        };
        assert!(bad.button().is_err());
    }
    #[test]
    fn rows_and_urls_are_validated() {
        let bad = Key {
            text: "bad".into(),
            kind: Kind::Url {
                url: "javascript:x".into(),
            },
            style: Style::Default,
            emoji_id: None,
        };
        assert!(bad.button().is_err());
        assert!(keyboard(&[vec![bad; 9]]).is_err());
    }
}
