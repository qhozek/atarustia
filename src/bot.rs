//! Bot-facing API: one error boundary and serializable results. Native objects are explicit opt-in.
//!
//! ```no_run
//! # async fn example(bot: atarustia::bot::Bot, peer: grammers_session::types::PeerRef) -> anyhow::Result<()> {
//! let sent: atarustia::botapi::SentMessage = bot
//!     .send_message(peer, grammers_client::message::InputMessage::new().text("hello"))
//!     .await?;
//! assert!(sent.message_id > 0);
//! // Explicit opt-in for trusted extensions that still require grammers objects:
//! let native: grammers_client::message::Message = bot.native()
//!     .send_message(peer, grammers_client::message::InputMessage::new().text("native"))
//!     .await?;
//! # Ok(())
//! # }
//! ```
use crate::{
    bot_response::{EditResult, MessageId, SentGuestMessage, SentMessage},
    botapi::{ApiError, ApiResult},
};
use grammers_client::{
    Client,
    message::{InputMessage, Message},
    tl,
    update::CallbackQuery,
};
use grammers_session::types::PeerRef;
use std::sync::Arc;

#[derive(Clone)]
pub struct Bot {
    pub client: Client,
    pub id: i64,
    pub username: String,
    pub session: Option<Arc<grammers_session::storages::ErasedSession>>,
}
async fn boundary<T>(
    method: &str,
    future: impl std::future::Future<Output = anyhow::Result<T>>,
) -> ApiResult<T> {
    future
        .await
        .map_err(|error| ApiError::from_error(method, error))
}
fn validated(message: SentMessage) -> anyhow::Result<SentMessage> {
    message.validate()?;
    Ok(message)
}
fn sent(message: Message) -> anyhow::Result<SentMessage> {
    SentMessage::from_native(&message)
}
fn acknowledged(peer: PeerRef, id: i32) -> anyhow::Result<EditResult> {
    anyhow::ensure!(id > 0, "message ID must be positive");
    Ok(EditResult::Acknowledged {
        chat_id: peer.id.bot_api_dialog_id_unchecked(),
        message_id: id,
    })
}
fn inline_ack(id: &str) -> anyhow::Result<EditResult> {
    crate::file_id::decode_inline_id(id)?;
    Ok(EditResult::Inline {
        inline_message_id: id.into(),
    })
}
fn callback_ack(query: &CallbackQuery) -> anyhow::Result<EditResult> {
    match &query.raw {
        tl::enums::Update::InlineBotCallbackQuery(q) => {
            inline_ack(&crate::file_id::encode_inline_id(&q.msg_id))
        }
        tl::enums::Update::BotCallbackQuery(q) => {
            anyhow::ensure!(q.msg_id > 0, "callback message ID must be positive");
            Ok(EditResult::Acknowledged {
                chat_id: grammers_session::types::PeerId::from(q.peer.clone())
                    .bot_api_dialog_id_unchecked(),
                message_id: q.msg_id,
            })
        }
        _ => anyhow::bail!("not a callback query"),
    }
}
impl Bot {
    pub fn from_native(native: crate::native_bot::NativeBot) -> Self {
        Self {
            client: native.client,
            id: native.id,
            username: native.username,
            session: native.session,
        }
    }
    /// Explicit compatibility escape hatch. Never implicitly deref the facade to the native API.
    pub fn native(&self) -> crate::native_bot::NativeBot {
        crate::native_bot::NativeBot {
            client: self.client.clone(),
            id: self.id,
            username: self.username.clone(),
            session: self.session.clone(),
        }
    }
    pub async fn resolve_chat(&self, value: &str) -> ApiResult<PeerRef> {
        boundary("resolve_chat", self.native().resolve_chat(value)).await
    }
    pub async fn command_scope(
        &self,
        scope: &crate::bot_requests::CommandScope,
    ) -> ApiResult<tl::enums::BotCommandScope> {
        boundary("command_scope", self.native().command_scope(scope)).await
    }
    pub async fn invoke<R: tl::RemoteCall>(&self, request: &R) -> ApiResult<R::Return> {
        boundary("invoke", self.native().invoke(request)).await
    }
    pub async fn invoke_api<R: tl::RemoteCall>(
        &self,
        method: &str,
        request: &R,
    ) -> ApiResult<R::Return> {
        self.client
            .invoke(request)
            .await
            .map_err(|e| ApiError::from_rpc(method, &e))
    }
    pub async fn invoke_business<R: tl::RemoteCall>(
        &self,
        method: &str,
        request: R,
        id: Option<&str>,
    ) -> ApiResult<R::Return> {
        self.native()
            .invoke_business(method, request, id)
            .await
            .map_err(|mut e| {
                e.method = method.into();
                e
            })
    }
    pub async fn get_me(&self) -> ApiResult<crate::botapi::User> {
        boundary("get_me", self.native().get_me()).await
    }
    pub async fn me(&self) -> ApiResult<crate::botapi::User> {
        boundary("me", self.native().me()).await
    }
    pub async fn get_chat(&self, peer: PeerRef) -> ApiResult<crate::botapi::ChatFullInfo> {
        boundary("get_chat", self.native().get_chat_full_info(peer)).await
    }
    pub async fn get_chat_full_info(
        &self,
        peer: PeerRef,
    ) -> ApiResult<crate::botapi::ChatFullInfo> {
        boundary("get_chat_full_info", self.native().get_chat_full_info(peer)).await
    }
    pub async fn get_chat_summary(&self, peer: PeerRef) -> ApiResult<crate::botapi::Chat> {
        boundary("get_chat_summary", self.native().get_chat(peer)).await
    }
    pub async fn get_chat_member(
        &self,
        peer: PeerRef,
        user: PeerRef,
    ) -> ApiResult<crate::botapi::ChatMember> {
        boundary(
            "get_chat_member",
            self.native().get_chat_member_info(peer, user),
        )
        .await
    }
    pub async fn get_chat_member_info(
        &self,
        peer: PeerRef,
        user: PeerRef,
    ) -> ApiResult<crate::botapi::ChatMember> {
        boundary(
            "get_chat_member_info",
            self.native().get_chat_member_info(peer, user),
        )
        .await
    }
    pub async fn get_file(&self, id: &str) -> ApiResult<crate::botapi::File> {
        boundary("get_file", self.native().get_file_info(id)).await
    }
    pub async fn get_file_info(&self, id: &str) -> ApiResult<crate::botapi::File> {
        boundary("get_file_info", self.native().get_file_info(id)).await
    }

    pub async fn send_message(
        &self,
        peer: PeerRef,
        message: InputMessage,
    ) -> ApiResult<SentMessage> {
        boundary("send_message", async {
            sent(self.native().send_message(peer, message).await?)
        })
        .await
    }
    pub async fn send_media(
        &self,
        peer: PeerRef,
        media: &crate::media::Media,
        caption: InputMessage,
    ) -> ApiResult<SentMessage> {
        boundary("send_media", async {
            media.validate()?;
            sent(self.native().send_media(peer, media, caption).await?)
        })
        .await
    }
    pub async fn send_photo(
        &self,
        peer: PeerRef,
        bytes: Vec<u8>,
        caption: &str,
    ) -> ApiResult<SentMessage> {
        boundary("send_photo", async {
            let media = crate::media::Media {
                kind: crate::file_id::FileKind::Photo,
                source: crate::media::Source::Bytes {
                    name: "photo.jpg".into(),
                    data: bytes,
                },
                ..Default::default()
            };
            sent(
                self.native()
                    .send_media(peer, &media, InputMessage::new().text(caption))
                    .await?,
            )
        })
        .await
    }
    pub async fn send_photo_url(
        &self,
        peer: PeerRef,
        url: &str,
        caption: &str,
    ) -> ApiResult<SentMessage> {
        boundary("send_photo_url", async {
            let media = crate::media::Media::from_url(crate::file_id::FileKind::Photo, url);
            sent(
                self.native()
                    .send_media(peer, &media, InputMessage::new().text(caption))
                    .await?,
            )
        })
        .await
    }
    pub async fn send_cached_photo(
        &self,
        db: &crate::database::Database,
        peer: PeerRef,
        bytes: Vec<u8>,
        caption: &str,
    ) -> ApiResult<SentMessage> {
        boundary("send_cached_photo", async {
            sent(
                self.native()
                    .send_cached_photo(db, peer, bytes, caption)
                    .await?,
            )
        })
        .await
    }
    pub async fn send_document(
        &self,
        peer: PeerRef,
        name: &str,
        bytes: Vec<u8>,
        caption: &str,
    ) -> ApiResult<SentMessage> {
        boundary("send_document", async {
            sent(
                self.native()
                    .send_document(peer, name, bytes, caption)
                    .await?,
            )
        })
        .await
    }
    #[allow(clippy::too_many_arguments)]
    pub async fn send_video(
        &self,
        peer: PeerRef,
        source: crate::media::Source,
        caption: &str,
        duration: f64,
        width: i32,
        height: i32,
    ) -> ApiResult<SentMessage> {
        boundary("send_video", async {
            sent(
                self.native()
                    .send_video(peer, source, caption, duration, width, height)
                    .await?,
            )
        })
        .await
    }
    pub async fn send_audio(
        &self,
        peer: PeerRef,
        source: crate::media::Source,
        caption: &str,
        duration: f64,
        title: Option<String>,
        performer: Option<String>,
    ) -> ApiResult<SentMessage> {
        boundary("send_audio", async {
            sent(
                self.native()
                    .send_audio(peer, source, caption, duration, title, performer)
                    .await?,
            )
        })
        .await
    }
    pub async fn send_voice(
        &self,
        peer: PeerRef,
        source: crate::media::Source,
        caption: &str,
        duration: f64,
    ) -> ApiResult<SentMessage> {
        boundary("send_voice", async {
            sent(
                self.native()
                    .send_voice(peer, source, caption, duration)
                    .await?,
            )
        })
        .await
    }
    pub async fn send_animation(
        &self,
        peer: PeerRef,
        source: crate::media::Source,
        caption: &str,
    ) -> ApiResult<SentMessage> {
        boundary("send_animation", async {
            sent(self.native().send_animation(peer, source, caption).await?)
        })
        .await
    }
    pub async fn send_sticker(
        &self,
        peer: PeerRef,
        source: crate::media::Source,
    ) -> ApiResult<SentMessage> {
        boundary("send_sticker", async {
            sent(self.native().send_sticker(peer, source).await?)
        })
        .await
    }
    pub async fn send_video_note(
        &self,
        peer: PeerRef,
        source: crate::media::Source,
        duration: f64,
        size: i32,
    ) -> ApiResult<SentMessage> {
        boundary("send_video_note", async {
            sent(
                self.native()
                    .send_video_note(peer, source, duration, size)
                    .await?,
            )
        })
        .await
    }
    pub async fn send_rich(
        &self,
        peer: PeerRef,
        rich: &crate::bot_edits::RichText,
        keys: Option<&[Vec<crate::markup::Key>]>,
    ) -> ApiResult<SentMessage> {
        boundary("send_rich", async {
            sent(self.native().send_rich(peer, rich, keys).await?)
        })
        .await
    }
    pub async fn send_media_group(
        &self,
        peer: PeerRef,
        items: &[(crate::media::Media, String)],
        reply_to: Option<i32>,
    ) -> ApiResult<Vec<SentMessage>> {
        boundary("send_media_group", async {
            let converted = items
                .iter()
                .map(|(media, text)| crate::bot_requests::AlbumItem {
                    media: media.clone(),
                    caption: crate::bot_requests::Text {
                        text: text.clone(),
                        ..Default::default()
                    },
                })
                .collect::<Vec<_>>();
            let options = crate::bot_requests::SendOptions {
                reply_to_message_id: reply_to,
                ..Default::default()
            };
            let messages = self
                .native()
                .send_media_group_with_options(peer, &converted, &options)
                .await?;
            messages.into_iter().map(validated).collect()
        })
        .await
    }
    pub async fn send_message_with_options(
        &self,
        peer: PeerRef,
        text: &crate::bot_requests::Text,
        rich: Option<&crate::bot_edits::RichText>,
        preview: &crate::bot_requests::Preview,
        options: &crate::bot_requests::SendOptions,
    ) -> ApiResult<SentMessage> {
        boundary("send_message_with_options", async {
            validated(
                self.native()
                    .send_message_with_options(peer, text, rich, preview, options)
                    .await?,
            )
        })
        .await
    }
    pub async fn send_media_with_options(
        &self,
        peer: PeerRef,
        media: &crate::media::Media,
        caption: &crate::bot_requests::Text,
        above: bool,
        options: &crate::bot_requests::SendOptions,
    ) -> ApiResult<SentMessage> {
        boundary("send_media_with_options", async {
            validated(
                self.native()
                    .send_media_with_options(peer, media, caption, above, options)
                    .await?,
            )
        })
        .await
    }
    pub async fn send_media_group_with_options(
        &self,
        peer: PeerRef,
        items: &[crate::bot_requests::AlbumItem],
        options: &crate::bot_requests::SendOptions,
    ) -> ApiResult<Vec<SentMessage>> {
        boundary("send_media_group_with_options", async {
            self.native()
                .send_media_group_with_options(peer, items, options)
                .await?
                .into_iter()
                .map(validated)
                .collect()
        })
        .await
    }
    pub async fn forward_message_with_options(
        &self,
        to: PeerRef,
        from: PeerRef,
        id: i32,
        copy: bool,
        options: &crate::bot_requests::SendOptions,
    ) -> ApiResult<SentMessage> {
        boundary("forward_message_with_options", async {
            validated(
                self.native()
                    .forward_message_with_options(to, from, id, copy, options)
                    .await?,
            )
        })
        .await
    }
    pub async fn copy_message(&self, peer: PeerRef, source: &Message) -> ApiResult<MessageId> {
        boundary("copy_message", async {
            let from = source
                .peer_ref()
                .await
                .map_err(anyhow::Error::from_boxed)?
                .ok_or_else(|| anyhow::anyhow!("copy source peer missing"))?;
            let message = self
                .native()
                .forward_message_with_options(peer, from, source.id(), true, &Default::default())
                .await?;
            message.validate()?;
            Ok(MessageId {
                message_id: message.message_id,
            })
        })
        .await
    }
    pub async fn forward_message(&self, peer: PeerRef, source: &Message) -> ApiResult<SentMessage> {
        boundary("forward_message", async {
            sent(self.native().forward_message(peer, source).await?)
        })
        .await
    }

    pub async fn edit_with_options(
        &self,
        peer: PeerRef,
        id: i32,
        edit: &crate::bot_requests::Edit,
        keys: Option<&crate::markup::Keyboard>,
        business: Option<&str>,
    ) -> ApiResult<EditResult> {
        boundary("edit_with_options", async {
            let message = self
                .native()
                .edit_with_options(peer, id, edit, keys, business)
                .await?;
            message.validate()?;
            Ok(EditResult::Message { message })
        })
        .await
    }
    pub async fn edit_inline_with_options(
        &self,
        id: &str,
        edit: &crate::bot_requests::Edit,
        keys: Option<&crate::markup::Keyboard>,
    ) -> ApiResult<EditResult> {
        boundary("edit_inline_with_options", async {
            let ack = inline_ack(id)?;
            self.native()
                .edit_inline_with_options(id, edit, keys)
                .await?;
            Ok(ack)
        })
        .await
    }
    pub async fn edit_message_text(
        &self,
        peer: PeerRef,
        id: i32,
        message: InputMessage,
    ) -> ApiResult<EditResult> {
        boundary("edit_message_text", async {
            let ack = acknowledged(peer, id)?;
            self.native().edit_message_text(peer, id, message).await?;
            Ok(ack)
        })
        .await
    }
    pub async fn edit_message_media(
        &self,
        peer: PeerRef,
        id: i32,
        media: &crate::media::Media,
        caption: InputMessage,
    ) -> ApiResult<EditResult> {
        boundary("edit_message_media", async {
            let ack = acknowledged(peer, id)?;
            self.native()
                .edit_message_media(peer, id, media, caption)
                .await?;
            Ok(ack)
        })
        .await
    }
    pub async fn edit_message_rich(
        &self,
        peer: PeerRef,
        id: i32,
        rich: &crate::bot_edits::RichText,
        keys: Option<&[Vec<crate::markup::Key>]>,
    ) -> ApiResult<EditResult> {
        boundary("edit_message_rich", async {
            let ack = acknowledged(peer, id)?;
            self.native()
                .edit_message_rich(peer, id, rich, keys)
                .await?;
            Ok(ack)
        })
        .await
    }
    pub async fn edit_message_caption(
        &self,
        peer: PeerRef,
        id: i32,
        text: &str,
        html: bool,
        keys: Option<&[Vec<crate::markup::Key>]>,
    ) -> ApiResult<EditResult> {
        boundary("edit_message_caption", async {
            let ack = acknowledged(peer, id)?;
            self.native()
                .edit_message_caption(peer, id, text, html, keys)
                .await?;
            Ok(ack)
        })
        .await
    }
    pub async fn edit_message_reply_markup(
        &self,
        peer: PeerRef,
        id: i32,
        keys: &[Vec<crate::markup::Key>],
    ) -> ApiResult<EditResult> {
        boundary("edit_message_reply_markup", async {
            let ack = acknowledged(peer, id)?;
            self.native()
                .edit_message_reply_markup(peer, id, keys)
                .await?;
            Ok(ack)
        })
        .await
    }
    pub async fn edit_inline_message(
        &self,
        id: &str,
        message: InputMessage,
        media: Option<&crate::media::Media>,
        rich: Option<&str>,
    ) -> ApiResult<EditResult> {
        boundary("edit_inline_message", async {
            let ack = inline_ack(id)?;
            self.native()
                .edit_inline_message(id, message, media, rich)
                .await?;
            Ok(ack)
        })
        .await
    }
    pub async fn edit_inline_caption(
        &self,
        id: &str,
        text: &str,
        html: bool,
        keys: Option<&[Vec<crate::markup::Key>]>,
    ) -> ApiResult<EditResult> {
        boundary("edit_inline_caption", async {
            let ack = inline_ack(id)?;
            self.native()
                .edit_inline_caption(id, text, html, keys)
                .await?;
            Ok(ack)
        })
        .await
    }
    pub async fn edit_inline_reply_markup(
        &self,
        id: &str,
        keys: &[Vec<crate::markup::Key>],
    ) -> ApiResult<EditResult> {
        boundary("edit_inline_reply_markup", async {
            let ack = inline_ack(id)?;
            self.native().edit_inline_reply_markup(id, keys).await?;
            Ok(ack)
        })
        .await
    }
    pub async fn edit_callback(
        &self,
        query: &CallbackQuery,
        message: InputMessage,
    ) -> ApiResult<EditResult> {
        boundary("edit_callback", async {
            let ack = callback_ack(query)?;
            self.native().edit_callback(query, message).await?;
            Ok(ack)
        })
        .await
    }
    pub async fn edit_callback_media(
        &self,
        query: &CallbackQuery,
        message: InputMessage,
        media: &crate::media::Media,
    ) -> ApiResult<EditResult> {
        boundary("edit_callback_media", async {
            let ack = callback_ack(query)?;
            self.native()
                .edit_callback_media(query, message, media)
                .await?;
            Ok(ack)
        })
        .await
    }
    pub async fn edit_callback_rich(
        &self,
        query: &CallbackQuery,
        message: InputMessage,
        rich: Option<String>,
    ) -> ApiResult<EditResult> {
        boundary("edit_callback_rich", async {
            let ack = callback_ack(query)?;
            self.native()
                .edit_callback_rich(query, message, rich)
                .await?;
            Ok(ack)
        })
        .await
    }

    pub async fn ban_chat_member(
        &self,
        peer: PeerRef,
        user: PeerRef,
        until: Option<i64>,
    ) -> ApiResult<()> {
        boundary(
            "ban_chat_member",
            self.native().ban_chat_member(peer, user, until),
        )
        .await
    }
    pub async fn kick_chat_member(
        &self,
        peer: PeerRef,
        user: PeerRef,
        until: Option<i64>,
    ) -> ApiResult<()> {
        boundary(
            "kick_chat_member",
            self.native().kick_chat_member(peer, user, until),
        )
        .await
    }
    pub async fn unban_chat_member(&self, peer: PeerRef, user: PeerRef) -> ApiResult<()> {
        boundary(
            "unban_chat_member",
            self.native().unban_chat_member(peer, user),
        )
        .await
    }
    pub async fn restrict_chat_member(
        &self,
        peer: PeerRef,
        user: PeerRef,
        permissions: &crate::moderation::ChatPermissions,
        until: Option<i64>,
    ) -> ApiResult<()> {
        boundary(
            "restrict_chat_member",
            self.native()
                .restrict_chat_member(peer, user, permissions, until),
        )
        .await
    }
    pub async fn leave_chat(&self, peer: PeerRef) -> ApiResult<()> {
        boundary("leave_chat", self.native().leave_chat(peer)).await
    }
    pub async fn delete_message(&self, peer: PeerRef, id: i32) -> ApiResult<()> {
        boundary("delete_message", self.native().delete_message(peer, id)).await
    }
    pub async fn delete_messages(&self, peer: PeerRef, ids: &[i32]) -> ApiResult<()> {
        boundary("delete_messages", self.native().delete_messages(peer, ids)).await
    }
    pub async fn pin_chat_message(&self, peer: PeerRef, id: i32, silent: bool) -> ApiResult<()> {
        boundary("pin_chat_message", async {
            acknowledged(peer, id)?;
            self.native().pin_chat_message(peer, id, silent).await
        })
        .await
    }
    pub async fn unpin_chat_message(&self, peer: PeerRef, id: i32) -> ApiResult<()> {
        boundary("unpin_chat_message", async {
            acknowledged(peer, id)?;
            self.native().unpin_chat_message(peer, id).await
        })
        .await
    }
    pub async fn unpin_all_chat_messages(
        &self,
        peer: PeerRef,
        topic: Option<i32>,
    ) -> ApiResult<()> {
        boundary(
            "unpin_all_chat_messages",
            self.native().unpin_all_chat_messages(peer, topic),
        )
        .await
    }
    pub async fn send_chat_action(
        &self,
        peer: PeerRef,
        action: tl::enums::SendMessageAction,
        topic: Option<i32>,
    ) -> ApiResult<()> {
        boundary("send_chat_action", async {
            anyhow::ensure!(topic.is_none_or(|id| id > 0), "thread ID must be positive");
            self.native().send_chat_action(peer, action, topic).await
        })
        .await
    }
    pub async fn send_chat_action_typed(
        &self,
        peer: PeerRef,
        action: crate::bot_requests::ChatAction,
        topic: Option<i32>,
    ) -> ApiResult<()> {
        boundary(
            "send_chat_action_typed",
            self.native().send_chat_action_typed(peer, action, topic),
        )
        .await
    }
    pub async fn set_my_commands(
        &self,
        commands: &[(String, String)],
        scope: tl::enums::BotCommandScope,
        language: &str,
    ) -> ApiResult<()> {
        boundary(
            "set_my_commands",
            self.native().set_my_commands(commands, scope, language),
        )
        .await
    }
    pub async fn get_my_commands(
        &self,
        scope: tl::enums::BotCommandScope,
        language: &str,
    ) -> ApiResult<Vec<crate::botapi::BotCommand>> {
        boundary("get_my_commands", async {
            Ok(self
                .native()
                .get_my_commands(scope, language)
                .await?
                .into_iter()
                .map(|(command, description)| crate::botapi::BotCommand {
                    command,
                    description,
                })
                .collect())
        })
        .await
    }
    pub async fn delete_my_commands(
        &self,
        scope: tl::enums::BotCommandScope,
        language: &str,
    ) -> ApiResult<()> {
        boundary(
            "delete_my_commands",
            self.native().delete_my_commands(scope, language),
        )
        .await
    }
    pub async fn answer_callback_query(
        &self,
        id: i64,
        text: Option<&str>,
        alert: bool,
    ) -> ApiResult<()> {
        boundary(
            "answer_callback_query",
            self.native().answer_callback_query_with_options(
                id,
                &crate::bot_requests::CallbackAnswer {
                    text: text.map(str::to_owned),
                    show_alert: alert,
                    ..Default::default()
                },
            ),
        )
        .await
    }
    pub async fn answer_callback_query_with_options(
        &self,
        id: i64,
        options: &crate::bot_requests::CallbackAnswer,
    ) -> ApiResult<()> {
        boundary(
            "answer_callback_query_with_options",
            self.native()
                .answer_callback_query_with_options(id, options),
        )
        .await
    }
    pub async fn answer_pre_checkout_query(
        &self,
        id: i64,
        success: bool,
        error: Option<&str>,
    ) -> ApiResult<()> {
        boundary(
            "answer_pre_checkout_query",
            self.native().answer_pre_checkout_query(id, success, error),
        )
        .await
    }
    pub async fn answer_shipping_query(
        &self,
        id: i64,
        options: Option<Vec<tl::enums::ShippingOption>>,
        error: Option<&str>,
    ) -> ApiResult<()> {
        boundary(
            "answer_shipping_query",
            self.native().answer_shipping_query(id, options, error),
        )
        .await
    }
    pub async fn answer_inline_query(
        &self,
        query: &grammers_client::update::InlineQuery,
        results: &[crate::botapi::InlineResult],
        cache: i32,
    ) -> ApiResult<()> {
        boundary(
            "answer_inline_query",
            self.native().answer_inline_query(query, results, cache),
        )
        .await
    }
    pub async fn answer_inline_query_with_options(
        &self,
        id: i64,
        results: &[crate::botapi::InlineResult],
        options: &crate::bot_requests::InlineAnswer,
    ) -> ApiResult<()> {
        boundary(
            "answer_inline_query_with_options",
            self.native()
                .answer_inline_query_with_options(id, results, options),
        )
        .await
    }
    pub async fn answer_guest_query(
        &self,
        query: &grammers_client::update::GuestChatQuery,
        result: &crate::botapi::InlineResult,
    ) -> ApiResult<SentGuestMessage> {
        boundary("answer_guest_query", async {
            Ok(SentGuestMessage {
                inline_message_id: self.native().answer_guest_query(query, result).await?,
            })
        })
        .await
    }
    pub async fn answer_web_app_query(
        &self,
        id: &str,
        result: &crate::botapi::InlineResult,
    ) -> ApiResult<crate::botapi::SentWebAppMessage> {
        boundary(
            "answer_web_app_query",
            self.native().answer_web_app_query(id, result),
        )
        .await
    }
    pub async fn download_file(&self, id: &str, limit: usize) -> ApiResult<Vec<u8>> {
        boundary("download_file", self.native().download_file(id, limit)).await
    }
    pub async fn download_to_path(
        &self,
        id: &str,
        path: &std::path::Path,
        limit: u64,
    ) -> ApiResult<u64> {
        boundary(
            "download_to_path",
            self.native().download_to_path(id, path, limit),
        )
        .await
    }
    pub async fn download_file_to_writer<W: tokio::io::AsyncWrite + Unpin>(
        &self,
        id: &str,
        writer: &mut W,
        limit: u64,
    ) -> ApiResult<u64> {
        boundary(
            "download_file_to_writer",
            self.native().download_file_to_writer(id, writer, limit),
        )
        .await
    }
    pub async fn download(&self, message: &Message, limit: usize) -> ApiResult<Vec<u8>> {
        boundary("download", self.native().download(message, limit)).await
    }
}
