//! Explicit native compatibility layer. Use bot::Bot for stable serializable results and ApiError.
use anyhow::{Context as _, Result};
use grammers_client::{
    Client,
    message::{InputMessage, Message},
    tl,
    update::CallbackQuery,
};
use grammers_session::types::PeerRef;
#[derive(Clone)]
pub struct NativeBot {
    pub client: Client,
    pub id: i64,
    pub username: String,
    /// Account's peer cache, shared with the sender pool. Needed for numeric Bot API IDs.
    pub session: Option<std::sync::Arc<grammers_session::storages::ErasedSession>>,
}
impl NativeBot {
    async fn resolve_entities(
        &self,
        entities: &mut Option<Vec<tl::enums::MessageEntity>>,
    ) -> Result<()> {
        if let Some(entities) = entities {
            for entity in entities {
                if let tl::enums::MessageEntity::MentionName(e) = entity {
                    let user = self.resolve_chat(&e.user_id.to_string()).await?;
                    *entity = tl::types::InputMessageEntityMentionName {
                        offset: e.offset,
                        length: e.length,
                        user_id: user.into(),
                    }
                    .into();
                }
            }
        }
        Ok(())
    }
    pub async fn command_scope(
        &self,
        scope: &crate::bot_requests::CommandScope,
    ) -> Result<tl::enums::BotCommandScope> {
        use crate::bot_requests::CommandScope as S;
        Ok(match scope {
            S::Default => tl::enums::BotCommandScope::Default,
            S::AllPrivateChats => tl::enums::BotCommandScope::Users,
            S::AllGroupChats => tl::enums::BotCommandScope::Chats,
            S::AllChatAdministrators => tl::enums::BotCommandScope::ChatAdmins,
            S::Chat { chat_id } => tl::types::BotCommandScopePeer {
                peer: self.resolve_chat(chat_id).await?.into(),
            }
            .into(),
            S::ChatAdministrators { chat_id } => tl::types::BotCommandScopePeerAdmins {
                peer: self.resolve_chat(chat_id).await?.into(),
            }
            .into(),
            S::ChatMember { chat_id, user_id } => {
                let user = self.resolve_chat(user_id).await?;
                anyhow::ensure!(
                    user.id.kind() == grammers_session::types::PeerKind::User,
                    "command scope member must be user"
                );
                tl::types::BotCommandScopePeerUser {
                    peer: self.resolve_chat(chat_id).await?.into(),
                    user_id: user.into(),
                }
                .into()
            }
        })
    }
    pub async fn send_chat_action_typed(
        &self,
        peer: PeerRef,
        action: crate::bot_requests::ChatAction,
        topic: Option<i32>,
    ) -> Result<()> {
        anyhow::ensure!(topic.is_none_or(|n| n > 0), "thread ID must be positive");
        self.send_chat_action(peer, action.to_tl(), topic).await
    }
    pub async fn download_file_to_writer<W: tokio::io::AsyncWrite + Unpin>(
        &self,
        file_id: &str,
        writer: &mut W,
        limit: u64,
    ) -> Result<u64> {
        use tokio::io::AsyncWriteExt;
        let reference = crate::file_id::FileRef::decode(file_id)?;
        let mut download = self.client.iter_download(&reference);
        let mut count = 0u64;
        while let Some(chunk) =
            tokio::time::timeout(std::time::Duration::from_secs(30), download.next()).await??
        {
            count = count
                .checked_add(chunk.len() as u64)
                .context("download size overflow")?;
            anyhow::ensure!(count <= limit, "download limit exceeded");
            writer.write_all(&chunk).await?;
        }
        writer.flush().await?;
        Ok(count)
    }
    pub async fn get_chat_full_info(&self, peer: PeerRef) -> Result<crate::botapi::ChatFullInfo> {
        let entity = self.client.resolve_peer(peer).await?;
        let chat = crate::botapi::Chat::from_peer(&entity);
        let (protected, color) = match &entity {
            grammers_client::peer::Peer::User(_) => (None, None),
            grammers_client::peer::Peer::Channel(c) => (
                Some(c.raw.noforwards),
                c.raw.color.as_ref().and_then(|c| {
                    if let tl::enums::PeerColor::Color(c) = c {
                        c.color
                    } else {
                        None
                    }
                }),
            ),
            grammers_client::peer::Peer::Group(g) => match &g.raw {
                tl::enums::Chat::Chat(c) => (Some(c.noforwards), None),
                tl::enums::Chat::Channel(c) => (
                    Some(c.noforwards),
                    c.color.as_ref().and_then(|c| {
                        if let tl::enums::PeerColor::Color(c) = c {
                            c.color
                        } else {
                            None
                        }
                    }),
                ),
                _ => (None, None),
            },
            _ => (None, None),
        };
        let (description, pinned, ttl, details) = match peer.id.kind() {
            grammers_session::types::PeerKind::User => {
                let tl::enums::users::UserFull::Full(result) = self
                    .client
                    .invoke(&tl::functions::users::GetFullUser { id: peer.into() })
                    .await?;
                let tl::enums::UserFull::Full(u) = result.full_user;
                (
                    u.about,
                    u.pinned_msg_id,
                    u.ttl_period,
                    serde_json::json!({"blocked":u.blocked,"has_protected_content":u.noforwards_peer_enabled,"has_private_forwards":u.private_forward_name.is_some(),"has_restricted_voice_and_video_messages":u.voice_messages_forbidden,"photo":u.profile_photo.as_ref().and_then(crate::rare_payload::photo),"personal_chat_id":u.personal_channel_id.and_then(grammers_session::types::PeerId::channel).map(|id|id.bot_api_dialog_id_unchecked()),"birthday":u.birthday.as_ref().map(|b|{let tl::enums::Birthday::Birthday(b)=b;serde_json::json!({"day":b.day,"month":b.month,"year":b.year})})}),
                )
            }
            _ => {
                let response = if peer.id.kind() == grammers_session::types::PeerKind::Channel {
                    self.client
                        .invoke(&tl::functions::channels::GetFullChannel {
                            channel: peer.into(),
                        })
                        .await?
                } else {
                    self.client
                        .invoke(&tl::functions::messages::GetFullChat {
                            chat_id: peer.id.bare_id_unchecked(),
                        })
                        .await?
                };
                let tl::enums::messages::ChatFull::Full(result) = response;
                match result.full_chat {
                    tl::enums::ChatFull::CommunityFull(_) => anyhow::bail!(
                        "community full-info is not represented by the original Bot API chat facade"
                    ),
                    tl::enums::ChatFull::Full(c) => (
                        Some(c.about),
                        c.pinned_msg_id,
                        c.ttl_period,
                        serde_json::json!({"photo":c.chat_photo.as_ref().and_then(crate::rare_payload::photo),"invite_link":c.exported_invite.as_ref().map(crate::bot_events::invite),"pending_join_request_count":c.requests_pending}),
                    ),
                    tl::enums::ChatFull::ChannelFull(c) => (
                        Some(c.about),
                        c.pinned_msg_id,
                        c.ttl_period,
                        serde_json::json!({"photo":crate::rare_payload::photo(&c.chat_photo),"invite_link":c.exported_invite.as_ref().map(crate::bot_events::invite),"member_count":c.participants_count,"administrator_count":c.admins_count,"slow_mode_delay":c.slowmode_seconds,"linked_chat_id":c.linked_chat_id.and_then(grammers_session::types::PeerId::channel).map(|id|id.bot_api_dialog_id_unchecked()),"has_hidden_members":c.participants_hidden,"has_aggressive_anti_spam_enabled":c.antispam,"available_reactions_limit":c.reactions_limit,"boosts_applied":c.boosts_applied}),
                    ),
                }
            }
        };
        let mut result = crate::botapi::ChatFullInfo {
            chat,
            description,
            pinned_message_id: pinned,
            message_auto_delete_time: ttl,
            details,
            ..Default::default()
        };
        result.promote_details();
        result.has_protected_content = result.has_protected_content.or(protected);
        result.accent_color_id = color;
        Ok(result)
    }
    pub async fn get_file_info(&self, id: &str) -> Result<crate::botapi::File> {
        let file = crate::file_id::FileRef::decode(id)?;
        Ok(crate::botapi::File {
            file_id: id.into(),
            file_unique_id: file.unique_id(),
            file_size: None,
            file_path: id.into(),
            file_name: None,
            mime_type: None,
        })
    }
    pub async fn edit_with_options(
        &self,
        peer: PeerRef,
        id: i32,
        edit: &crate::bot_requests::Edit,
        keyboard: Option<&crate::markup::Keyboard>,
        business: Option<&str>,
    ) -> Result<crate::bot_response::SentMessage> {
        crate::bot_requests::business(business)?;
        anyhow::ensure!(id > 0, "message ID must be positive");
        let mut fields = edit.fields(keyboard)?;
        self.resolve_entities(&mut fields.entities).await?;
        if let crate::bot_requests::Edit::Media { media, .. } = edit {
            fields.media = Some(media.prepare(&self.client).await?);
        }
        let result = self
            .invoke_business("edit_message", fields.chat(peer, id)?, business)
            .await?;
        crate::bot_response::edited(&result, peer, id)
    }
    pub async fn edit_inline_with_options(
        &self,
        id: &str,
        edit: &crate::bot_requests::Edit,
        keyboard: Option<&crate::markup::Keyboard>,
    ) -> Result<()> {
        let id = crate::file_id::decode_inline_id(id)?;
        let mut fields = edit.fields(keyboard)?;
        self.resolve_entities(&mut fields.entities).await?;
        if let crate::bot_requests::Edit::Media { media, .. } = edit {
            let mut prepared = media.prepare(&self.client).await?;
            if !matches!(
                prepared,
                tl::enums::InputMedia::Photo(_) | tl::enums::InputMedia::Document(_)
            ) {
                let stored = self
                    .client
                    .invoke(&tl::functions::messages::UploadMedia {
                        business_connection_id: None,
                        peer: tl::enums::InputPeer::PeerSelf,
                        media: prepared,
                    })
                    .await?;
                prepared = crate::bot_requests::stored_media(stored)?;
            }
            match &mut prepared {
                tl::enums::InputMedia::Photo(p) => p.spoiler = media.spoiler,
                tl::enums::InputMedia::Document(d) => d.spoiler = media.spoiler,
                _ => {}
            }
            fields.media = Some(prepared);
        }
        self.invoke_inline_dc(id.dc_id(), &fields.inline(id.clone()))
            .await?;
        Ok(())
    }
    /// Resolve Bot API dialog IDs or @usernames without guessing an access hash.
    pub async fn resolve_chat(&self, value: &str) -> Result<PeerRef> {
        if let Ok(id) = value.trim().parse::<i64>() {
            let id = grammers_session::types::PeerId::from_bot_api_dialog_id(id)
                .context("invalid Bot API chat ID")?;
            if id.kind() == grammers_session::types::PeerKind::Chat {
                return Ok(id.to_ambient_ref());
            }
            let session=self.session.as_ref().context("numeric user/channel lookup needs the sender-pool session; supply PeerRef or username")?;
            return session
                .peer_ref(id)
                .await
                .map_err(anyhow::Error::from_boxed)?
                .context("numeric user/channel access hash not cached; use username or PeerRef");
        }
        let name = value.trim().trim_start_matches('@');
        anyhow::ensure!(
            (1..=64).contains(&name.len())
                && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'),
            "invalid chat username"
        );
        self.client
            .resolve_username(name)
            .await?
            .context("chat username not found")?
            .to_ref()
            .await
            .map_err(anyhow::Error::from_boxed)?
            .context("resolved peer has no usable reference")
    }
    pub async fn invoke_business<R: tl::RemoteCall>(
        &self,
        method: &str,
        request: R,
        connection_id: Option<&str>,
    ) -> std::result::Result<R::Return, crate::botapi::ApiError> {
        crate::bot_requests::business(connection_id)
            .map_err(|e| crate::botapi::ApiError::validation(method, e.to_string()))?;
        if let Some(connection_id) = connection_id {
            self.invoke_api(
                method,
                &tl::functions::InvokeWithBusinessConnection {
                    connection_id: connection_id.into(),
                    query: request,
                },
            )
            .await
        } else {
            self.invoke_api(method, &request).await
        }
    }
    pub async fn send_message_with_options(
        &self,
        peer: PeerRef,
        text: &crate::bot_requests::Text,
        rich: Option<&crate::bot_edits::RichText>,
        preview: &crate::bot_requests::Preview,
        options: &crate::bot_requests::SendOptions,
    ) -> Result<crate::bot_response::SentMessage> {
        let random = rand::random();
        let request = crate::bot_requests::send(peer, text, rich, preview, options, random)?;
        let (result, message, markup) = match request {
            crate::bot_requests::SendRequest::Text(mut r) => {
                self.resolve_entities(&mut r.entities).await?;
                let message = r.message.clone();
                let markup = r.reply_markup.clone();
                (
                    self.invoke_business(
                        "send_message",
                        r,
                        options.business_connection_id.as_deref(),
                    )
                    .await?,
                    message,
                    markup,
                )
            }
            crate::bot_requests::SendRequest::Media(mut r) => {
                self.resolve_entities(&mut r.entities).await?;
                let message = r.message.clone();
                let markup = r.reply_markup.clone();
                (
                    self.invoke_business(
                        "send_message",
                        r,
                        options.business_connection_id.as_deref(),
                    )
                    .await?,
                    message,
                    markup,
                )
            }
        };
        crate::bot_response::sent(&result, peer, random, &message, markup.as_ref())
    }
    pub async fn send_media_with_options(
        &self,
        peer: PeerRef,
        media: &crate::media::Media,
        caption: &crate::bot_requests::Text,
        above: bool,
        options: &crate::bot_requests::SendOptions,
    ) -> Result<crate::bot_response::SentMessage> {
        options.validate()?;
        media.validate()?;
        let (message, mut entities) = caption.parse(1024, true)?;
        self.resolve_entities(&mut entities).await?;
        let media = media.prepare(&self.client).await?;
        let random = rand::random();
        let request = crate::bot_requests::send_media(
            peer,
            media,
            message.clone(),
            entities,
            above,
            options,
            random,
        )?;
        let markup = request.reply_markup.clone();
        let result = self
            .invoke_business(
                "send_media",
                request,
                options.business_connection_id.as_deref(),
            )
            .await?;
        crate::bot_response::sent(&result, peer, random, &message, markup.as_ref())
    }
    pub async fn forward_message_with_options(
        &self,
        to: PeerRef,
        from: PeerRef,
        id: i32,
        copy: bool,
        options: &crate::bot_requests::SendOptions,
    ) -> Result<crate::bot_response::SentMessage> {
        let random = rand::random();
        let request = crate::bot_requests::forward(from, to, id, copy, options, random)?;
        let result = self
            .invoke_business(
                if copy {
                    "copy_message"
                } else {
                    "forward_message"
                },
                request,
                options.business_connection_id.as_deref(),
            )
            .await?;
        crate::bot_response::sent(&result, to, random, "", None)
    }
    pub async fn send_media_group_with_options(
        &self,
        peer: PeerRef,
        items: &[crate::bot_requests::AlbumItem],
        options: &crate::bot_requests::SendOptions,
    ) -> Result<Vec<crate::bot_response::SentMessage>> {
        crate::bot_requests::validate_album(items, options)?;
        let mut values = vec![];
        let mut ids = vec![];
        for item in items {
            let mut media = item.media.prepare(&self.client).await?;
            if !matches!(
                media,
                tl::enums::InputMedia::Photo(_) | tl::enums::InputMedia::Document(_)
            ) {
                let uploaded = self
                    .invoke_business(
                        "send_media_group",
                        tl::functions::messages::UploadMedia {
                            business_connection_id: options.business_connection_id.clone(),
                            peer: peer.into(),
                            media,
                        },
                        None,
                    )
                    .await?;
                media = crate::bot_requests::stored_media(uploaded)?;
                match &mut media {
                    tl::enums::InputMedia::Photo(p) => p.spoiler = item.media.spoiler,
                    tl::enums::InputMedia::Document(d) => d.spoiler = item.media.spoiler,
                    _ => {}
                }
            }
            let (message, mut entities) = item.caption.parse(1024, true)?;
            self.resolve_entities(&mut entities).await?;
            let random_id = rand::random();
            ids.push(random_id);
            values.push(
                tl::types::InputSingleMedia {
                    media,
                    random_id,
                    message,
                    entities,
                }
                .into(),
            );
        }
        let result = self
            .invoke_business(
                "send_media_group",
                crate::bot_requests::album(peer, values, options)?,
                options.business_connection_id.as_deref(),
            )
            .await?;
        crate::bot_response::album(&result, peer, &ids)
    }
    pub async fn answer_callback_query_with_options(
        &self,
        id: i64,
        options: &crate::bot_requests::CallbackAnswer,
    ) -> Result<()> {
        self.invoke_api("answer_callback_query", &options.request(id)?)
            .await?;
        Ok(())
    }
    pub async fn answer_inline_query_with_options(
        &self,
        id: i64,
        results: &[crate::botapi::InlineResult],
        options: &crate::bot_requests::InlineAnswer,
    ) -> Result<()> {
        self.invoke_api("answer_inline_query", &options.request(id, results)?)
            .await?;
        Ok(())
    }
    pub async fn delete_message(&self, peer: PeerRef, id: i32) -> Result<()> {
        anyhow::ensure!(id > 0, "message ID must be positive");
        self.delete_messages(peer, &[id]).await
    }
    pub async fn unpin_all_chat_messages(&self, peer: PeerRef, topic: Option<i32>) -> Result<()> {
        anyhow::ensure!(topic.is_none_or(|n| n > 0), "thread ID must be positive");
        self.invoke_api(
            "unpin_chat_message",
            &tl::functions::messages::UnpinAllMessages {
                peer: peer.into(),
                top_msg_id: topic,
                saved_peer_id: None,
            },
        )
        .await?;
        Ok(())
    }
    pub async fn me(&self) -> Result<crate::botapi::User> {
        self.get_me().await
    }
    /// Other-DC inline messages require both an encrypted transport key and the
    /// bot's exported authorization; a fresh sender may have only the first.
    async fn invoke_inline_dc<R: tl::RemoteCall>(&self, dc: i32, request: &R) -> Result<R::Return> {
        match self.client.invoke_in_dc(dc, request).await {
            Ok(result) => Ok(result),
            Err(grammers_client::InvocationError::Rpc(error))
                if error.name == "AUTH_KEY_UNREGISTERED" =>
            {
                let tl::enums::auth::ExportedAuthorization::Authorization(auth) = self
                    .client
                    .invoke(&tl::functions::auth::ExportAuthorization { dc_id: dc })
                    .await?;
                self.client
                    .invoke_in_dc(
                        dc,
                        &tl::functions::auth::ImportAuthorization {
                            id: auth.id,
                            bytes: auth.bytes,
                        },
                    )
                    .await?;
                Ok(self.client.invoke_in_dc(dc, request).await?)
            }
            Err(error) => Err(error.into()),
        }
    }
    pub async fn get_me(&self) -> Result<crate::botapi::User> {
        let user = self.client.get_me().await?;
        crate::botapi::User::from_tl(&user.raw).context("Telegram returned no user")
    }
    pub async fn get_chat(&self, peer: PeerRef) -> Result<crate::botapi::Chat> {
        let peer = self.client.resolve_peer(peer).await?;
        Ok(match &peer {
            grammers_client::peer::Peer::User(u) => crate::botapi::Chat::from_user(
                &crate::botapi::User::from_tl(&u.raw).context("user missing")?,
            ),
            grammers_client::peer::Peer::Group(g) => crate::botapi::Chat::from_tl(&g.raw),
            grammers_client::peer::Peer::Channel(c) => {
                crate::botapi::Chat::from_tl(&c.raw.clone().into())
            }
            grammers_client::peer::Peer::Community(_) => {
                anyhow::bail!("community is not represented by Bot API Chat")
            }
        })
    }
    pub async fn get_chat_member(
        &self,
        peer: PeerRef,
        user: PeerRef,
    ) -> Result<grammers_client::client::ParticipantPermissions> {
        Ok(self.client.get_permissions(peer, user).await?)
    }
    /// Serializable member object. The existing get_chat_member retains its native return type.
    pub async fn get_chat_member_info(
        &self,
        chat: PeerRef,
        user: PeerRef,
    ) -> Result<crate::botapi::ChatMember> {
        use grammers_session::types::PeerKind;
        anyhow::ensure!(
            matches!(chat.id.kind(), PeerKind::Chat | PeerKind::Channel),
            "get_chat_member_info requires a group or channel"
        );
        anyhow::ensure!(
            user.id.kind() == PeerKind::User,
            "member target must be a user"
        );
        let entity = self.client.resolve_peer(user).await?;
        let grammers_client::peer::Peer::User(entity) = entity else {
            anyhow::bail!("member target is not a user");
        };
        let id = entity.id().bare_id_unchecked();
        let info = crate::botapi::User::from_tl(&entity.raw).context("member user missing")?;
        if chat.id.kind() == PeerKind::Channel {
            let tl::enums::channels::ChannelParticipant::Participant(result) = self
                .client
                .invoke(&tl::functions::channels::GetParticipant {
                    channel: chat.into(),
                    participant: user.into(),
                })
                .await?;
            crate::botapi::ChatMember::from_channel(info, &result.participant)
        } else {
            let tl::enums::messages::ChatFull::Full(result) = self
                .client
                .invoke(&tl::functions::messages::GetFullChat {
                    chat_id: chat.id.bare_id_unchecked(),
                })
                .await?;
            if let tl::enums::ChatFull::Full(full) = result.full_chat {
                let participant = match full.participants {
                    tl::enums::ChatParticipants::Participants(p) => {
                        p.participants.into_iter().find(|p| p.user_id() == id)
                    }
                    tl::enums::ChatParticipants::Forbidden(p) => {
                        p.self_participant.filter(|p| p.user_id() == id)
                    }
                };
                if let Some(participant) = participant {
                    return crate::botapi::ChatMember::from_group(info, &participant);
                }
            }
            anyhow::bail!("participant not available in Telegram response")
        }
    }
    /// Ban (not kick-then-unban). `until_date` is an absolute Unix timestamp; None/0 is forever.
    pub async fn ban_chat_member(
        &self,
        chat: PeerRef,
        user: PeerRef,
        until_date: Option<i64>,
    ) -> Result<()> {
        let request = crate::moderation::edit_request(
            chat,
            user,
            crate::moderation::ban_rights(until_date)?,
        )?;
        self.client.invoke(&request).await?;
        Ok(())
    }
    /// Original Bot facade alias: this leaves the ban in place, unlike Client::kick_participant.
    pub async fn kick_chat_member(
        &self,
        chat: PeerRef,
        user: PeerRef,
        until_date: Option<i64>,
    ) -> Result<()> {
        self.ban_chat_member(chat, user, until_date).await
    }
    /// Clears individual restrictions; does not add the user back or override chat defaults.
    pub async fn unban_chat_member(&self, chat: PeerRef, user: PeerRef) -> Result<()> {
        let request =
            crate::moderation::edit_request(chat, user, crate::moderation::unrestricted())?;
        self.client.invoke(&request).await?;
        Ok(())
    }
    pub async fn restrict_chat_member(
        &self,
        chat: PeerRef,
        user: PeerRef,
        permissions: &crate::moderation::ChatPermissions,
        until_date: Option<i64>,
    ) -> Result<()> {
        let request =
            crate::moderation::edit_request(chat, user, permissions.to_banned_rights(until_date)?)?;
        self.client.invoke(&request).await?;
        Ok(())
    }
    pub async fn leave_chat(&self, chat: PeerRef) -> Result<()> {
        match crate::moderation::leave_request(chat)? {
            crate::moderation::LeaveRequest::Channel(request) => {
                self.client.invoke(&request).await?;
            }
            crate::moderation::LeaveRequest::Group(request) => {
                self.client.invoke(&request).await?;
            }
        }
        Ok(())
    }
    pub async fn send_video_note(
        &self,
        peer: PeerRef,
        source: crate::media::Source,
        duration: f64,
        size: i32,
    ) -> Result<Message> {
        self.send_media(
            peer,
            &crate::media::Media {
                kind: crate::file_id::FileKind::VideoNote,
                source,
                duration,
                width: size,
                height: size,
                ..Default::default()
            },
            InputMessage::new(),
        )
        .await
    }
    pub async fn set_my_commands(
        &self,
        commands: &[(String, String)],
        scope: tl::enums::BotCommandScope,
        language: &str,
    ) -> Result<()> {
        anyhow::ensure!(commands.len() <= 100, "too many bot commands");
        anyhow::ensure!(
            language.is_empty()
                || language.len() == 2 && language.bytes().all(|b| b.is_ascii_lowercase()),
            "language must be empty or ISO 639-1 lowercase code"
        );
        let mut values = vec![];
        for (command, description) in commands {
            let command = command.trim_start_matches('/');
            anyhow::ensure!(
                !command.is_empty()
                    && command.len() <= 32
                    && command
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'),
                "invalid bot command"
            );
            anyhow::ensure!(
                !description.is_empty() && description.chars().count() <= 256,
                "invalid bot command description"
            );
            values.push(
                tl::types::BotCommand {
                    ephemeral: false,
                    command: command.into(),
                    description: description.clone(),
                }
                .into(),
            );
        }
        self.client
            .invoke(&tl::functions::bots::SetBotCommands {
                scope,
                lang_code: language.into(),
                commands: values,
            })
            .await?;
        Ok(())
    }
    pub async fn get_my_commands(
        &self,
        scope: tl::enums::BotCommandScope,
        language: &str,
    ) -> Result<Vec<(String, String)>> {
        anyhow::ensure!(
            language.is_empty()
                || language.len() == 2 && language.bytes().all(|b| b.is_ascii_lowercase()),
            "invalid bot command language"
        );
        let commands = self
            .client
            .invoke(&tl::functions::bots::GetBotCommands {
                scope,
                lang_code: language.into(),
            })
            .await?;
        Ok(commands
            .into_iter()
            .map(|c| {
                let tl::enums::BotCommand::Command(c) = c;
                (c.command, c.description)
            })
            .collect())
    }
    pub async fn delete_my_commands(
        &self,
        scope: tl::enums::BotCommandScope,
        language: &str,
    ) -> Result<()> {
        anyhow::ensure!(
            language.is_empty()
                || language.len() == 2 && language.bytes().all(|b| b.is_ascii_lowercase()),
            "invalid bot command language"
        );
        self.client
            .invoke(&tl::functions::bots::ResetBotCommands {
                scope,
                lang_code: language.into(),
            })
            .await?;
        Ok(())
    }
    pub async fn send_chat_action(
        &self,
        peer: PeerRef,
        action: tl::enums::SendMessageAction,
        topic: Option<i32>,
    ) -> Result<()> {
        self.client
            .invoke(&tl::functions::messages::SetTyping {
                peer: peer.into(),
                top_msg_id: topic,
                action,
            })
            .await?;
        Ok(())
    }
    pub async fn pin_chat_message(&self, peer: PeerRef, id: i32, silent: bool) -> Result<()> {
        self.client
            .invoke(&tl::functions::messages::UpdatePinnedMessage {
                silent,
                unpin: false,
                pm_oneside: false,
                peer: peer.into(),
                id,
            })
            .await?;
        Ok(())
    }
    pub async fn unpin_chat_message(&self, peer: PeerRef, id: i32) -> Result<()> {
        self.client
            .invoke(&tl::functions::messages::UpdatePinnedMessage {
                silent: true,
                unpin: true,
                pm_oneside: false,
                peer: peer.into(),
                id,
            })
            .await?;
        Ok(())
    }
    pub async fn answer_callback_query(
        &self,
        id: i64,
        text: Option<&str>,
        alert: bool,
    ) -> Result<()> {
        self.client
            .invoke(&tl::functions::messages::SetBotCallbackAnswer {
                alert,
                query_id: id,
                message: text.map(str::to_owned),
                url: None,
                cache_time: 0,
            })
            .await?;
        Ok(())
    }
    pub async fn answer_pre_checkout_query(
        &self,
        id: i64,
        success: bool,
        error: Option<&str>,
    ) -> Result<()> {
        anyhow::ensure!(
            success || error.is_some_and(|s| !s.is_empty()),
            "failed checkout needs an error"
        );
        self.client
            .invoke(&tl::functions::messages::SetBotPrecheckoutResults {
                success,
                query_id: id,
                error: error.map(str::to_owned),
            })
            .await?;
        Ok(())
    }
    pub async fn answer_shipping_query(
        &self,
        id: i64,
        options: Option<Vec<tl::enums::ShippingOption>>,
        error: Option<&str>,
    ) -> Result<()> {
        anyhow::ensure!(
            options.is_some() ^ error.is_some(),
            "provide shipping options or error"
        );
        self.client
            .invoke(&tl::functions::messages::SetBotShippingResults {
                query_id: id,
                error: error.map(str::to_owned),
                shipping_options: options,
            })
            .await?;
        Ok(())
    }
    pub async fn send_cached_photo(
        &self,
        db: &crate::database::Database,
        peer: PeerRef,
        bytes: Vec<u8>,
        caption: &str,
    ) -> Result<Message> {
        let key = crate::assets::cache_key(self.id, &bytes);
        if let Some(id) = crate::assets::cached(db, &key) {
            let media = crate::media::Media {
                kind: crate::file_id::FileKind::Photo,
                source: crate::media::Source::FileId { file_id: id },
                ..Default::default()
            };
            match self
                .send_media(peer, &media, InputMessage::new().text(caption))
                .await
            {
                Ok(message) => return Ok(message),
                Err(error) => {
                    let retry=error.downcast_ref::<grammers_client::InvocationError>().is_some_and(|e|matches!(e,grammers_client::InvocationError::Rpc(r) if r.name.starts_with("FILE_REFERENCE")||r.name=="FILE_ID_INVALID"||r.name=="PHOTO_INVALID"));
                    if !retry {
                        return Err(error);
                    }
                    crate::assets::forget(db, &key)?;
                }
            }
        }
        let message = self.send_photo(peer, bytes, caption).await?;
        if let Some(media) = message.media() {
            crate::assets::remember(db, &key, &media)?;
        }
        Ok(message)
    }
    pub async fn invoke_api<R: tl::RemoteCall>(
        &self,
        method: &str,
        request: &R,
    ) -> std::result::Result<R::Return, crate::botapi::ApiError> {
        self.client
            .invoke(request)
            .await
            .map_err(|e| crate::botapi::ApiError::from_rpc(method, &e))
    }
    pub async fn answer_inline_query(
        &self,
        query: &grammers_client::update::InlineQuery,
        results: &[crate::botapi::InlineResult],
        cache_seconds: i32,
    ) -> Result<()> {
        anyhow::ensure!(
            (0..=86400).contains(&cache_seconds),
            "invalid inline cache duration"
        );
        query
            .answer(crate::botapi::inline_results(results)?)
            .private()
            .cache_time(cache_seconds)
            .send()
            .await?;
        Ok(())
    }
    pub async fn answer_guest_query(
        &self,
        query: &grammers_client::update::GuestChatQuery,
        result: &crate::botapi::InlineResult,
    ) -> Result<String> {
        let message = query.answer(result.to_tl()?).await?;
        Ok(crate::file_id::encode_inline_id(&message.raw))
    }
    pub async fn answer_web_app_query(
        &self,
        query_id: &str,
        result: &crate::botapi::InlineResult,
    ) -> Result<crate::botapi::SentWebAppMessage> {
        let sent = self
            .client
            .invoke(&crate::botapi::web_app_request(query_id, result)?)
            .await?;
        Ok(crate::botapi::SentWebAppMessage::from_tl(&sent))
    }
    pub async fn send_media(
        &self,
        peer: PeerRef,
        media: &crate::media::Media,
        caption: InputMessage,
    ) -> Result<Message> {
        let prepared = media.prepare(&self.client).await?;
        Ok(self
            .client
            .send_message(peer, caption.media(prepared))
            .await?)
    }
    pub async fn edit_message_media(
        &self,
        peer: PeerRef,
        id: i32,
        media: &crate::media::Media,
        caption: InputMessage,
    ) -> Result<()> {
        let prepared = media.prepare(&self.client).await?;
        Ok(self
            .client
            .edit_message(peer, id, caption.media(prepared))
            .await?)
    }
    pub async fn send_media_group(
        &self,
        peer: PeerRef,
        items: &[(crate::media::Media, String)],
        reply_to: Option<i32>,
    ) -> Result<Vec<Option<Message>>> {
        crate::media::Media::validate_album(
            &items.iter().map(|(m, _)| m.clone()).collect::<Vec<_>>(),
        )?;
        let mut album = Vec::with_capacity(items.len());
        for (media, caption) in items {
            album.push(
                grammers_client::media::InputMedia::new()
                    .caption(caption)
                    .reply_to(reply_to)
                    .media(media.prepare(&self.client).await?),
            );
        }
        Ok(self.client.send_album(peer, album).await?)
    }
    pub async fn send_video(
        &self,
        peer: PeerRef,
        source: crate::media::Source,
        caption: &str,
        duration: f64,
        width: i32,
        height: i32,
    ) -> Result<Message> {
        self.send_media(
            peer,
            &crate::media::Media {
                kind: crate::file_id::FileKind::Video,
                source,
                duration,
                width,
                height,
                ..Default::default()
            },
            InputMessage::new().text(caption),
        )
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
    ) -> Result<Message> {
        self.send_media(
            peer,
            &crate::media::Media {
                kind: crate::file_id::FileKind::Audio,
                source,
                duration,
                title,
                performer,
                ..Default::default()
            },
            InputMessage::new().text(caption),
        )
        .await
    }
    pub async fn send_voice(
        &self,
        peer: PeerRef,
        source: crate::media::Source,
        caption: &str,
        duration: f64,
    ) -> Result<Message> {
        self.send_media(
            peer,
            &crate::media::Media {
                kind: crate::file_id::FileKind::Voice,
                source,
                duration,
                ..Default::default()
            },
            InputMessage::new().text(caption),
        )
        .await
    }
    pub async fn send_animation(
        &self,
        peer: PeerRef,
        source: crate::media::Source,
        caption: &str,
    ) -> Result<Message> {
        self.send_media(
            peer,
            &crate::media::Media {
                kind: crate::file_id::FileKind::Animation,
                source,
                ..Default::default()
            },
            InputMessage::new().text(caption),
        )
        .await
    }
    pub async fn send_sticker(
        &self,
        peer: PeerRef,
        source: crate::media::Source,
    ) -> Result<Message> {
        self.send_media(
            peer,
            &crate::media::Media {
                kind: crate::file_id::FileKind::Sticker,
                source,
                ..Default::default()
            },
            InputMessage::new(),
        )
        .await
    }
    pub async fn get_file(&self, file_id: &str) -> Result<crate::file_id::FileRef> {
        crate::file_id::FileRef::decode(file_id)
    }
    pub async fn download_file(&self, file_id: &str, limit: usize) -> Result<Vec<u8>> {
        let file = crate::file_id::FileRef::decode(file_id)?;
        let mut download = self.client.iter_download(&file);
        let mut bytes = vec![];
        while let Some(chunk) =
            tokio::time::timeout(std::time::Duration::from_secs(30), download.next()).await??
        {
            anyhow::ensure!(
                bytes.len() + chunk.len() <= limit,
                "download limit exceeded"
            );
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    }
    pub async fn download_to_path(
        &self,
        file_id: &str,
        path: &std::path::Path,
        limit: u64,
    ) -> Result<u64> {
        use tokio::io::AsyncWriteExt;
        let file = crate::file_id::FileRef::decode(file_id)?;
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| std::path::Path::new("."));
        let temp = tempfile::NamedTempFile::new_in(parent)?;
        let mut output = tokio::fs::File::from_std(temp.reopen()?);
        let mut count = 0u64;
        let mut download = self.client.iter_download(&file);
        while let Some(chunk) =
            tokio::time::timeout(std::time::Duration::from_secs(30), download.next()).await??
        {
            count = count
                .checked_add(chunk.len() as u64)
                .ok_or_else(|| anyhow::anyhow!("download size overflow"))?;
            anyhow::ensure!(count <= limit, "download limit exceeded");
            output.write_all(&chunk).await?;
        }
        output.sync_all().await?;
        drop(output);
        temp.persist(path).map_err(|e| e.error)?;
        Ok(count)
    }
    pub async fn copy_message(&self, peer: PeerRef, source: &Message) -> Result<Message> {
        let from = source
            .peer_ref()
            .await
            .map_err(anyhow::Error::from_boxed)?
            .context("copy source peer missing")?;
        let sent = self
            .forward_message_with_options(
                peer,
                from,
                source.id(),
                true,
                &crate::bot_requests::SendOptions::default(),
            )
            .await?;
        self.client
            .get_messages_by_id(peer, &[sent.message_id])
            .await?
            .into_iter()
            .next()
            .flatten()
            .context("copied message could not be fetched")
    }
    pub async fn forward_message(&self, peer: PeerRef, source: &Message) -> Result<Message> {
        Ok(source.forward_to(peer).await?)
    }
    pub async fn edit_inline_message(
        &self,
        id: &str,
        message: InputMessage,
        media: Option<&crate::media::Media>,
        rich: Option<&str>,
    ) -> Result<()> {
        let id = crate::file_id::decode_inline_id(id)?;
        anyhow::ensure!(
            rich.is_none() || media.is_none(),
            "rich content cannot be combined with media"
        );
        let media = if let Some(media) = media {
            Some(media.prepare(&self.client).await?)
        } else {
            None
        };
        let rich = rich.map(|s| crate::bot_edits::RichText::Html { source: s.into() });
        let request = crate::bot_edits::inline(id.clone(), message, rich.as_ref(), media)?;
        self.invoke_inline_dc(id.dc_id(), &request).await?;
        Ok(())
    }
    pub async fn invoke<R: tl::RemoteCall>(&self, request: &R) -> Result<R::Return> {
        Ok(self.client.invoke(request).await?)
    }
    pub async fn edit_message_text(
        &self,
        peer: PeerRef,
        id: i32,
        message: InputMessage,
    ) -> Result<()> {
        Ok(self.client.edit_message(peer, id, message).await?)
    }
    pub async fn edit_message_rich(
        &self,
        peer: PeerRef,
        id: i32,
        rich: &crate::bot_edits::RichText,
        keyboard: Option<&[Vec<crate::markup::Key>]>,
    ) -> Result<()> {
        let markup = keyboard
            .map(crate::markup::keyboard)
            .transpose()?
            .map(|k| k.raw);
        self.client
            .invoke(&crate::bot_edits::rich(peer, id, rich, markup)?)
            .await?;
        Ok(())
    }
    pub async fn send_rich(
        &self,
        peer: PeerRef,
        rich: &crate::bot_edits::RichText,
        keyboard: Option<&[Vec<crate::markup::Key>]>,
    ) -> Result<Message> {
        let mut input = InputMessage::new()
            .rich_message(grammers_client::message::InputRichMessage { raw: rich.to_tl()? });
        if let Some(keys) = keyboard {
            input = input.reply_markup(crate::markup::keyboard(keys)?);
        }
        Ok(self.client.send_message(peer, input).await?)
    }
    pub async fn edit_inline_caption(
        &self,
        id: &str,
        text: &str,
        html: bool,
        keys: Option<&[Vec<crate::markup::Key>]>,
    ) -> Result<()> {
        let id = crate::file_id::decode_inline_id(id)?;
        let request = crate::bot_edits::inline_caption(id.clone(), text, html, keys)?;
        self.invoke_inline_dc(id.dc_id(), &request).await?;
        Ok(())
    }
    pub async fn edit_inline_reply_markup(
        &self,
        id: &str,
        keys: &[Vec<crate::markup::Key>],
    ) -> Result<()> {
        let id = crate::file_id::decode_inline_id(id)?;
        let request = crate::bot_edits::inline_keyboard(id.clone(), keys)?;
        self.invoke_inline_dc(id.dc_id(), &request).await?;
        Ok(())
    }
    pub async fn edit_message_caption(
        &self,
        peer: PeerRef,
        id: i32,
        text: &str,
        html: bool,
        keyboard: Option<&[Vec<crate::markup::Key>]>,
    ) -> Result<()> {
        self.client
            .invoke(&crate::bot_edits::caption(peer, id, text, html, keyboard)?)
            .await?;
        Ok(())
    }
    pub async fn edit_message_reply_markup(
        &self,
        peer: PeerRef,
        id: i32,
        keys: &[Vec<crate::markup::Key>],
    ) -> Result<()> {
        self.client
            .invoke(&crate::bot_edits::keyboard(peer, id, keys)?)
            .await?;
        Ok(())
    }
    pub async fn delete_messages(&self, peer: PeerRef, ids: &[i32]) -> Result<()> {
        anyhow::ensure!(
            !ids.is_empty() && ids.len() <= 100 && ids.iter().all(|id| *id > 0),
            "delete needs 1–100 positive message IDs"
        );
        let result = if peer.id.kind() == grammers_session::types::PeerKind::Channel {
            self.client
                .invoke(&tl::functions::channels::DeleteMessages {
                    channel: peer.into(),
                    id: ids.to_vec(),
                })
                .await?
        } else {
            self.client
                .invoke(&tl::functions::messages::DeleteMessages {
                    revoke: true,
                    id: ids.to_vec(),
                })
                .await?
        };
        let tl::enums::messages::AffectedMessages::Messages(result) = result;
        anyhow::ensure!(result.pts_count > 0, "message to delete not found");
        Ok(())
    }
    pub async fn send_photo(
        &self,
        peer: PeerRef,
        bytes: Vec<u8>,
        caption: &str,
    ) -> Result<Message> {
        let media = crate::media::Media {
            kind: crate::file_id::FileKind::Photo,
            source: crate::media::Source::Bytes {
                name: "photo.jpg".into(),
                data: bytes,
            },
            ..Default::default()
        };
        self.send_media(peer, &media, InputMessage::new().text(caption))
            .await
    }
    pub async fn send_photo_url(&self, peer: PeerRef, url: &str, caption: &str) -> Result<Message> {
        let parsed = reqwest::Url::parse(url)?;
        anyhow::ensure!(parsed.scheme() == "https", "photo URL must use HTTPS");
        Ok(self
            .client
            .send_message(peer, InputMessage::new().text(caption).photo_url(url))
            .await?)
    }
    pub async fn download(&self, message: &Message, limit: usize) -> Result<Vec<u8>> {
        let media = message
            .media()
            .ok_or_else(|| anyhow::anyhow!("message contains no media"))?;
        let mut download = self.client.iter_download(&media);
        let mut bytes = Vec::new();
        while let Some(chunk) =
            tokio::time::timeout(std::time::Duration::from_secs(30), download.next()).await??
        {
            anyhow::ensure!(
                bytes.len() + chunk.len() <= limit,
                "download limit exceeded"
            );
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    }
    pub async fn send_message(&self, peer: PeerRef, message: InputMessage) -> Result<Message> {
        Ok(self.client.send_message(peer, message).await?)
    }
    pub async fn send_document(
        &self,
        peer: PeerRef,
        name: &str,
        bytes: Vec<u8>,
        caption: &str,
    ) -> Result<Message> {
        let media = crate::media::Media {
            kind: crate::file_id::FileKind::Document,
            source: crate::media::Source::Bytes {
                name: name.into(),
                data: bytes,
            },
            ..Default::default()
        };
        self.send_media(peer, &media, InputMessage::new().text(caption))
            .await
    }
    pub async fn edit_callback(&self, query: &CallbackQuery, message: InputMessage) -> Result<()> {
        self.edit_callback_rich(query, message, None).await
    }
    pub async fn edit_callback_media(
        &self,
        query: &CallbackQuery,
        message: InputMessage,
        media: &crate::media::Media,
    ) -> Result<()> {
        if let tl::enums::Update::InlineBotCallbackQuery(q) = &query.raw {
            self.edit_inline_message(
                &crate::file_id::encode_inline_id(&q.msg_id),
                message,
                Some(media),
                None,
            )
            .await?;
            query.answer().send().await?;
        } else {
            let prepared = media.prepare(&self.client).await?;
            query.answer().edit(message.media(prepared)).await?;
        }
        Ok(())
    }
    pub async fn edit_callback_rich(
        &self,
        query: &CallbackQuery,
        message: InputMessage,
        rich: Option<String>,
    ) -> Result<()> {
        if let tl::enums::Update::InlineBotCallbackQuery(q) = &query.raw {
            let rich = rich.map(|source| crate::bot_edits::RichText::Html { source });
            let request = crate::bot_edits::inline(q.msg_id.clone(), message, rich.as_ref(), None)?;
            let dc = q.msg_id.dc_id();
            self.invoke_inline_dc(dc, &request).await?;
            query.answer().send().await?;
        } else if let (Some(source), tl::enums::Update::BotCallbackQuery(q)) = (rich, &query.raw) {
            let article = grammers_client::update::Article::new("edit", message);
            let tl::enums::InputBotInlineMessage::Text(content) = article.raw.send_message else {
                anyhow::bail!("expected callback content");
            };
            let peer = query
                .peer_ref()
                .await
                .map_err(anyhow::Error::from_boxed)?
                .context("callback peer missing")?;
            self.client
                .invoke(&crate::bot_edits::rich(
                    peer,
                    q.msg_id,
                    &crate::bot_edits::RichText::Html { source },
                    content.reply_markup,
                )?)
                .await?;
            query.answer().send().await?;
        } else {
            query.answer().edit(message).await?;
        }
        Ok(())
    }
}
