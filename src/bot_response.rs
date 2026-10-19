//! Correlate actual MTProto response messages. Never fabricate success when an update is missing.
use anyhow::{Context, Result, ensure};
use grammers_client::tl;
use grammers_session::types::PeerRef;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug)]
pub struct InvalidResponse(pub &'static str);
impl std::fmt::Display for InvalidResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for InvalidResponse {}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SentMessage {
    pub message_id: i32,
    pub chat_id: i64,
    pub data: Value,
}
impl SentMessage {
    pub fn from_native(message: &grammers_client::message::Message) -> Result<Self> {
        ensure!(
            message.id() > 0,
            InvalidResponse("Telegram returned an empty or invalid message")
        );
        let mut result = Self {
            message_id: message.id(),
            chat_id: message.peer_id().bot_api_dialog_id_unchecked(),
            data: crate::message_payload::fields(&message.raw),
        };
        result.data["object"] = serde_json::to_value(crate::botapi::Message::from_native(message))?;
        result.validate()?;
        Ok(result)
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.message_id > 0
                && grammers_session::types::PeerId::from_bot_api_dialog_id(self.chat_id).is_some(),
            InvalidResponse("invalid sent-message identity")
        );
        ensure!(
            self.data.get("message_id").and_then(Value::as_i64) == Some(i64::from(self.message_id))
                && self.data.get("chat_id").and_then(Value::as_i64) == Some(self.chat_id),
            InvalidResponse("sent-message body identity mismatch")
        );
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MessageId {
    pub message_id: i32,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EditResult {
    Message { message: SentMessage },
    Acknowledged { chat_id: i64, message_id: i32 },
    Inline { inline_message_id: String },
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SentGuestMessage {
    pub inline_message_id: String,
}
fn message(raw: &tl::enums::Message) -> Option<SentMessage> {
    let data = crate::message_payload::fields(raw);
    Some(SentMessage {
        message_id: data.get("message_id")?.as_i64()?.try_into().ok()?,
        chat_id: data.get("chat_id")?.as_i64()?,
        data,
    })
}
fn updates(result: &tl::enums::Updates) -> &[tl::enums::Update] {
    match result {
        tl::enums::Updates::Updates(r) => &r.updates,
        tl::enums::Updates::Combined(r) => &r.updates,
        _ => &[],
    }
}
fn raw_message(update: &tl::enums::Update) -> Option<&tl::enums::Message> {
    match update {
        tl::enums::Update::NewMessage(u) => Some(&u.message),
        tl::enums::Update::NewChannelMessage(u) => Some(&u.message),
        tl::enums::Update::EditMessage(u) => Some(&u.message),
        tl::enums::Update::EditChannelMessage(u) => Some(&u.message),
        tl::enums::Update::BotNewBusinessMessage(u) => Some(&u.message),
        tl::enums::Update::BotEditBusinessMessage(u) => Some(&u.message),
        _ => None,
    }
}
fn enrich_object(message: &mut SentMessage, result: &tl::enums::Updates) -> Result<()> {
    let (users, chats) = match result {
        tl::enums::Updates::Updates(r) => (r.users.as_slice(), r.chats.as_slice()),
        tl::enums::Updates::Combined(r) => (r.users.as_slice(), r.chats.as_slice()),
        _ => ([].as_slice(), [].as_slice()),
    };
    let user = |id: i64| {
        users
            .iter()
            .find_map(|u| crate::botapi::User::from_tl(u).filter(|u| u.id == id))
    };
    let chat = if message.chat_id > 0 {
        crate::botapi::Chat::from_user(&user(message.chat_id).unwrap_or(crate::botapi::User {
            id: message.chat_id,
            ..Default::default()
        }))
    } else {
        chats
            .iter()
            .map(crate::botapi::Chat::from_tl)
            .find(|c| c.id == message.chat_id)
            .unwrap_or(crate::botapi::Chat {
                id: message.chat_id,
                kind: if message.chat_id <= -1000000000000 {
                    "supergroup"
                } else {
                    "group"
                }
                .into(),
                ..Default::default()
            })
    };
    let list = if let tl::enums::Updates::UpdateShort(r) = result {
        std::slice::from_ref(&r.update)
    } else {
        updates(result)
    };
    let raw = list.iter().filter_map(raw_message).find(|raw| match raw {
        tl::enums::Message::Message(m) => {
            m.id == message.message_id
                && grammers_session::types::PeerId::from(m.peer_id.clone())
                    .bot_api_dialog_id_unchecked()
                    == message.chat_id
        }
        tl::enums::Message::Service(m) => {
            m.id == message.message_id
                && grammers_session::types::PeerId::from(m.peer_id.clone())
                    .bot_api_dialog_id_unchecked()
                    == message.chat_id
        }
        _ => false,
    });
    let mut object = if let Some(raw) = raw {
        let from = match raw {
            tl::enums::Message::Message(m) => m.from_id.as_ref(),
            tl::enums::Message::Service(m) => m.from_id.as_ref(),
            _ => None,
        };
        let from = from.and_then(|p| {
            if let tl::enums::Peer::User(u) = p {
                user(u.user_id)
            } else {
                None
            }
        });
        crate::botapi::Message::from_tl(raw, chat, from)
    } else {
        let mut object = message.object().map_err(anyhow::Error::new)?;
        object.chat = chat;
        if let tl::enums::Updates::UpdateShortSentMessage(r) = result {
            object.media = crate::object_media::Fields::from_tl(r.media.as_ref());
            object.reply_markup = message
                .data
                .get("reply_markup")
                .filter(|v| !v.is_null())
                .cloned();
        }
        object
    };
    if let Some(from) = &object.from_user {
        object.from_user = user(from.id).or_else(|| object.from_user.clone());
    }
    if let Some(via) = &object.via_bot {
        object.via_bot = user(via.id).or_else(|| object.via_bot.clone());
    }
    for entity in object
        .entities
        .iter_mut()
        .chain(object.caption_entities.iter_mut())
    {
        if let Some(id) = entity.user_id {
            entity.user = user(id).or_else(|| entity.user.clone());
        }
    }
    object.new_chat_members = object
        .new_chat_members
        .into_iter()
        .map(|u| user(u.id).unwrap_or(u))
        .collect();
    if let Some(left) = object.left_chat_member.take() {
        object.left_chat_member = Some(user(left.id).unwrap_or(left));
    }
    message.data["object"] = serde_json::to_value(object)?;
    Ok(())
}
pub fn sent(
    result: &tl::enums::Updates,
    peer: PeerRef,
    random_id: i64,
    source_text: &str,
    markup: Option<&tl::enums::ReplyMarkup>,
) -> Result<SentMessage> {
    sent_inner(result, peer, random_id, source_text, markup)
        .and_then(|mut message| {
            enrich_object(&mut message, result)?;
            Ok(message)
        })
        .map_err(|error| error.context(InvalidResponse("invalid Telegram send response")))
}
fn sent_inner(
    result: &tl::enums::Updates,
    peer: PeerRef,
    random_id: i64,
    source_text: &str,
    markup: Option<&tl::enums::ReplyMarkup>,
) -> Result<SentMessage> {
    if let tl::enums::Updates::UpdateShortSentMessage(r) = result {
        ensure!(r.id > 0, "Telegram returned an invalid message ID");
        let chat = peer.id.bot_api_dialog_id_unchecked();
        let is_caption = matches!(
            r.media,
            Some(
                tl::enums::MessageMedia::Photo(_)
                    | tl::enums::MessageMedia::Document(_)
                    | tl::enums::MessageMedia::PaidMedia(_)
            )
        );
        let mut data = json!({"message_id":r.id,"chat_id":chat,"date":r.date,"outgoing":r.out,"media":r.media.as_ref().map(crate::message_payload::media),"reply_markup":markup.map(crate::keyboard_payload::from_tl),"ttl_period":r.ttl_period});
        data[if is_caption { "caption" } else { "text" }] = json!(source_text);
        data[if is_caption {
            "caption_entities"
        } else {
            "entities"
        }] = json!(
            r.entities
                .as_ref()
                .map(|v| v
                    .iter()
                    .filter_map(crate::botapi::MessageEntity::from_tl)
                    .collect::<Vec<_>>())
                .unwrap_or_default()
        );
        return Ok(SentMessage {
            message_id: r.id,
            chat_id: chat,
            data,
        });
    }
    let list = if let tl::enums::Updates::UpdateShort(r) = result {
        std::slice::from_ref(&r.update)
    } else {
        updates(result)
    };
    let id = list.iter().find_map(|u| {
        if let tl::enums::Update::MessageId(u) = u {
            if u.random_id == random_id {
                Some(u.id)
            } else {
                None
            }
        } else {
            None
        }
    });
    ensure!(
        id.is_some()
            || !list
                .iter()
                .any(|u| matches!(u, tl::enums::Update::MessageId(_))),
        "Telegram acknowledged a different random ID"
    );
    let chat = peer.id.bot_api_dialog_id_unchecked();
    let candidates = list
        .iter()
        .filter_map(raw_message)
        .filter_map(message)
        .filter(|m| m.chat_id == chat)
        .collect::<Vec<_>>();
    if let Some(id) = id {
        return candidates
            .into_iter()
            .find(|m| m.message_id == id)
            .context("sent message ID was acknowledged but its body is absent");
    }
    ensure!(
        candidates.len() == 1,
        "Telegram response has no uniquely correlated sent message"
    );
    Ok(candidates.into_iter().next().unwrap())
}
pub fn edited(result: &tl::enums::Updates, peer: PeerRef, id: i32) -> Result<SentMessage> {
    edited_inner(result, peer, id)
        .and_then(|mut message| {
            enrich_object(&mut message, result)?;
            Ok(message)
        })
        .map_err(|error| error.context(InvalidResponse("invalid Telegram edit response")))
}
fn edited_inner(result: &tl::enums::Updates, peer: PeerRef, id: i32) -> Result<SentMessage> {
    let list = if let tl::enums::Updates::UpdateShort(r) = result {
        std::slice::from_ref(&r.update)
    } else {
        updates(result)
    };
    list.iter()
        .filter_map(raw_message)
        .filter_map(message)
        .find(|m| m.chat_id == peer.id.bot_api_dialog_id_unchecked() && m.message_id == id)
        .context("Telegram edit returned no matching message body")
}
pub fn album(
    result: &tl::enums::Updates,
    peer: PeerRef,
    random_ids: &[i64],
) -> Result<Vec<SentMessage>> {
    album_inner(result, peer, random_ids)
        .and_then(|mut messages| {
            for message in &mut messages {
                enrich_object(message, result)?;
            }
            Ok(messages)
        })
        .map_err(|error| error.context(InvalidResponse("invalid Telegram album response")))
}
fn album_inner(
    result: &tl::enums::Updates,
    peer: PeerRef,
    random_ids: &[i64],
) -> Result<Vec<SentMessage>> {
    ensure!(
        (2..=10).contains(&random_ids.len()),
        "album response needs 2–10 request IDs"
    );
    let list = updates(result);
    let mut messages = Vec::with_capacity(random_ids.len());
    for random in random_ids {
        let id = list
            .iter()
            .find_map(|u| {
                if let tl::enums::Update::MessageId(u) = u {
                    (u.random_id == *random).then_some(u.id)
                } else {
                    None
                }
            })
            .context("album item has no random-ID acknowledgement")?;
        let found = list
            .iter()
            .filter_map(raw_message)
            .filter_map(message)
            .find(|m| m.chat_id == peer.id.bot_api_dialog_id_unchecked() && m.message_id == id)
            .context("album item body missing")?;
        ensure!(
            !messages.iter().any(|m: &SentMessage| m.message_id == id),
            "duplicate album message ID"
        );
        messages.push(found);
    }
    Ok(messages)
}
