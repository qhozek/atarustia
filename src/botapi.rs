//! Serializable bot-facing objects. Native MTProto originals remain available through Client.
pub use crate::inline_content::{GeoPoint, InputContent, VenueProvider};
pub use crate::moderation::ChatPermissions;
pub use crate::object_media::{
    Animation, Audio, Contact, Dice, Document, Location, PhotoSize, Poll, PollOption, Sticker,
    Venue, Video, VideoNote, Voice,
};
use anyhow::{Context, Result};
use grammers_client::{
    media::Media as NativeMedia, message::Message as NativeMessage, peer::Peer, tl,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct User {
    pub id: i64,
    pub is_bot: bool,
    pub first_name: String,
    pub last_name: Option<String>,
    pub username: Option<String>,
    #[serde(default)]
    pub language_code: Option<String>,
    #[serde(default)]
    pub is_premium: Option<bool>,
    #[serde(default)]
    pub added_to_attachment_menu: Option<bool>,
    #[serde(default)]
    pub can_join_groups: Option<bool>,
    #[serde(default)]
    pub can_read_all_group_messages: Option<bool>,
    #[serde(default)]
    pub supports_inline_queries: Option<bool>,
}
impl User {
    pub fn from_tl(user: &tl::enums::User) -> Option<Self> {
        let tl::enums::User::User(user) = user else {
            let tl::enums::User::Empty(user) = user else {
                unreachable!();
            };
            return Some(Self {
                id: user.id,
                ..Default::default()
            });
        };
        Some(Self {
            id: user.id,
            is_bot: user.bot,
            first_name: user.first_name.clone().unwrap_or_default(),
            last_name: user.last_name.clone(),
            username: user.username.clone().or_else(|| {
                user.usernames.as_ref().and_then(|v| {
                    v.iter().find_map(|u| {
                        let tl::enums::Username::Username(u) = u;
                        u.active.then(|| u.username.clone())
                    })
                })
            }),
            language_code: user.lang_code.clone(),
            is_premium: user.premium.then_some(true),
            added_to_attachment_menu: user.attach_menu_enabled.then_some(true),
            can_join_groups: user.bot.then_some(!user.bot_nochats),
            can_read_all_group_messages: user.bot.then_some(user.bot_chat_history),
            supports_inline_queries: user.bot.then_some(user.bot_inline_placeholder.is_some()),
        })
    }
    pub fn full_name(&self) -> String {
        std::iter::once(self.first_name.as_str())
            .chain(self.last_name.as_deref())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    }
    pub fn url(&self) -> String {
        format!("tg://user?id={}", self.id)
    }
    pub fn mention_html(&self, name: Option<&str>) -> String {
        let full = self.full_name();
        format!(
            "<a href=\"{}\">{}</a>",
            self.url(),
            crate::output::escape(name.filter(|n| !n.is_empty()).unwrap_or(&full))
        )
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemberStatus {
    Creator,
    Administrator,
    Member,
    Restricted,
    Left,
    Kicked,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChatMember {
    pub status: MemberStatus,
    pub user: User,
    /// Telegram member/admin restrictions and dates, shared with member-event conversion.
    pub details: serde_json::Value,
    #[serde(default)]
    pub until_date: Option<i32>,
    #[serde(default)]
    pub is_anonymous: Option<bool>,
    #[serde(default)]
    pub custom_title: Option<String>,
}
impl ChatMember {
    pub fn from_channel(user: User, participant: &tl::enums::ChannelParticipant) -> Result<Self> {
        use tl::enums::ChannelParticipant as P;
        let (id, status) = match participant {
            P::Participant(p) => (p.user_id, MemberStatus::Member),
            P::ParticipantSelf(p) => (p.user_id, MemberStatus::Member),
            P::Creator(p) => (p.user_id, MemberStatus::Creator),
            P::Admin(p) => (p.user_id, MemberStatus::Administrator),
            P::Banned(p) => {
                let tl::enums::Peer::User(peer) = &p.peer else {
                    anyhow::bail!("participant is not a user");
                };
                let tl::enums::ChatBannedRights::Rights(rights) = &p.banned_rights;
                (
                    peer.user_id,
                    if rights.view_messages {
                        MemberStatus::Kicked
                    } else {
                        MemberStatus::Restricted
                    },
                )
            }
            P::Left(p) => {
                let tl::enums::Peer::User(peer) = &p.peer else {
                    anyhow::bail!("participant is not a user");
                };
                (peer.user_id, MemberStatus::Left)
            }
        };
        anyhow::ensure!(
            user.id == id,
            "participant identity does not match requested user"
        );
        let details = crate::bot_events::participant(participant);
        Ok(Self {
            status,
            user,
            until_date: details
                .pointer("/restrictions/until_date")
                .and_then(serde_json::Value::as_i64)
                .and_then(|v| v.try_into().ok()),
            is_anonymous: details
                .pointer("/rights/is_anonymous")
                .and_then(serde_json::Value::as_bool),
            custom_title: details
                .get("rank")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned),
            details,
        })
    }
    pub fn from_group(user: User, participant: &tl::enums::ChatParticipant) -> Result<Self> {
        use tl::enums::ChatParticipant as P;
        let (id, status) = match participant {
            P::Participant(p) => (p.user_id, MemberStatus::Member),
            P::Creator(p) => (p.user_id, MemberStatus::Creator),
            P::Admin(p) => (p.user_id, MemberStatus::Administrator),
        };
        anyhow::ensure!(
            user.id == id,
            "participant identity does not match requested user"
        );
        let details = crate::bot_events::chat_participant(participant);
        Ok(Self {
            status,
            user,
            custom_title: details
                .get("rank")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned),
            until_date: None,
            is_anonymous: None,
            details,
        })
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Chat {
    pub id: i64,
    #[serde(rename = "type")]
    pub kind: String,
    pub title: Option<String>,
    pub username: Option<String>,
    #[serde(default)]
    pub first_name: Option<String>,
    #[serde(default)]
    pub last_name: Option<String>,
    #[serde(default)]
    pub is_forum: Option<bool>,
}
impl Chat {
    pub fn from_peer(peer: &Peer) -> Self {
        match peer {
            Peer::User(u) => Self::from_user(&User::from_tl(&u.raw).expect("user conversion")),
            Peer::Group(g) => Self::from_tl(&g.raw),
            Peer::Channel(c) => Self::from_tl(&c.raw.clone().into()),
            Peer::Community(c) => Self {
                id: c.id().bot_api_dialog_id_unchecked(),
                kind: "community".into(),
                title: Some(c.raw.title.clone()),
                ..Default::default()
            },
        }
    }
    pub fn full_name(&self) -> String {
        if self.kind == "private" {
            self.first_name
                .iter()
                .chain(self.last_name.iter())
                .filter(|s| !s.is_empty())
                .cloned()
                .collect::<Vec<_>>()
                .join(" ")
        } else {
            self.title.clone().unwrap_or_default()
        }
    }
    pub fn from_user(user: &User) -> Self {
        Self {
            id: user.id,
            kind: "private".into(),
            first_name: Some(user.first_name.clone()),
            last_name: user.last_name.clone(),
            username: user.username.clone(),
            ..Default::default()
        }
    }
    pub fn from_tl(chat: &tl::enums::Chat) -> Self {
        let summary = crate::story_payload::chat(chat);
        let (id, forum) = match chat {
            tl::enums::Chat::Empty(c) => (-c.id, None),
            tl::enums::Chat::Chat(c) => (-c.id, None),
            tl::enums::Chat::Forbidden(c) => (-c.id, None),
            tl::enums::Chat::Channel(c) => (
                grammers_session::types::PeerId::channel(c.id)
                    .map(|id| id.bot_api_dialog_id_unchecked())
                    .unwrap_or(0),
                c.forum.then_some(true),
            ),
            tl::enums::Chat::ChannelForbidden(c) => (
                grammers_session::types::PeerId::channel(c.id)
                    .map(|id| id.bot_api_dialog_id_unchecked())
                    .unwrap_or(0),
                None,
            ),
            _ => (0, None),
        };
        let mut result = Self {
            id,
            kind: summary["type"].as_str().unwrap_or("unknown").into(),
            title: summary["title"].as_str().map(str::to_owned),
            username: summary["username"].as_str().map(str::to_owned),
            is_forum: forum,
            ..Default::default()
        };
        if let tl::enums::Chat::Channel(c) = chat {
            result.username = result.username.or_else(|| {
                c.usernames.as_ref().and_then(|v| {
                    v.iter().find_map(|u| {
                        let tl::enums::Username::Username(u) = u;
                        u.active.then(|| u.username.clone())
                    })
                })
            });
        }
        result
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ChatFullInfo {
    pub chat: Chat,
    pub description: Option<String>,
    pub pinned_message_id: Option<i32>,
    pub message_auto_delete_time: Option<i32>,
    pub details: serde_json::Value,
    #[serde(default)]
    pub bio: Option<String>,
    #[serde(default)]
    pub invite_link: Option<String>,
    #[serde(default)]
    pub linked_chat_id: Option<i64>,
    #[serde(default)]
    pub has_protected_content: Option<bool>,
    #[serde(default)]
    pub accent_color_id: Option<i32>,
    #[serde(default)]
    pub max_reaction_count: Option<i32>,
}
impl ChatFullInfo {
    pub fn full_name(&self) -> String {
        self.chat.full_name()
    }
    pub fn promote_details(&mut self) {
        self.bio = if self.chat.kind == "private" {
            self.description.clone()
        } else {
            None
        };
        self.invite_link = self
            .details
            .get("invite_link")
            .and_then(|v| {
                v.as_str()
                    .or_else(|| v.get("invite_link").and_then(serde_json::Value::as_str))
            })
            .map(str::to_owned);
        self.linked_chat_id = self
            .details
            .get("linked_chat_id")
            .and_then(serde_json::Value::as_i64);
        self.has_protected_content = self
            .details
            .get("has_protected_content")
            .and_then(serde_json::Value::as_bool);
        self.max_reaction_count = self
            .details
            .get("available_reactions_limit")
            .and_then(serde_json::Value::as_i64)
            .and_then(|n| n.try_into().ok());
        self.accent_color_id = self
            .details
            .get("accent_color_id")
            .and_then(serde_json::Value::as_i64)
            .and_then(|n| n.try_into().ok());
    }
}
impl std::ops::Deref for ChatFullInfo {
    type Target = Chat;
    fn deref(&self) -> &Chat {
        &self.chat
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct File {
    pub file_id: String,
    pub file_unique_id: String,
    pub file_size: Option<u64>,
    pub file_path: String,
    pub file_name: Option<String>,
    pub mime_type: Option<String>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Message {
    pub message_id: i32,
    pub chat: Chat,
    pub from_user: Option<User>,
    pub text: Option<String>,
    pub caption: Option<String>,
    pub file: Option<File>,
    pub media_type: Option<crate::file_id::FileKind>,
    pub reply_to_message_id: Option<i32>,
    pub entities: Vec<MessageEntity>,
    pub caption_entities: Vec<MessageEntity>,
    /// Raw-message metadata shared with business events (media, dates, reply/forward IDs).
    #[serde(default)]
    pub details: serde_json::Value,
    #[serde(default)]
    pub message_thread_id: Option<i32>,
    #[serde(default)]
    pub sender_chat: Option<Chat>,
    #[serde(default)]
    pub date: Option<i32>,
    #[serde(default)]
    pub forward_origin: Option<serde_json::Value>,
    #[serde(default)]
    pub is_topic_message: Option<bool>,
    #[serde(default)]
    pub reply_to_message: Option<Box<Message>>,
    #[serde(default)]
    pub via_bot: Option<User>,
    #[serde(default)]
    pub edit_date: Option<i32>,
    #[serde(default)]
    pub media_group_id: Option<String>,
    #[serde(default)]
    pub author_signature: Option<String>,
    #[serde(default)]
    pub link_preview_options: Option<crate::bot_requests::Preview>,
    #[serde(default)]
    pub show_caption_above_media: Option<bool>,
    #[serde(default)]
    pub has_media_spoiler: Option<bool>,
    #[serde(flatten)]
    pub media: crate::object_media::Fields,
    #[serde(default)]
    pub new_chat_members: Vec<User>,
    #[serde(default)]
    pub left_chat_member: Option<User>,
    #[serde(default)]
    pub reply_markup: Option<serde_json::Value>,
    #[serde(default)]
    pub business_connection_id: Option<String>,
    #[serde(default)]
    pub guest_query_id: Option<String>,
    #[serde(default)]
    pub guest_bot_caller_user: Option<User>,
    #[serde(default)]
    pub guest_bot_caller_chat: Option<Chat>,
}
impl std::ops::Deref for Message {
    type Target = crate::object_media::Fields;
    fn deref(&self) -> &Self::Target {
        &self.media
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MessageEntity {
    #[serde(rename = "type")]
    pub kind: String,
    pub offset: i32,
    pub length: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_emoji_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unix_time: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<User>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date_time_format: Option<String>,
}
impl MessageEntity {
    pub fn to_tl(&self, text: &str) -> Result<tl::enums::MessageEntity> {
        anyhow::ensure!(self.length > 0, "entity length must be positive");
        self.extract(text)?;
        let offset = self.offset;
        let length = self.length;
        Ok(match self.kind.as_str() {
            "bold" => tl::types::MessageEntityBold { offset, length }.into(),
            "italic" => tl::types::MessageEntityItalic { offset, length }.into(),
            "underline" => tl::types::MessageEntityUnderline { offset, length }.into(),
            "strikethrough" => tl::types::MessageEntityStrike { offset, length }.into(),
            "spoiler" => tl::types::MessageEntitySpoiler { offset, length }.into(),
            "code" => tl::types::MessageEntityCode { offset, length }.into(),
            "pre" => tl::types::MessageEntityPre {
                offset,
                length,
                language: self.language.clone().unwrap_or_default(),
            }
            .into(),
            "text_link" => {
                let url = self.url.as_ref().context("text_link entity needs URL")?;
                let parsed = reqwest::Url::parse(url)?;
                anyhow::ensure!(
                    url.len() <= 8192
                        && matches!(parsed.scheme(), "https" | "http" | "tg" | "mailto"),
                    "invalid text link"
                );
                tl::types::MessageEntityTextUrl {
                    offset,
                    length,
                    url: url.clone(),
                }
                .into()
            }
            "text_mention" => {
                let id = self
                    .user_id
                    .or_else(|| self.user.as_ref().map(|u| u.id))
                    .context("text_mention needs user ID")?;
                anyhow::ensure!(
                    grammers_session::types::PeerId::user(id).is_some(),
                    "invalid mention user ID"
                );
                tl::types::MessageEntityMentionName {
                    offset,
                    length,
                    user_id: id,
                }
                .into()
            }
            "custom_emoji" => {
                let id = self
                    .custom_emoji_id
                    .as_ref()
                    .context("custom emoji needs document ID")?
                    .parse::<i64>()?;
                anyhow::ensure!(id > 0, "invalid custom emoji ID");
                tl::types::MessageEntityCustomEmoji {
                    offset,
                    length,
                    document_id: id,
                }
                .into()
            }
            "blockquote" | "expandable_blockquote" => tl::types::MessageEntityBlockquote {
                offset,
                length,
                collapsed: self.kind == "expandable_blockquote",
            }
            .into(),
            "mention" => tl::types::MessageEntityMention { offset, length }.into(),
            "hashtag" => tl::types::MessageEntityHashtag { offset, length }.into(),
            "cashtag" => tl::types::MessageEntityCashtag { offset, length }.into(),
            "bot_command" => tl::types::MessageEntityBotCommand { offset, length }.into(),
            "url" => tl::types::MessageEntityUrl { offset, length }.into(),
            "email" => tl::types::MessageEntityEmail { offset, length }.into(),
            "phone_number" => tl::types::MessageEntityPhone { offset, length }.into(),
            "bank_card" => tl::types::MessageEntityBankCard { offset, length }.into(),
            "ton_address" => tl::types::MessageEntityTonAddress { offset, length }.into(),
            "unknown" => tl::types::MessageEntityUnknown { offset, length }.into(),
            "diff_insert" => tl::types::MessageEntityDiffInsert { offset, length }.into(),
            "diff_delete" => tl::types::MessageEntityDiffDelete { offset, length }.into(),
            "diff_replace" => tl::types::MessageEntityDiffReplace {
                offset,
                length,
                old_text: self
                    .extra
                    .as_ref()
                    .and_then(|e| e.get("old_text"))
                    .and_then(serde_json::Value::as_str)
                    .context("diff_replace needs old_text")?
                    .into(),
            }
            .into(),
            "date_time" => {
                let extra = |name: &str| {
                    self.extra
                        .as_ref()
                        .and_then(|e| e.get(name))
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false)
                };
                let format = self.date_time_format.as_deref();
                let flag = |name: &str, letter: char| {
                    format
                        .map(|f| f.contains(letter))
                        .unwrap_or_else(|| extra(name))
                };
                tl::types::MessageEntityFormattedDate {
                    offset,
                    length,
                    date: self.unix_time.context("date_time needs Unix time")?,
                    relative: flag("relative", 'r'),
                    short_time: flag("short_time", 't'),
                    long_time: flag("long_time", 'T'),
                    short_date: flag("short_date", 'd'),
                    long_date: flag("long_date", 'D'),
                    day_of_week: flag("day_of_week", 'w'),
                }
                .into()
            }
            _ => anyhow::bail!("unsupported entity type"),
        })
    }
    pub fn from_tl(entity: &tl::enums::MessageEntity) -> Option<Self> {
        use tl::enums::MessageEntity as E;
        let (kind, offset, length, url, language, emoji) = match entity {
            E::Bold(e) => ("bold", e.offset, e.length, None, None, None),
            E::Italic(e) => ("italic", e.offset, e.length, None, None, None),
            E::Underline(e) => ("underline", e.offset, e.length, None, None, None),
            E::Strike(e) => ("strikethrough", e.offset, e.length, None, None, None),
            E::Spoiler(e) => ("spoiler", e.offset, e.length, None, None, None),
            E::Code(e) => ("code", e.offset, e.length, None, None, None),
            E::Pre(e) => (
                "pre",
                e.offset,
                e.length,
                None,
                Some(e.language.clone()),
                None,
            ),
            E::TextUrl(e) => (
                "text_link",
                e.offset,
                e.length,
                Some(e.url.clone()),
                None,
                None,
            ),
            E::Url(e) => ("url", e.offset, e.length, None, None, None),
            E::Mention(e) => ("mention", e.offset, e.length, None, None, None),
            E::Hashtag(e) => ("hashtag", e.offset, e.length, None, None, None),
            E::BotCommand(e) => ("bot_command", e.offset, e.length, None, None, None),
            E::Email(e) => ("email", e.offset, e.length, None, None, None),
            E::Phone(e) => ("phone_number", e.offset, e.length, None, None, None),
            E::Cashtag(e) => ("cashtag", e.offset, e.length, None, None, None),
            E::BankCard(e) => ("bank_card", e.offset, e.length, None, None, None),
            E::MentionName(e) => ("text_mention", e.offset, e.length, None, None, None),
            E::FormattedDate(e) => ("date_time", e.offset, e.length, None, None, None),
            E::DiffInsert(e) => ("diff_insert", e.offset, e.length, None, None, None),
            E::DiffReplace(e) => ("diff_replace", e.offset, e.length, None, None, None),
            E::DiffDelete(e) => ("diff_delete", e.offset, e.length, None, None, None),
            E::TonAddress(e) => ("ton_address", e.offset, e.length, None, None, None),
            E::Unknown(e) => ("unknown", e.offset, e.length, None, None, None),
            E::InputMessageEntityMentionName(e) => {
                ("text_mention", e.offset, e.length, None, None, None)
            }
            E::Blockquote(e) => (
                if e.collapsed {
                    "expandable_blockquote"
                } else {
                    "blockquote"
                },
                e.offset,
                e.length,
                None,
                None,
                None,
            ),
            E::CustomEmoji(e) => (
                "custom_emoji",
                e.offset,
                e.length,
                None,
                None,
                Some(e.document_id.to_string()),
            ),
        };
        Some(Self {
            kind: kind.into(),
            offset,
            length,
            url,
            language,
            custom_emoji_id: emoji,
            user_id: match entity {
                E::MentionName(e) => Some(e.user_id),
                E::InputMessageEntityMentionName(e) => {
                    if let tl::enums::InputUser::User(u) = &e.user_id {
                        Some(u.user_id)
                    } else {
                        None
                    }
                }
                _ => None,
            },
            unix_time: if let E::FormattedDate(e) = entity {
                Some(e.date)
            } else {
                None
            },
            extra: match entity {
                E::DiffReplace(e) => Some(serde_json::json!({"old_text":e.old_text})),
                E::FormattedDate(e) => Some(
                    serde_json::json!({"relative":e.relative,"short_time":e.short_time,"long_time":e.long_time,"short_date":e.short_date,"long_date":e.long_date,"day_of_week":e.day_of_week}),
                ),
                _ => None,
            },
            user: match entity {
                E::MentionName(e) => Some(User {
                    id: e.user_id,
                    ..Default::default()
                }),
                _ => None,
            },
            date_time_format: if let E::FormattedDate(e) = entity {
                if e.relative {
                    Some("r".into())
                } else {
                    let format = [
                        (e.day_of_week, 'w'),
                        (e.long_date, 'D'),
                        (e.short_date && !e.long_date, 'd'),
                        (e.long_time, 'T'),
                        (e.short_time && !e.long_time, 't'),
                    ]
                    .into_iter()
                    .filter_map(|(enabled, c)| enabled.then_some(c))
                    .collect::<String>();
                    (!format.is_empty()).then_some(format)
                }
            } else {
                None
            },
        })
    }
    pub fn extract(&self, text: &str) -> Result<String> {
        let units: Vec<_> = text.encode_utf16().collect();
        let start = usize::try_from(self.offset)?;
        let length = usize::try_from(self.length)?;
        let end = start.checked_add(length).context("entity range overflow")?;
        let slice = units.get(start..end).context("entity outside text")?;
        Ok(String::from_utf16(slice)?)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CallbackQuery {
    pub id: String,
    pub from_user: User,
    pub data: Option<String>,
    pub inline_message_id: Option<String>,
    pub message_id: Option<i32>,
    pub chat_id: Option<i64>,
    #[serde(default)]
    pub data_base64: Option<String>,
    #[serde(default)]
    pub chat_instance: Option<String>,
    #[serde(default)]
    pub game_short_name: Option<String>,
    #[serde(default)]
    pub message: Option<Box<Message>>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InlineQuery {
    pub id: String,
    pub from_user: User,
    pub query: String,
    pub offset: String,
    pub chat_type: Option<String>,
    pub location: Option<Location>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChosenInlineResult {
    pub result_id: String,
    pub from_user: User,
    pub location: Option<Location>,
    pub inline_message_id: Option<String>,
    pub query: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SentWebAppMessage {
    pub inline_message_id: Option<String>,
}
impl SentWebAppMessage {
    pub fn from_tl(sent: &tl::enums::WebViewMessageSent) -> Self {
        let tl::enums::WebViewMessageSent::Sent(sent) = sent;
        Self {
            inline_message_id: sent.msg_id.as_ref().map(crate::file_id::encode_inline_id),
        }
    }
}
pub fn web_app_request(
    query_id: &str,
    result: &InlineResult,
) -> Result<tl::functions::messages::SendWebViewResultMessage> {
    anyhow::ensure!(
        !query_id.is_empty() && query_id.len() <= 4096 && !query_id.chars().any(char::is_control),
        "web app query ID must contain 1–4096 bytes without control characters"
    );
    Ok(tl::functions::messages::SendWebViewResultMessage {
        bot_query_id: query_id.into(),
        result: result.to_tl()?,
    })
}
impl File {
    pub fn from_media(media: &NativeMedia) -> Result<Self> {
        let (reference, file_size, file_name, mime_type) =
            crate::file_id::FileRef::from_media(media)?;
        let file_id = reference.encode()?;
        Ok(Self {
            file_unique_id: reference.unique_id(),
            file_path: file_id.clone(),
            file_id,
            file_size,
            file_name,
            mime_type,
        })
    }
}
impl Message {
    pub fn from_native(message: &NativeMessage) -> Self {
        let peer = message.peer();
        let mut chat = Chat {
            id: message.peer_id().bot_api_dialog_id_unchecked(),
            kind: match peer {
                Some(Peer::User(_)) => "private",
                Some(Peer::Group(_)) => {
                    if message.peer_id().kind() == grammers_session::types::PeerKind::Channel {
                        "supergroup"
                    } else {
                        "group"
                    }
                }
                Some(Peer::Channel(_)) => "channel",
                _ => "unknown",
            }
            .into(),
            title: peer.and_then(|p| p.name()).map(str::to_owned),
            username: peer.and_then(|p| p.username()).map(str::to_owned),
            ..Default::default()
        };
        if let Some(peer) = peer {
            chat = match peer {
                Peer::User(u) => Chat::from_user(&User::from_tl(&u.raw).expect("user variant")),
                Peer::Group(g) => Chat::from_tl(&g.raw),
                Peer::Channel(c) => Chat::from_tl(&c.raw.clone().into()),
                Peer::Community(_) => chat,
            };
        }
        let from_user = message.sender().and_then(|p| {
            if let Peer::User(user) = p {
                User::from_tl(&user.raw)
            } else {
                None
            }
        });
        let media = message.media();
        let file = media.as_ref().and_then(|m| File::from_media(m).ok());
        let media_type = file
            .as_ref()
            .and_then(|f| crate::file_id::FileRef::decode(&f.file_id).ok())
            .map(|f| f.kind);
        // Web-page previews do not turn a text message into a media caption.
        let caption = matches!(
            media,
            Some(NativeMedia::Photo(_) | NativeMedia::Document(_) | NativeMedia::Sticker(_))
        ) || matches!(&message.raw,tl::enums::Message::Message(m) if matches!(m.media,Some(tl::enums::MessageMedia::PaidMedia(_))));
        let text = (!message.text().is_empty()).then(|| message.text().to_owned());
        let mut result = Self {
            message_id: message.id(),
            chat,
            from_user,
            text: if caption { None } else { text.clone() },
            caption: if caption { text } else { None },
            file,
            media_type,
            reply_to_message_id: message.reply_to_message_id(),
            entities: if !caption {
                message
                    .fmt_entities()
                    .map(|e| e.iter().filter_map(MessageEntity::from_tl).collect())
                    .unwrap_or_default()
            } else {
                vec![]
            },
            caption_entities: if caption {
                message
                    .fmt_entities()
                    .map(|e| e.iter().filter_map(MessageEntity::from_tl).collect())
                    .unwrap_or_default()
            } else {
                vec![]
            },
            details: crate::message_payload::fields(&message.raw),
            ..Default::default()
        };
        result.enrich(&message.raw);
        if let tl::enums::Message::Message(m) = &message.raw {
            if let Some(sender) = message.sender()
                && !matches!(sender, Peer::User(_))
            {
                result.sender_chat = Some(Chat {
                    id: sender.id().bot_api_dialog_id_unchecked(),
                    kind: if matches!(sender, Peer::Channel(_)) {
                        "channel"
                    } else {
                        "group"
                    }
                    .into(),
                    title: sender.name().map(str::to_owned),
                    username: sender.username().map(str::to_owned),
                    ..Default::default()
                });
            }
            if m.from_id.is_none() && m.post {
                result.sender_chat = Some(result.chat.clone());
            }
        }
        result
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    BadRequest,
    Forbidden,
    Unauthorized,
    RetryAfter,
    Network,
    Server,
    Other,
    NotFound,
    InvalidResponse,
    LocalIo,
}
pub type ApiResult<T> = std::result::Result<T, ApiError>;
pub use crate::bot_response::{EditResult, MessageId, SentGuestMessage, SentMessage};
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct BotCommand {
    pub command: String,
    pub description: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApiError {
    pub method: String,
    pub kind: ErrorKind,
    pub description: String,
    pub retry_after: Option<u32>,
}
impl ApiError {
    /// Normalize at the outermost public method, retaining typed causes through anyhow contexts.
    pub fn from_error(method: &str, error: anyhow::Error) -> Self {
        // anyhow typed context is downcastable through Error, but is not always a chain element.
        if let Some(api) = error.downcast_ref::<Self>() {
            let mut api = api.clone();
            api.method = method.into();
            return api;
        }
        if let Some(rpc) = error.downcast_ref::<grammers_client::InvocationError>() {
            return Self::from_rpc(method, rpc);
        }
        if let Some(response) = error.downcast_ref::<crate::bot_response::InvalidResponse>() {
            return Self {
                method: method.into(),
                kind: ErrorKind::InvalidResponse,
                description: format!("Invalid Response: {response}"),
                retry_after: None,
            };
        }
        for cause in error.chain() {
            if let Some(api) = cause.downcast_ref::<Self>() {
                let mut api = api.clone();
                api.method = method.into();
                return api;
            }
            if let Some(rpc) = cause.downcast_ref::<grammers_client::InvocationError>() {
                return Self::from_rpc(method, rpc);
            }
            if cause.is::<crate::bot_response::InvalidResponse>() {
                return Self {
                    method: method.into(),
                    kind: ErrorKind::InvalidResponse,
                    description: format!("Invalid Response: {cause}"),
                    retry_after: None,
                };
            }
            if cause.is::<tokio::time::error::Elapsed>() {
                return Self {
                    method: method.into(),
                    kind: ErrorKind::Network,
                    description: "Network: operation timed out".into(),
                    retry_after: None,
                };
            }
            if let Some(io) = cause.downcast_ref::<std::io::Error>() {
                return Self {
                    method: method.into(),
                    kind: match io.kind() {
                        std::io::ErrorKind::TimedOut
                        | std::io::ErrorKind::ConnectionRefused
                        | std::io::ErrorKind::ConnectionReset
                        | std::io::ErrorKind::ConnectionAborted
                        | std::io::ErrorKind::NotConnected
                        | std::io::ErrorKind::BrokenPipe
                        | std::io::ErrorKind::UnexpectedEof => ErrorKind::Network,
                        _ => ErrorKind::LocalIo,
                    },
                    description: format!("I/O: {io}"),
                    retry_after: None,
                };
            }
        }
        Self::validation(method, error.to_string())
    }
    pub fn validation(method: &str, description: String) -> Self {
        Self {
            method: method.into(),
            kind: ErrorKind::BadRequest,
            description: format!("Bad Request: {description}"),
            retry_after: None,
        }
    }
    pub fn from_rpc(method: &str, error: &grammers_client::InvocationError) -> Self {
        use grammers_client::InvocationError;
        let (kind, retry_after) = match error {
            InvocationError::Rpc(r)
                if matches!(
                    r.name.as_str(),
                    "USER_IS_BLOCKED"
                        | "USER_DEACTIVATED"
                        | "INPUT_USER_DEACTIVATED"
                        | "CHAT_WRITE_FORBIDDEN"
                        | "USER_BOT_INVALID"
                ) =>
            {
                (ErrorKind::Forbidden, None)
            }
            InvocationError::Rpc(r) => match r.code {
                400 | 406 => (ErrorKind::BadRequest, None),
                401 => (ErrorKind::Unauthorized, None),
                403 => (ErrorKind::Forbidden, None),
                404 => (ErrorKind::NotFound, None),
                420 | 429 => (ErrorKind::RetryAfter, Some(r.value.unwrap_or(1).max(1))),
                500..=599 => (ErrorKind::Server, None),
                _ => (ErrorKind::Other, None),
            },
            InvocationError::Io(_) => (ErrorKind::Network, None),
            InvocationError::Dropped(_) | InvocationError::Transport(_) => {
                (ErrorKind::Network, None)
            }
            InvocationError::Deserialize(_) => (ErrorKind::InvalidResponse, None),
            InvocationError::InvalidDc => (ErrorKind::BadRequest, None),
            InvocationError::Session(_) => (ErrorKind::LocalIo, None),
            _ => (ErrorKind::Other, None),
        };
        let description = if let InvocationError::Rpc(r) = error {
            let detail = match r.name.as_str() {
                "MESSAGE_NOT_MODIFIED" => "message is not modified",
                "MESSAGE_ID_INVALID" => "message to edit not found",
                "MESSAGE_EDIT_TIME_EXPIRED" | "MESSAGE_AUTHOR_REQUIRED" => {
                    "message can't be edited"
                }
                "MESSAGE_DELETE_FORBIDDEN" => "message can't be deleted",
                "MESSAGE_EMPTY" => "message text is empty",
                "MESSAGE_TOO_LONG" => "message is too long",
                "MEDIA_CAPTION_TOO_LONG" => "message caption is too long",
                "PEER_ID_INVALID" | "CHANNEL_INVALID" | "CHAT_ID_INVALID" | "CHANNEL_PRIVATE" => {
                    "chat not found"
                }
                "USER_ID_INVALID" => "user not found",
                "QUERY_ID_INVALID" => "query is too old or response timeout expired",
                "ENTITY_BOUNDS_INVALID" => "can't parse entities",
                "MEDIA_EMPTY" | "FILE_REFERENCE_EXPIRED" => {
                    "wrong file identifier/HTTP URL specified"
                }
                "WEBPAGE_CURL_FAILED" => "failed to get HTTP URL content",
                "WEBPAGE_MEDIA_EMPTY" => "wrong type of web page content",
                "USER_IS_BLOCKED" => "bot was blocked by the user",
                "USER_DEACTIVATED" | "INPUT_USER_DEACTIVATED" => "user is deactivated",
                "CHAT_WRITE_FORBIDDEN" => "bot is not a member of the chat",
                "CHAT_ADMIN_REQUIRED" | "USER_ADMIN_INVALID" => "not enough rights",
                "MESSAGE_IDS_EMPTY" => "message identifier is not specified",
                _ => r.name.as_str(),
            };
            let prefix = match kind {
                ErrorKind::BadRequest => "Bad Request",
                ErrorKind::Forbidden => "Forbidden",
                ErrorKind::Unauthorized => "Unauthorized",
                ErrorKind::NotFound => "Not Found",
                ErrorKind::RetryAfter => "Too Many Requests",
                ErrorKind::Server => "Internal Server Error",
                _ => "Error",
            };
            if let Some(seconds) = retry_after {
                format!("{prefix}: retry after {seconds}")
            } else {
                format!("{prefix}: {detail}")
            }
        } else {
            error.to_string()
        };
        Self {
            method: method.into(),
            kind,
            description,
            retry_after,
        }
    }
}
impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.method, self.description)
    }
}
impl std::error::Error for ApiError {}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum InlineResult {
    Game {
        id: String,
        short_name: String,
        #[serde(default)]
        keyboard: Vec<Vec<crate::markup::Key>>,
    },
    Typed {
        result: Box<crate::bot_inline::ResultSpec>,
    },
    Article {
        id: String,
        title: String,
        text: String,
        #[serde(default)]
        html: bool,
    },
    Media {
        id: String,
        title: String,
        caption: String,
        media: crate::media::Media,
        #[serde(default)]
        html: bool,
    },
    /// Explicit input_message_content with optional cached/URL media and keyboard.
    Content {
        id: String,
        title: String,
        #[serde(default)]
        description: Option<String>,
        #[serde(default)]
        url: Option<String>,
        #[serde(default)]
        thumbnail_url: Option<String>,
        #[serde(default)]
        media: Option<Box<crate::media::Media>>,
        content: InputContent,
        #[serde(default)]
        keyboard: Vec<Vec<crate::markup::Key>>,
    },
}
impl InlineResult {
    pub fn id(&self) -> &str {
        match self {
            Self::Game { id, .. } => id,
            Self::Typed { result } => &result.id,
            Self::Article { id, .. } | Self::Media { id, .. } | Self::Content { id, .. } => id,
        }
    }
    pub fn to_tl(&self) -> Result<tl::enums::InputBotInlineResult> {
        match self {
            Self::Game {
                id,
                short_name,
                keyboard,
            } => crate::bot_inline::game(id, short_name, keyboard),
            Self::Typed { result } => result.to_tl(),
            Self::Article {
                id,
                title,
                text,
                html,
            } => crate::media::inline_result(
                id,
                title,
                if *html {
                    grammers_client::message::InputMessage::new().html(text)
                } else {
                    grammers_client::message::InputMessage::new().text(text)
                },
                None,
                None,
            ),
            Self::Media {
                id,
                title,
                caption,
                media,
                html,
            } => crate::media::inline_result(
                id,
                title,
                if *html {
                    grammers_client::message::InputMessage::new().html(caption)
                } else {
                    grammers_client::message::InputMessage::new().text(caption)
                },
                Some(media),
                None,
            ),
            Self::Content {
                id,
                title,
                description,
                url,
                thumbnail_url,
                media,
                content,
                keyboard,
            } => crate::inline_content::result(
                id,
                title,
                description.as_deref(),
                url.as_deref(),
                thumbnail_url.as_deref(),
                media.as_deref(),
                content,
                keyboard,
            ),
        }
    }
}
pub fn inline_results(results: &[InlineResult]) -> Result<Vec<tl::enums::InputBotInlineResult>> {
    anyhow::ensure!(results.len() <= 50, "inline answer exceeds 50 results");
    let mut ids = std::collections::HashSet::new();
    for r in results {
        let id = r.id();
        anyhow::ensure!(ids.insert(id), "duplicate inline result ID");
    }
    results.iter().map(InlineResult::to_tl).collect()
}
pub fn callback_from_native(
    query: &grammers_client::update::CallbackQuery,
) -> Result<CallbackQuery> {
    let id = query.sender_id().bare_id_unchecked();
    let mut from_user = User {
        id,
        is_bot: false,
        first_name: query
            .sender()
            .and_then(|p| p.name())
            .unwrap_or_default()
            .into(),
        last_name: None,
        username: query.sender().and_then(|p| p.username()).map(str::to_owned),
        ..Default::default()
    };
    if let Some(Peer::User(user)) = query.sender() {
        from_user = User::from_tl(&user.raw).expect("user conversion");
    }
    let (query_id, inline_message_id, message_id, chat_id) = match &query.raw {
        tl::enums::Update::BotCallbackQuery(q) => (
            q.query_id,
            None,
            Some(q.msg_id),
            Some(query.peer_id().bot_api_dialog_id_unchecked()),
        ),
        tl::enums::Update::InlineBotCallbackQuery(q) => (
            q.query_id,
            Some(crate::file_id::encode_inline_id(&q.msg_id)),
            None,
            None,
        ),
        _ => anyhow::bail!("not a callback update"),
    };
    let (raw_data, chat_instance, game_short_name) = match &query.raw {
        tl::enums::Update::BotCallbackQuery(q) => (
            q.data.as_ref(),
            Some(q.chat_instance.to_string()),
            q.game_short_name.clone(),
        ),
        tl::enums::Update::InlineBotCallbackQuery(q) => (
            q.data.as_ref(),
            Some(q.chat_instance.to_string()),
            q.game_short_name.clone(),
        ),
        _ => unreachable!("checked callback update"),
    };
    let data = raw_data
        .and_then(|d| std::str::from_utf8(d).ok())
        .map(str::to_owned);
    use base64::Engine as _;
    let data_base64 = raw_data.map(|d| base64::engine::general_purpose::STANDARD.encode(d));
    Ok(CallbackQuery {
        id: query_id.to_string(),
        from_user,
        data,
        inline_message_id,
        message_id,
        chat_id,
        data_base64,
        chat_instance,
        game_short_name,
        message: match (chat_id, message_id) {
            (Some(chat_id), Some(message_id)) => Some(Box::new(Message {
                message_id,
                chat: query.peer().map(Chat::from_peer).unwrap_or(Chat {
                    id: chat_id,
                    kind: "unknown".into(),
                    ..Default::default()
                }),
                ..Default::default()
            })),
            _ => None,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn entities_use_utf16_not_utf8() {
        let entity = MessageEntity {
            kind: "bold".into(),
            offset: 2,
            length: 6,
            url: None,
            language: None,
            custom_emoji_id: None,
            user_id: None,
            unix_time: None,
            extra: None,
            user: None,
            date_time_format: None,
        };
        assert_eq!(entity.extract("🦀hello!").unwrap(), "hello!");
        let mut invalid = entity.clone();
        invalid.offset = 1;
        invalid.length = 1;
        assert!(invalid.extract("🦀hello!").is_err());
        invalid.offset = -1;
        assert!(invalid.extract("text").is_err());
    }
    #[test]
    fn result_validation_and_serialization() {
        let result = InlineResult::Article {
            id: "one".into(),
            title: "test".into(),
            text: "<b>hi</b>".into(),
            html: true,
        };
        assert!(result.to_tl().is_ok());
        assert!(inline_results(&[result.clone(), result.clone()]).is_err());
        let round: InlineResult =
            serde_json::from_str(&serde_json::to_string(&result).unwrap()).unwrap();
        assert!(round.to_tl().is_ok());
    }
}
