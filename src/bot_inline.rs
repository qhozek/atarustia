//! Extra inline result kinds and explicit outer metadata, independent of sent content.
use anyhow::{Result, ensure};
use grammers_client::tl;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Article,
    Photo,
    Gif,
    Mpeg4Gif,
    Video,
    Audio,
    Voice,
    Document,
    Sticker,
    Contact,
    Location,
    Venue,
}
impl Kind {
    pub fn tl_name(self) -> &'static str {
        match self {
            Self::Article => "article",
            Self::Photo => "photo",
            Self::Gif | Self::Mpeg4Gif => "gif",
            Self::Video => "video",
            Self::Audio => "audio",
            Self::Voice => "voice",
            Self::Document => "file",
            Self::Sticker => "sticker",
            Self::Contact => "contact",
            Self::Location => "geo",
            Self::Venue => "venue",
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResultSpec {
    pub id: String,
    pub kind: Kind,
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub thumbnail_url: Option<String>,
    #[serde(default)]
    pub media: Option<Box<crate::media::Media>>,
    pub content: crate::inline_content::InputContent,
    #[serde(default)]
    pub keyboard: Vec<Vec<crate::markup::Key>>,
}
impl ResultSpec {
    pub fn to_tl(&self) -> Result<tl::enums::InputBotInlineResult> {
        ensure!(
            !matches!(
                self.content,
                crate::inline_content::InputContent::Caption { .. }
            ) || self.media.is_some(),
            "inline caption content requires media"
        );
        if let Some(media) = &self.media {
            let expected = match self.kind {
                Kind::Photo => crate::file_id::FileKind::Photo,
                Kind::Gif | Kind::Mpeg4Gif => crate::file_id::FileKind::Animation,
                Kind::Video => crate::file_id::FileKind::Video,
                Kind::Audio => crate::file_id::FileKind::Audio,
                Kind::Voice => crate::file_id::FileKind::Voice,
                Kind::Document => crate::file_id::FileKind::Document,
                Kind::Sticker => crate::file_id::FileKind::Sticker,
                _ => anyhow::bail!("non-media inline kind cannot have media source"),
            };
            ensure!(
                media.kind == expected,
                "inline kind and media kind do not match"
            );
        } else {
            ensure!(
                matches!(
                    self.kind,
                    Kind::Article | Kind::Contact | Kind::Location | Kind::Venue
                ),
                "media inline result needs a source"
            );
        }
        let mut result = crate::inline_content::result(
            &self.id,
            &self.title,
            self.description.as_deref(),
            self.url.as_deref(),
            self.thumbnail_url.as_deref(),
            self.media.as_deref(),
            &self.content,
            &self.keyboard,
        )?;
        match &mut result {
            tl::enums::InputBotInlineResult::Result(r) => r.r#type = self.kind.tl_name().into(),
            tl::enums::InputBotInlineResult::Photo(r) => r.r#type = self.kind.tl_name().into(),
            tl::enums::InputBotInlineResult::Document(r) => r.r#type = self.kind.tl_name().into(),
            _ => anyhow::bail!("unexpected inline constructor"),
        };
        Ok(result)
    }
}
pub fn game(
    id: &str,
    short_name: &str,
    keyboard: &[Vec<crate::markup::Key>],
) -> Result<tl::enums::InputBotInlineResult> {
    ensure!(
        !id.is_empty() && id.len() <= 64,
        "inline game ID must be 1–64 bytes"
    );
    ensure!(
        !short_name.is_empty()
            && short_name.len() <= 64
            && short_name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_'),
        "invalid game short name"
    );
    Ok(tl::types::InputBotInlineResultGame {
        id: id.into(),
        short_name: short_name.into(),
        send_message: tl::types::InputBotInlineMessageGame {
            reply_markup: if keyboard.is_empty() {
                None
            } else {
                Some(crate::markup::keyboard(keyboard)?.raw)
            },
        }
        .into(),
    }
    .into())
}
