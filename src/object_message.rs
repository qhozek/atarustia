//! Typed Message metadata plus explicit object binding, without serializing a bot handle.
use crate::{
    bot::Bot,
    bot_requests::{SendOptions, Text},
    botapi::*,
};
use grammers_client::tl;
use serde_json::Value;

impl Message {
    pub fn content_type(&self) -> &str {
        if self.text.as_ref().is_some_and(|t| !t.is_empty()) {
            "text"
        } else {
            self.media.content_type().unwrap_or("unknown")
        }
    }
    pub fn html_text(&self) -> ApiResult<String> {
        self.formatted(true)
    }
    pub fn md_text(&self) -> ApiResult<String> {
        self.formatted(false)
    }
    fn formatted(&self, html: bool) -> ApiResult<String> {
        let (text, entities) = if let Some(text) = &self.text {
            (text.as_str(), &self.entities)
        } else {
            (
                self.caption.as_deref().unwrap_or(""),
                &self.caption_entities,
            )
        };
        let mut ranges = entities
            .iter()
            .map(|e| (e.offset, i64::from(e.offset) + i64::from(e.length)))
            .collect::<Vec<_>>();
        ranges.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
        let mut ends = vec![];
        for (start, end) in ranges {
            while ends.last().is_some_and(|e| *e <= i64::from(start)) {
                ends.pop();
            }
            if ends.last().is_some_and(|e| end > *e) {
                return Err(ApiError::validation(
                    if html { "html_text" } else { "md_text" },
                    "crossing entity ranges cannot be formatted".into(),
                ));
            }
            ends.push(end);
        }
        let raw = entities
            .iter()
            .map(|e| e.to_tl(text))
            .collect::<anyhow::Result<Vec<_>>>()
            .map_err(|e| ApiError::from_error(if html { "html_text" } else { "md_text" }, e))?;
        Ok(if html {
            crate::object_format::html(text, entities)
        } else {
            grammers_client::parsers::generate_markdown_message(text, &raw)
        })
    }
    /// Raw conversion accepts explicit peer summaries; it never fetches or invents private profile data.
    pub fn from_tl(raw: &tl::enums::Message, chat: Chat, from_user: Option<User>) -> Self {
        let id = match raw {
            tl::enums::Message::Message(m) => m.id,
            tl::enums::Message::Service(m) => m.id,
            tl::enums::Message::Empty(m) => m.id,
        };
        let mut result = Self {
            message_id: id,
            chat,
            from_user,
            details: crate::message_payload::fields(raw),
            ..Default::default()
        };
        result.enrich(raw);
        result
    }
    pub(crate) fn enrich(&mut self, raw: &tl::enums::Message) {
        let (media, date, reply, from) = match raw {
            tl::enums::Message::Message(m) => {
                self.edit_date = m.edit_date;
                self.media_group_id = m.grouped_id.map(|id| id.to_string());
                self.author_signature = m.post_author.clone();
                self.show_caption_above_media = Some(m.invert_media);
                self.forward_origin = self
                    .details
                    .get("forward_origin")
                    .filter(|v| !v.is_null())
                    .cloned();
                self.reply_markup = m
                    .reply_markup
                    .as_ref()
                    .map(crate::keyboard_payload::from_tl);
                self.media = crate::object_media::Fields::from_tl(m.media.as_ref());
                let caption = m.media.as_ref().is_some_and(|m| {
                    !matches!(
                        m,
                        tl::enums::MessageMedia::Empty | tl::enums::MessageMedia::WebPage(_)
                    )
                });
                let entities = m
                    .entities
                    .as_ref()
                    .map(|v| {
                        v.iter()
                            .filter_map(MessageEntity::from_tl)
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                if caption {
                    self.caption = (!m.message.is_empty()).then(|| m.message.clone());
                    self.caption_entities = entities;
                    self.text = None;
                    self.entities.clear();
                } else {
                    self.text = Some(m.message.clone());
                    self.entities = entities;
                    self.caption = None;
                    self.caption_entities.clear();
                }
                if let Some(id) = m.via_bot_id {
                    self.via_bot = Some(User {
                        id,
                        is_bot: true,
                        ..Default::default()
                    });
                }
                self.has_media_spoiler = match &m.media {
                    Some(tl::enums::MessageMedia::Photo(p)) => Some(p.spoiler),
                    Some(tl::enums::MessageMedia::Document(d)) => Some(d.spoiler),
                    _ => None,
                };
                if let Some(tl::enums::MessageMedia::WebPage(p)) = &m.media {
                    let url = match &p.webpage {
                        tl::enums::WebPage::Page(p) => Some(p.url.clone()),
                        tl::enums::WebPage::Pending(p) => p.url.clone(),
                        tl::enums::WebPage::Empty(p) => p.url.clone(),
                        _ => None,
                    };
                    self.link_preview_options = Some(crate::bot_requests::Preview {
                        url,
                        prefer_large_media: p.force_large_media,
                        prefer_small_media: p.force_small_media,
                        show_above_text: m.invert_media,
                        ..Default::default()
                    });
                }
                (
                    m.media.as_ref(),
                    m.date,
                    m.reply_to.as_ref(),
                    m.from_id.as_ref(),
                )
            }
            tl::enums::Message::Service(m) => {
                self.reply_markup = None;
                let unknown = |id| User {
                    id,
                    ..Default::default()
                };
                match &m.action {
                    tl::enums::MessageAction::ChatAddUser(a) => {
                        self.new_chat_members = a.users.iter().map(|id| unknown(*id)).collect()
                    }
                    tl::enums::MessageAction::ChatCreate(a) => {
                        self.new_chat_members = a.users.iter().map(|id| unknown(*id)).collect()
                    }
                    tl::enums::MessageAction::ChatDeleteUser(a) => {
                        self.left_chat_member = Some(unknown(a.user_id))
                    }
                    _ => {}
                }
                (None, m.date, m.reply_to.as_ref(), m.from_id.as_ref())
            }
            _ => return,
        };
        self.date = Some(date);
        if self.from_user.is_none()
            && let Some(tl::enums::Peer::User(u)) = from
        {
            self.from_user = Some(User {
                id: u.user_id,
                ..Default::default()
            });
        }
        if let Some(from) = from
            && !matches!(from, tl::enums::Peer::User(_))
        {
            let id =
                grammers_session::types::PeerId::from(from.clone()).bot_api_dialog_id_unchecked();
            self.sender_chat = Some(Chat {
                id,
                kind: if matches!(from, tl::enums::Peer::Channel(_)) {
                    "channel"
                } else {
                    "group"
                }
                .into(),
                ..Default::default()
            });
        }
        if let tl::enums::Message::Message(m) = raw {
            if self.from_user.is_none()
                && m.from_id.is_none()
                && !m.out
                && self.chat.kind == "private"
            {
                self.from_user = Some(User {
                    id: self.chat.id,
                    first_name: self.chat.first_name.clone().unwrap_or_default(),
                    last_name: self.chat.last_name.clone(),
                    username: self.chat.username.clone(),
                    ..Default::default()
                });
            }
            if m.from_id.is_none() && m.post {
                self.sender_chat = Some(self.chat.clone());
            }
        }
        if let Some(tl::enums::MessageReplyHeader::Header(r)) = reply {
            self.is_topic_message = r.forum_topic.then_some(true);
            self.message_thread_id = if r.forum_topic {
                r.reply_to_top_id.or(r.reply_to_msg_id)
            } else {
                None
            };
            self.reply_to_message_id = r
                .reply_to_msg_id
                .filter(|_| !r.forum_topic || r.reply_to_top_id.is_some());
            self.reply_to_message = self.reply_to_message_id.map(|id| {
                Box::new(Self {
                    message_id: id,
                    chat: self.chat.clone(),
                    ..Default::default()
                })
            });
        }
        if let Some(media) = media
            && let Some(native) = grammers_client::media::Media::from_raw(media.clone())
        {
            self.file = File::from_media(&native).ok();
            self.media_type = self
                .file
                .as_ref()
                .and_then(|f| crate::file_id::FileRef::decode(&f.file_id).ok())
                .map(|f| f.kind);
        }
    }
    pub fn bind<'a>(&'a self, bot: &'a Bot) -> BoundMessage<'a> {
        BoundMessage { message: self, bot }
    }
    pub fn thread_options(&self, mut options: SendOptions, reply: bool) -> SendOptions {
        if self.is_topic_message == Some(true) && options.message_thread_id.is_none() {
            options.message_thread_id = self.message_thread_id;
        }
        if reply && options.reply_to_message_id.is_none() {
            options.reply_to_message_id = Some(self.message_id);
        }
        if options.business_connection_id.is_none() {
            options.business_connection_id = self.business_connection_id.clone();
        }
        options
    }
}
impl CallbackQuery {
    pub fn from_tl(
        raw: &tl::enums::Update,
        from_user: User,
        chat: Option<Chat>,
    ) -> ApiResult<Self> {
        use base64::{Engine as _, engine::general_purpose::STANDARD};
        let (id, user_id, message_id, chat_id, inline_id, instance, data, game) = match raw {
            tl::enums::Update::BotCallbackQuery(q) => (
                q.query_id,
                q.user_id,
                Some(q.msg_id),
                Some(
                    grammers_session::types::PeerId::from(q.peer.clone())
                        .bot_api_dialog_id_unchecked(),
                ),
                None,
                q.chat_instance,
                q.data.as_ref(),
                q.game_short_name.clone(),
            ),
            tl::enums::Update::InlineBotCallbackQuery(q) => (
                q.query_id,
                q.user_id,
                None,
                None,
                Some(crate::file_id::encode_inline_id(&q.msg_id)),
                q.chat_instance,
                q.data.as_ref(),
                q.game_short_name.clone(),
            ),
            _ => {
                return Err(ApiError::validation(
                    "callback_object",
                    "not a callback update".into(),
                ));
            }
        };
        if from_user.id != user_id || chat.as_ref().zip(chat_id).is_some_and(|(c, id)| c.id != id) {
            return Err(ApiError::validation(
                "callback_object",
                "callback identity mismatch".into(),
            ));
        }
        let message = message_id.zip(chat_id).map(|(message_id, id)| {
            Box::new(Message {
                message_id,
                chat: chat.unwrap_or(Chat {
                    id,
                    kind: if id > 0 { "private" } else { "unknown" }.into(),
                    ..Default::default()
                }),
                ..Default::default()
            })
        });
        Ok(Self {
            id: id.to_string(),
            from_user,
            data: data
                .and_then(|d| std::str::from_utf8(d).ok())
                .map(str::to_owned),
            inline_message_id: inline_id,
            message_id,
            chat_id,
            data_base64: data.map(|d| STANDARD.encode(d)),
            chat_instance: Some(instance.to_string()),
            game_short_name: game,
            message,
        })
    }
    pub async fn answer(
        &self,
        bot: &Bot,
        options: &crate::bot_requests::CallbackAnswer,
    ) -> ApiResult<()> {
        let id = self.id.parse::<i64>().map_err(|_| {
            ApiError::validation("callback_answer", "invalid callback query ID".into())
        })?;
        bot.answer_callback_query_with_options(id, options)
            .await
            .map_err(|mut e| {
                e.method = "callback_answer".into();
                e
            })
    }
}
fn query_location(value: Option<&tl::enums::GeoPoint>) -> Option<Location> {
    let tl::enums::GeoPoint::Point(p) = value? else {
        return None;
    };
    Some(Location {
        latitude: p.lat,
        longitude: p.long,
        horizontal_accuracy: p.accuracy_radius,
        live_period: None,
        heading: None,
        proximity_alert_radius: None,
    })
}
impl InlineQuery {
    pub fn from_tl(raw: &tl::types::UpdateBotInlineQuery, from_user: User) -> ApiResult<Self> {
        if raw.user_id != from_user.id {
            return Err(ApiError::validation(
                "inline_query_object",
                "inline query user mismatch".into(),
            ));
        }
        let chat_type = raw.peer_type.as_ref().map(|p| {
            match p {
                tl::enums::InlineQueryPeerType::Pm => "private",
                tl::enums::InlineQueryPeerType::Chat => "group",
                tl::enums::InlineQueryPeerType::Megagroup => "supergroup",
                tl::enums::InlineQueryPeerType::Broadcast => "channel",
                tl::enums::InlineQueryPeerType::BotPm => "bot_private",
                tl::enums::InlineQueryPeerType::SameBotPm => "same_bot_private",
            }
            .into()
        });
        Ok(Self {
            id: raw.query_id.to_string(),
            from_user,
            query: raw.query.clone(),
            offset: raw.offset.clone(),
            chat_type,
            location: query_location(raw.geo.as_ref()),
        })
    }
    pub async fn answer(
        &self,
        bot: &Bot,
        results: &[InlineResult],
        options: &crate::bot_requests::InlineAnswer,
    ) -> ApiResult<()> {
        let id = self.id.parse::<i64>().map_err(|_| {
            ApiError::validation("inline_query_answer", "invalid inline query ID".into())
        })?;
        bot.answer_inline_query_with_options(id, results, options)
            .await
            .map_err(|mut e| {
                e.method = "inline_query_answer".into();
                e
            })
    }
}
impl ChosenInlineResult {
    pub fn from_tl(raw: &tl::types::UpdateBotInlineSend, from_user: User) -> ApiResult<Self> {
        if raw.user_id != from_user.id {
            return Err(ApiError::validation(
                "chosen_inline_result_object",
                "chosen result user mismatch".into(),
            ));
        }
        Ok(Self {
            result_id: raw.id.clone(),
            from_user,
            location: query_location(raw.geo.as_ref()),
            inline_message_id: raw.msg_id.as_ref().map(crate::file_id::encode_inline_id),
            query: raw.query.clone(),
        })
    }
}
/// Explicit lifetime binding replaces Python's hidden .as_(bot); serialization remains pure data.
pub struct BoundMessage<'a> {
    pub message: &'a Message,
    pub bot: &'a Bot,
}
impl BoundMessage<'_> {
    async fn peer(&self, method: &str) -> ApiResult<grammers_session::types::PeerRef> {
        self.bot
            .resolve_chat(&self.message.chat.id.to_string())
            .await
            .map_err(|mut e| {
                e.method = method.into();
                e
            })
    }
    pub async fn answer(&self, text: &Text, options: SendOptions) -> ApiResult<SentMessage> {
        let options = self.message.thread_options(options, false);
        let peer = self.peer("answer").await?;
        self.bot
            .send_message_with_options(peer, text, None, &Default::default(), &options)
            .await
            .map_err(|mut e| {
                e.method = "answer".into();
                e
            })
    }
    pub async fn reply(&self, text: &Text, options: SendOptions) -> ApiResult<SentMessage> {
        let options = self.message.thread_options(options, true);
        let peer = self.peer("reply").await?;
        self.bot
            .send_message_with_options(peer, text, None, &Default::default(), &options)
            .await
            .map_err(|mut e| {
                e.method = "reply".into();
                e
            })
    }
    pub async fn answer_media(
        &self,
        media: &crate::media::Media,
        caption: &Text,
        options: SendOptions,
    ) -> ApiResult<SentMessage> {
        let options = self.message.thread_options(options, false);
        let peer = self.peer("answer_media").await?;
        self.bot
            .send_media_with_options(peer, media, caption, false, &options)
            .await
            .map_err(|mut e| {
                e.method = "answer_media".into();
                e
            })
    }
    pub async fn reply_media(
        &self,
        media: &crate::media::Media,
        caption: &Text,
        options: SendOptions,
    ) -> ApiResult<SentMessage> {
        let options = self.message.thread_options(options, true);
        let peer = self.peer("reply_media").await?;
        self.bot
            .send_media_with_options(peer, media, caption, false, &options)
            .await
            .map_err(|mut e| {
                e.method = "reply_media".into();
                e
            })
    }
    pub async fn edit(
        &self,
        edit: &crate::bot_requests::Edit,
        keyboard: Option<&crate::markup::Keyboard>,
    ) -> ApiResult<EditResult> {
        let peer = self.peer("edit").await?;
        self.bot
            .edit_with_options(
                peer,
                self.message.message_id,
                edit,
                keyboard,
                self.message.business_connection_id.as_deref(),
            )
            .await
            .map_err(|mut e| {
                e.method = "edit".into();
                e
            })
    }
    pub async fn edit_text(
        &self,
        text: Text,
        preview: crate::bot_requests::Preview,
        keyboard: Option<&crate::markup::Keyboard>,
    ) -> ApiResult<EditResult> {
        self.edit(&crate::bot_requests::Edit::Text { text, preview }, keyboard)
            .await
            .map_err(|mut e| {
                e.method = "edit_text".into();
                e
            })
    }
    pub async fn edit_caption(
        &self,
        caption: Text,
        above: bool,
        keyboard: Option<&crate::markup::Keyboard>,
    ) -> ApiResult<EditResult> {
        self.edit(
            &crate::bot_requests::Edit::Caption {
                caption,
                show_above_media: above,
            },
            keyboard,
        )
        .await
        .map_err(|mut e| {
            e.method = "edit_caption".into();
            e
        })
    }
    pub async fn edit_media(
        &self,
        media: crate::media::Media,
        caption: Text,
        above: bool,
        keyboard: Option<&crate::markup::Keyboard>,
    ) -> ApiResult<EditResult> {
        self.edit(
            &crate::bot_requests::Edit::Media {
                media: Box::new(media),
                caption,
                show_above_media: above,
            },
            keyboard,
        )
        .await
        .map_err(|mut e| {
            e.method = "edit_media".into();
            e
        })
    }
    pub async fn edit_reply_markup(
        &self,
        keyboard: Option<&crate::markup::Keyboard>,
    ) -> ApiResult<EditResult> {
        self.edit(&crate::bot_requests::Edit::Keyboard, keyboard)
            .await
            .map_err(|mut e| {
                e.method = "edit_reply_markup".into();
                e
            })
    }
    pub async fn delete_reply_markup(&self) -> ApiResult<EditResult> {
        self.edit_reply_markup(None).await.map_err(|mut e| {
            e.method = "delete_reply_markup".into();
            e
        })
    }
    pub async fn delete(&self) -> ApiResult<()> {
        let peer = self.peer("delete").await?;
        self.bot
            .delete_message(peer, self.message.message_id)
            .await
            .map_err(|mut e| {
                e.method = "delete".into();
                e
            })
    }
    pub async fn pin(&self, silent: bool) -> ApiResult<()> {
        let peer = self.peer("pin").await?;
        self.bot
            .pin_chat_message(peer, self.message.message_id, silent)
            .await
            .map_err(|mut e| {
                e.method = "pin".into();
                e
            })
    }
    pub async fn unpin(&self) -> ApiResult<()> {
        let peer = self.peer("unpin").await?;
        self.bot
            .unpin_chat_message(peer, self.message.message_id)
            .await
            .map_err(|mut e| {
                e.method = "unpin".into();
                e
            })
    }
    pub async fn forward(
        &self,
        to: grammers_session::types::PeerRef,
        options: SendOptions,
    ) -> ApiResult<SentMessage> {
        let from = self.peer("forward").await?;
        self.bot
            .forward_message_with_options(to, from, self.message.message_id, false, &options)
            .await
            .map_err(|mut e| {
                e.method = "forward".into();
                e
            })
    }
}
macro_rules! media_shortcut {
    ($answer:ident,$reply:ident,$kind:ident) => {
        impl BoundMessage<'_> {
            pub async fn $answer(
                &self,
                source: crate::media::Source,
                caption: &Text,
                options: SendOptions,
            ) -> ApiResult<SentMessage> {
                let media = crate::media::Media {
                    kind: crate::file_id::FileKind::$kind,
                    source,
                    ..Default::default()
                };
                self.answer_media(&media, caption, options)
                    .await
                    .map_err(|mut e| {
                        e.method = stringify!($answer).into();
                        e
                    })
            }
            pub async fn $reply(
                &self,
                source: crate::media::Source,
                caption: &Text,
                options: SendOptions,
            ) -> ApiResult<SentMessage> {
                let media = crate::media::Media {
                    kind: crate::file_id::FileKind::$kind,
                    source,
                    ..Default::default()
                };
                self.reply_media(&media, caption, options)
                    .await
                    .map_err(|mut e| {
                        e.method = stringify!($reply).into();
                        e
                    })
            }
        }
    };
}
media_shortcut!(answer_photo, reply_photo, Photo);
media_shortcut!(answer_document, reply_document, Document);
media_shortcut!(answer_video, reply_video, Video);
media_shortcut!(answer_animation, reply_animation, Animation);
media_shortcut!(answer_audio, reply_audio, Audio);
media_shortcut!(answer_voice, reply_voice, Voice);
media_shortcut!(answer_sticker, reply_sticker, Sticker);
impl crate::bot_response::SentMessage {
    pub fn object(&self) -> ApiResult<Message> {
        self.validate()
            .map_err(|e| ApiError::from_error("message_object", e))?;
        let data = &self.data;
        if let Some(object) = data.get("object") {
            let object: Message = serde_json::from_value(object.clone())
                .map_err(|e| ApiError::from_error("message_object", e.into()))?;
            if object.message_id != self.message_id || object.chat.id != self.chat_id {
                return Err(ApiError::from_error(
                    "message_object",
                    anyhow::anyhow!(crate::bot_response::InvalidResponse(
                        "object identity mismatch"
                    )),
                ));
            }
            return Ok(object);
        }
        let mut object = Message {
            message_id: self.message_id,
            chat: Chat {
                id: self.chat_id,
                kind: if self.chat_id > 0 {
                    "private"
                } else {
                    "unknown"
                }
                .into(),
                ..Default::default()
            },
            details: data.clone(),
            ..Default::default()
        };
        object.text = data.get("text").and_then(Value::as_str).map(str::to_owned);
        object.caption = data
            .get("caption")
            .and_then(Value::as_str)
            .map(str::to_owned);
        object.date = data
            .get("date")
            .and_then(Value::as_i64)
            .and_then(|n| n.try_into().ok());
        object.message_thread_id = data
            .pointer("/reply/message_thread_id")
            .and_then(Value::as_i64)
            .and_then(|n| n.try_into().ok());
        object.is_topic_message = data
            .pointer("/reply/is_topic_message")
            .and_then(Value::as_bool);
        object.entities = serde_json::from_value(
            data.get("entities")
                .cloned()
                .unwrap_or(serde_json::json!([])),
        )
        .map_err(|e| ApiError::from_error("message_object", e.into()))?;
        object.caption_entities = serde_json::from_value(
            data.get("caption_entities")
                .cloned()
                .unwrap_or(serde_json::json!([])),
        )
        .map_err(|e| ApiError::from_error("message_object", e.into()))?;
        Ok(object)
    }
}
