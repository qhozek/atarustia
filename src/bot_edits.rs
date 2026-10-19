//! Pure Bot API edit builders: omitted fields stay omitted (keyboard-only edits never erase text).
use anyhow::{Result, ensure};
use grammers_client::{
    message::{InputMessage, InputRichMessage},
    tl,
};
use grammers_session::types::PeerRef;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum RichText {
    Html {
        source: String,
    },
    Markdown {
        source: String,
    },
    HtmlOptions {
        source: String,
        #[serde(default)]
        is_rtl: bool,
        #[serde(default)]
        skip_entity_detection: bool,
    },
    MarkdownOptions {
        source: String,
        #[serde(default)]
        is_rtl: bool,
        #[serde(default)]
        skip_entity_detection: bool,
    },
}
impl RichText {
    pub fn to_tl(&self) -> Result<tl::enums::InputRichMessage> {
        let source = match self {
            Self::Html { source }
            | Self::Markdown { source }
            | Self::HtmlOptions { source, .. }
            | Self::MarkdownOptions { source, .. } => source,
        };
        ensure!(
            !source.is_empty() && source.len() <= 128 * 1024,
            "rich source must contain 1 byte–128 KiB"
        );
        Ok(match self {
            Self::Html { .. } => InputRichMessage::html(source).raw,
            Self::Markdown { .. } => InputRichMessage::markdown(source).raw,
            Self::HtmlOptions {
                is_rtl,
                skip_entity_detection,
                ..
            } => tl::types::InputRichMessageHtml {
                rtl: *is_rtl,
                noautolink: *skip_entity_detection,
                html: source.clone(),
                files: None,
            }
            .into(),
            Self::MarkdownOptions {
                is_rtl,
                skip_entity_detection,
                ..
            } => tl::types::InputRichMessageMarkdown {
                rtl: *is_rtl,
                noautolink: *skip_entity_detection,
                markdown: source.clone(),
                files: None,
            }
            .into(),
        })
    }
}
pub fn inline(
    id: tl::enums::InputBotInlineMessageId,
    message: InputMessage,
    rich: Option<&RichText>,
    media: Option<tl::enums::InputMedia>,
) -> Result<tl::functions::messages::EditInlineBotMessage> {
    ensure!(
        rich.is_none() || media.is_none(),
        "rich content cannot be combined with media"
    );
    let article = grammers_client::update::Article::new("edit", message);
    let tl::enums::InputBotInlineMessage::Text(content) = article.raw.send_message else {
        anyhow::bail!("expected text edit content");
    };
    Ok(tl::functions::messages::EditInlineBotMessage {
        id,
        no_webpage: content.no_webpage,
        invert_media: content.invert_media,
        message: if rich.is_some() {
            None
        } else {
            Some(content.message)
        },
        media,
        entities: if rich.is_some() {
            None
        } else {
            content.entities
        },
        reply_markup: content.reply_markup,
        rich_message: rich.map(RichText::to_tl).transpose()?,
    })
}
pub fn rich(
    chat: PeerRef,
    id: i32,
    value: &RichText,
    markup: Option<tl::enums::ReplyMarkup>,
) -> Result<tl::functions::messages::EditMessage> {
    ensure!(id > 0, "message ID must be positive");
    Ok(tl::functions::messages::EditMessage {
        peer: chat.into(),
        id,
        no_webpage: false,
        invert_media: false,
        message: None,
        entities: None,
        media: None,
        reply_markup: markup,
        rich_message: Some(value.to_tl()?),
        schedule_date: None,
        schedule_repeat_period: None,
        quick_reply_shortcut_id: None,
    })
}
pub fn caption(
    chat: PeerRef,
    id: i32,
    text: &str,
    html: bool,
    keyboard: Option<&[Vec<crate::markup::Key>]>,
) -> Result<tl::functions::messages::EditMessage> {
    ensure!(
        id > 0 && text.len() <= 64 * 1024,
        "invalid message ID or caption source"
    );
    let input = if html {
        InputMessage::new().html(text)
    } else {
        InputMessage::new().text(text)
    };
    let article = grammers_client::update::Article::new("caption", input);
    let tl::enums::InputBotInlineMessage::Text(content) = article.raw.send_message else {
        unreachable!();
    };
    ensure!(
        content.message.encode_utf16().count() <= 1024,
        "caption exceeds 1024 UTF-16 units"
    );
    Ok(tl::functions::messages::EditMessage {
        peer: chat.into(),
        id,
        no_webpage: false,
        invert_media: false,
        message: Some(content.message),
        entities: Some(content.entities.unwrap_or_default()),
        media: None,
        reply_markup: keyboard
            .map(crate::markup::keyboard)
            .transpose()?
            .map(|k| k.raw),
        rich_message: None,
        schedule_date: None,
        schedule_repeat_period: None,
        quick_reply_shortcut_id: None,
    })
}
pub fn keyboard(
    chat: PeerRef,
    id: i32,
    keys: &[Vec<crate::markup::Key>],
) -> Result<tl::functions::messages::EditMessage> {
    let mut request = caption(chat, id, "", false, Some(keys))?;
    request.message = None;
    request.entities = None;
    Ok(request)
}
pub fn inline_caption(
    id: tl::enums::InputBotInlineMessageId,
    text: &str,
    html: bool,
    keys: Option<&[Vec<crate::markup::Key>]>,
) -> Result<tl::functions::messages::EditInlineBotMessage> {
    let peer = grammers_session::types::PeerId::self_user().to_ambient_ref();
    let base = caption(peer, 1, text, html, keys)?;
    Ok(tl::functions::messages::EditInlineBotMessage {
        id,
        no_webpage: false,
        invert_media: false,
        message: base.message,
        entities: base.entities,
        media: None,
        reply_markup: base.reply_markup,
        rich_message: None,
    })
}
pub fn inline_keyboard(
    id: tl::enums::InputBotInlineMessageId,
    keys: &[Vec<crate::markup::Key>],
) -> Result<tl::functions::messages::EditInlineBotMessage> {
    let mut request = inline_caption(id, "", false, Some(keys))?;
    request.message = None;
    request.entities = None;
    Ok(request)
}
