//! Bot-facing request options and pure MTProto builders, validated before any upload/RPC.
use crate::{bot_edits::RichText, botapi::MessageEntity, markup::Keyboard};
use anyhow::{Context, Result, ensure};
use grammers_client::{message::InputMessage, tl};
use grammers_session::types::PeerRef;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParseMode {
    #[default]
    Plain,
    Html,
    Markdown,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatAction {
    Typing,
    UploadPhoto,
    RecordVideo,
    UploadVideo,
    RecordVoice,
    UploadVoice,
    UploadDocument,
    ChooseSticker,
    FindLocation,
    RecordVideoNote,
    UploadVideoNote,
    Cancel,
}
impl ChatAction {
    pub fn to_tl(self) -> tl::enums::SendMessageAction {
        match self {
            Self::Typing => tl::enums::SendMessageAction::SendMessageTypingAction,
            Self::RecordVideo => tl::enums::SendMessageAction::SendMessageRecordVideoAction,
            Self::RecordVoice => tl::enums::SendMessageAction::SendMessageRecordAudioAction,
            Self::ChooseSticker => tl::enums::SendMessageAction::SendMessageChooseStickerAction,
            Self::FindLocation => tl::enums::SendMessageAction::SendMessageGeoLocationAction,
            Self::RecordVideoNote => tl::enums::SendMessageAction::SendMessageRecordRoundAction,
            Self::Cancel => tl::enums::SendMessageAction::SendMessageCancelAction,
            Self::UploadPhoto => tl::types::SendMessageUploadPhotoAction { progress: 0 }.into(),
            Self::UploadVideo => tl::types::SendMessageUploadVideoAction { progress: 0 }.into(),
            Self::UploadVoice => tl::types::SendMessageUploadAudioAction { progress: 0 }.into(),
            Self::UploadDocument => {
                tl::types::SendMessageUploadDocumentAction { progress: 0 }.into()
            }
            Self::UploadVideoNote => tl::types::SendMessageUploadRoundAction { progress: 0 }.into(),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum CommandScope {
    Default,
    AllPrivateChats,
    AllGroupChats,
    AllChatAdministrators,
    Chat { chat_id: String },
    ChatAdministrators { chat_id: String },
    ChatMember { chat_id: String, user_id: String },
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Text {
    pub text: String,
    pub parse_mode: ParseMode,
    pub entities: Option<Vec<MessageEntity>>,
}
impl Text {
    pub fn parse(
        &self,
        limit: usize,
        allow_empty: bool,
    ) -> Result<(String, Option<Vec<tl::enums::MessageEntity>>)> {
        ensure!(self.text.len() <= 128 * 1024, "text source exceeds 128 KiB");
        let (text, entities) = if let Some(entities) = &self.entities {
            ensure!(entities.len() <= 1000, "too many text entities");
            let values = entities
                .iter()
                .map(|e| e.to_tl(&self.text))
                .collect::<Result<Vec<_>>>()?;
            (self.text.clone(), values)
        } else {
            let input = match self.parse_mode {
                ParseMode::Plain => InputMessage::new().text(&self.text),
                ParseMode::Html => InputMessage::new().html(&self.text),
                ParseMode::Markdown => InputMessage::new().markdown(&self.text),
            };
            let article = grammers_client::update::Article::new("", input);
            let tl::enums::InputBotInlineMessage::Text(t) = article.raw.send_message else {
                unreachable!();
            };
            (t.message, t.entities.unwrap_or_default())
        };
        ensure!(
            (allow_empty || !text.is_empty()) && text.encode_utf16().count() <= limit,
            "text is empty or exceeds Telegram length limit"
        );
        Ok((
            text,
            if entities.is_empty() {
                None
            } else {
                Some(entities)
            },
        ))
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Preview {
    pub is_disabled: bool,
    pub url: Option<String>,
    pub prefer_large_media: bool,
    pub prefer_small_media: bool,
    pub show_above_text: bool,
}
impl Preview {
    pub fn media(&self) -> Result<Option<tl::enums::InputMedia>> {
        ensure!(
            !(self.prefer_large_media && self.prefer_small_media),
            "conflicting preview size preferences"
        );
        if let Some(url) = &self.url {
            let u = reqwest::Url::parse(url)?;
            ensure!(
                url.len() <= 8192
                    && matches!(u.scheme(), "http" | "https")
                    && u.host_str().is_some(),
                "invalid preview URL"
            );
        }
        Ok(self.url.as_ref().filter(|_| !self.is_disabled).map(|url| {
            tl::types::InputMediaWebPage {
                force_large_media: self.prefer_large_media,
                force_small_media: self.prefer_small_media,
                optional: true,
                url: url.clone(),
            }
            .into()
        }))
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SendOptions {
    pub disable_notification: bool,
    pub protect_content: bool,
    pub reply_to_message_id: Option<i32>,
    pub message_thread_id: Option<i32>,
    pub reply_markup: Option<Keyboard>,
    pub message_effect_id: Option<i64>,
    pub business_connection_id: Option<String>,
}
impl SendOptions {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.reply_to_message_id.is_none_or(|n| n > 0)
                && self.message_thread_id.is_none_or(|n| n > 0),
            "reply/thread IDs must be positive"
        );
        ensure!(
            self.message_effect_id.is_none_or(|n| n > 0),
            "effect ID must be positive"
        );
        business(self.business_connection_id.as_deref())?;
        if let Some(keyboard) = &self.reply_markup {
            keyboard.to_tl()?;
        }
        Ok(())
    }
    pub fn reply(&self) -> Option<tl::enums::InputReplyTo> {
        self.reply_to_message_id
            .or(self.message_thread_id)
            .map(|id| {
                tl::types::InputReplyToMessage {
                    reply_to_msg_id: id,
                    top_msg_id: if self.reply_to_message_id.is_some() {
                        self.message_thread_id
                    } else {
                        None
                    },
                    reply_to_peer_id: None,
                    quote_text: None,
                    quote_entities: None,
                    quote_offset: None,
                    monoforum_peer_id: None,
                    todo_item_id: None,
                    poll_option: None,
                }
                .into()
            })
    }
    pub fn markup(&self) -> Result<Option<tl::enums::ReplyMarkup>> {
        self.reply_markup.as_ref().map(Keyboard::to_tl).transpose()
    }
}
pub fn business(id: Option<&str>) -> Result<()> {
    ensure!(
        id.is_none_or(|s| !s.is_empty() && s.len() <= 4096 && !s.chars().any(char::is_control)),
        "invalid business connection ID"
    );
    Ok(())
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AlbumItem {
    pub media: crate::media::Media,
    #[serde(default)]
    pub caption: Text,
}
pub fn validate_album(items: &[AlbumItem], options: &SendOptions) -> Result<()> {
    options.validate()?;
    ensure!(
        options.reply_markup.is_none(),
        "albums do not support reply markup"
    );
    crate::media::Media::validate_album(
        &items.iter().map(|i| i.media.clone()).collect::<Vec<_>>(),
    )?;
    for item in items {
        item.caption.parse(1024, true)?;
    }
    Ok(())
}
pub fn album(
    peer: PeerRef,
    items: Vec<tl::enums::InputSingleMedia>,
    options: &SendOptions,
) -> Result<tl::functions::messages::SendMultiMedia> {
    options.validate()?;
    ensure!(
        (2..=10).contains(&items.len()) && options.reply_markup.is_none(),
        "invalid album count or reply markup"
    );
    Ok(tl::functions::messages::SendMultiMedia {
        silent: options.disable_notification,
        background: false,
        clear_draft: false,
        noforwards: options.protect_content,
        update_stickersets_order: false,
        invert_media: false,
        allow_paid_floodskip: false,
        peer: peer.into(),
        reply_to: options.reply(),
        multi_media: items,
        schedule_date: None,
        send_as: None,
        quick_reply_shortcut: None,
        effect: options.message_effect_id,
        allow_paid_stars: None,
    })
}
pub fn stored_media(media: tl::enums::MessageMedia) -> Result<tl::enums::InputMedia> {
    let native = grammers_client::media::Media::from_raw(media)
        .context("uploaded media response is empty/unsupported")?;
    crate::file_id::FileRef::from_media(&native)?
        .0
        .input_media()
}
pub enum SendRequest {
    Text(tl::functions::messages::SendMessage),
    Media(tl::functions::messages::SendMedia),
}
pub fn send(
    peer: PeerRef,
    text: &Text,
    rich: Option<&RichText>,
    preview: &Preview,
    options: &SendOptions,
    random_id: i64,
) -> Result<SendRequest> {
    options.validate()?;
    let preview_media = preview.media()?;
    let (message, entities) = if rich.is_some() {
        (String::new(), None)
    } else {
        text.parse(4096, false)?
    };
    if let Some(rich) = rich {
        return Ok(SendRequest::Text(tl::functions::messages::SendMessage {
            no_webpage: false,
            silent: options.disable_notification,
            background: false,
            clear_draft: false,
            noforwards: options.protect_content,
            update_stickersets_order: false,
            invert_media: false,
            allow_paid_floodskip: false,
            peer: peer.into(),
            reply_to: options.reply(),
            message,
            random_id,
            reply_markup: options.markup()?,
            entities: None,
            schedule_date: None,
            schedule_repeat_period: None,
            send_as: None,
            quick_reply_shortcut: None,
            effect: options.message_effect_id,
            allow_paid_stars: None,
            suggested_post: None,
            rich_message: Some(rich.to_tl()?),
        }));
    }
    if let Some(media) = preview_media {
        return Ok(SendRequest::Media(send_media(
            peer,
            media,
            message,
            entities,
            preview.show_above_text,
            options,
            random_id,
        )?));
    }
    Ok(SendRequest::Text(tl::functions::messages::SendMessage {
        no_webpage: preview.is_disabled,
        silent: options.disable_notification,
        background: false,
        clear_draft: false,
        noforwards: options.protect_content,
        update_stickersets_order: false,
        invert_media: preview.show_above_text,
        allow_paid_floodskip: false,
        peer: peer.into(),
        reply_to: options.reply(),
        message,
        random_id,
        reply_markup: options.markup()?,
        entities,
        schedule_date: None,
        schedule_repeat_period: None,
        send_as: None,
        quick_reply_shortcut: None,
        effect: options.message_effect_id,
        allow_paid_stars: None,
        suggested_post: None,
        rich_message: None,
    }))
}
pub fn send_media(
    peer: PeerRef,
    media: tl::enums::InputMedia,
    message: String,
    entities: Option<Vec<tl::enums::MessageEntity>>,
    above: bool,
    options: &SendOptions,
    random_id: i64,
) -> Result<tl::functions::messages::SendMedia> {
    options.validate()?;
    Ok(tl::functions::messages::SendMedia {
        silent: options.disable_notification,
        background: false,
        clear_draft: false,
        noforwards: options.protect_content,
        update_stickersets_order: false,
        invert_media: above,
        allow_paid_floodskip: false,
        peer: peer.into(),
        reply_to: options.reply(),
        media,
        message,
        random_id,
        reply_markup: options.markup()?,
        entities,
        schedule_date: None,
        schedule_repeat_period: None,
        send_as: None,
        quick_reply_shortcut: None,
        effect: options.message_effect_id,
        allow_paid_stars: None,
        suggested_post: None,
    })
}
pub fn forward(
    from: PeerRef,
    to: PeerRef,
    id: i32,
    copy: bool,
    options: &SendOptions,
    random_id: i64,
) -> Result<tl::functions::messages::ForwardMessages> {
    options.validate()?;
    ensure!(id > 0, "message ID must be positive");
    ensure!(
        options.reply_markup.is_none(),
        "forward does not accept reply markup"
    );
    Ok(tl::functions::messages::ForwardMessages {
        silent: options.disable_notification,
        background: false,
        with_my_score: false,
        drop_author: copy,
        drop_media_captions: false,
        noforwards: options.protect_content,
        allow_paid_floodskip: false,
        from_ephemeral: false,
        from_peer: from.into(),
        id: vec![id],
        random_id: vec![random_id],
        to_peer: to.into(),
        top_msg_id: options.message_thread_id,
        reply_to: options.reply(),
        schedule_date: None,
        schedule_repeat_period: None,
        send_as: None,
        quick_reply_shortcut: None,
        effect: options.message_effect_id,
        video_timestamp: None,
        allow_paid_stars: None,
        suggested_post: None,
    })
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Edit {
    Text {
        text: Text,
        #[serde(default)]
        preview: Preview,
    },
    Rich {
        rich: RichText,
    },
    Caption {
        caption: Text,
        #[serde(default)]
        show_above_media: bool,
    },
    Media {
        media: Box<crate::media::Media>,
        #[serde(default)]
        caption: Text,
        #[serde(default)]
        show_above_media: bool,
    },
    Keyboard,
}
pub struct EditFields {
    pub text: Option<String>,
    pub entities: Option<Vec<tl::enums::MessageEntity>>,
    pub media: Option<tl::enums::InputMedia>,
    pub rich: Option<tl::enums::InputRichMessage>,
    pub no_webpage: bool,
    pub invert: bool,
    pub markup: Option<tl::enums::ReplyMarkup>,
}
impl Edit {
    pub fn fields(&self, keyboard: Option<&Keyboard>) -> Result<EditFields> {
        let mut fields = EditFields {
            text: None,
            entities: None,
            media: None,
            rich: None,
            no_webpage: false,
            invert: false,
            markup: keyboard.map(Keyboard::to_tl).transpose()?,
        };
        match self {
            Self::Text { text, preview } => {
                let (text, entities) = text.parse(4096, false)?;
                fields.text = Some(text);
                fields.entities = Some(entities.unwrap_or_default());
                fields.media = preview.media()?;
                fields.no_webpage = preview.is_disabled;
                fields.invert = preview.show_above_text;
            }
            Self::Rich { rich } => fields.rich = Some(rich.to_tl()?),
            Self::Caption {
                caption,
                show_above_media,
            }
            | Self::Media {
                caption,
                show_above_media,
                ..
            } => {
                let (text, entities) = caption.parse(1024, true)?;
                fields.text = Some(text);
                fields.entities = Some(entities.unwrap_or_default());
                fields.invert = *show_above_media;
            }
            Self::Keyboard => {
                if fields.markup.is_none() {
                    fields.markup = Some(crate::markup::keyboard(&[])?.raw);
                }
            }
        }
        if let Self::Media { media, .. } = self {
            media.validate()?;
        }
        Ok(fields)
    }
}
impl EditFields {
    pub fn chat(self, peer: PeerRef, id: i32) -> Result<tl::functions::messages::EditMessage> {
        ensure!(id > 0, "message ID must be positive");
        Ok(tl::functions::messages::EditMessage {
            peer: peer.into(),
            id,
            no_webpage: self.no_webpage,
            invert_media: self.invert,
            message: self.text,
            entities: self.entities,
            media: self.media,
            reply_markup: self.markup,
            rich_message: self.rich,
            schedule_date: None,
            schedule_repeat_period: None,
            quick_reply_shortcut_id: None,
        })
    }
    pub fn inline(
        self,
        id: tl::enums::InputBotInlineMessageId,
    ) -> tl::functions::messages::EditInlineBotMessage {
        tl::functions::messages::EditInlineBotMessage {
            id,
            no_webpage: self.no_webpage,
            invert_media: self.invert,
            message: self.text,
            entities: self.entities,
            media: self.media,
            reply_markup: self.markup,
            rich_message: self.rich,
        }
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CallbackAnswer {
    pub text: Option<String>,
    pub show_alert: bool,
    pub url: Option<String>,
    pub cache_time: i32,
}
impl CallbackAnswer {
    pub fn request(&self, id: i64) -> Result<tl::functions::messages::SetBotCallbackAnswer> {
        ensure!(
            self.text
                .as_ref()
                .is_none_or(|s| s.encode_utf16().count() <= 200),
            "callback answer exceeds 200 UTF-16 units"
        );
        ensure!(
            (0..=86400).contains(&self.cache_time),
            "invalid callback cache time"
        );
        if let Some(url) = &self.url {
            let u = reqwest::Url::parse(url)?;
            ensure!(
                url.len() <= 8192 && matches!(u.scheme(), "https" | "tg"),
                "invalid callback URL"
            );
        }
        Ok(tl::functions::messages::SetBotCallbackAnswer {
            alert: self.show_alert,
            query_id: id,
            message: self.text.clone().filter(|s| !s.is_empty()),
            url: self.url.clone(),
            cache_time: self.cache_time,
        })
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum InlineButton {
    Start { text: String, parameter: String },
    WebApp { text: String, url: String },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct InlineAnswer {
    pub cache_time: i32,
    pub is_personal: bool,
    pub next_offset: Option<String>,
    pub button: Option<InlineButton>,
    pub gallery: Option<bool>,
}
impl Default for InlineAnswer {
    fn default() -> Self {
        Self {
            cache_time: 300,
            is_personal: false,
            next_offset: None,
            button: None,
            gallery: None,
        }
    }
}
impl InlineAnswer {
    pub fn request(
        &self,
        id: i64,
        results: &[crate::botapi::InlineResult],
    ) -> Result<tl::functions::messages::SetInlineBotResults> {
        ensure!(
            (0..=86400).contains(&self.cache_time),
            "invalid inline cache time"
        );
        ensure!(
            self.next_offset.as_ref().is_none_or(|s| s.len() <= 64),
            "inline next_offset exceeds 64 bytes"
        );
        let mut switch_pm = None;
        let mut switch_webview = None;
        if let Some(button) = &self.button {
            let text = match button {
                InlineButton::Start { text, .. } | InlineButton::WebApp { text, .. } => text,
            };
            ensure!(
                !text.is_empty() && text.encode_utf16().count() <= 64,
                "invalid inline switch button text"
            );
            match button {
                InlineButton::Start { text, parameter } => {
                    ensure!(
                        !parameter.is_empty()
                            && parameter.len() <= 64
                            && parameter
                                .bytes()
                                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-')),
                        "invalid start parameter"
                    );
                    switch_pm = Some(
                        tl::types::InlineBotSwitchPm {
                            text: text.clone(),
                            start_param: parameter.clone(),
                        }
                        .into(),
                    );
                }
                InlineButton::WebApp { text, url } => {
                    let u = reqwest::Url::parse(url)?;
                    ensure!(
                        u.scheme() == "https" && u.host_str().is_some() && url.len() <= 8192,
                        "inline web app requires HTTPS"
                    );
                    switch_webview = Some(
                        tl::types::InlineBotWebView {
                            text: text.clone(),
                            url: url.clone(),
                        }
                        .into(),
                    );
                }
            }
        }
        let results = crate::botapi::inline_results(results)?;
        let gallery = self.gallery.unwrap_or(
            !results.is_empty()
                && results.iter().all(|r| match r {
                    tl::enums::InputBotInlineResult::Result(r) => {
                        matches!(r.r#type.as_str(), "photo" | "gif" | "sticker")
                    }
                    tl::enums::InputBotInlineResult::Photo(_) => true,
                    tl::enums::InputBotInlineResult::Document(r) => {
                        matches!(r.r#type.as_str(), "gif" | "sticker")
                    }
                    _ => false,
                }),
        );
        Ok(tl::functions::messages::SetInlineBotResults {
            gallery,
            private: self.is_personal,
            query_id: id,
            results,
            cache_time: self.cache_time,
            next_offset: self.next_offset.clone().filter(|s| !s.is_empty()),
            switch_pm,
            switch_webview,
        })
    }
}
