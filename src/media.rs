//! Shared, validated media pipeline for ordinary bot messages, edits, albums and inline units.
use crate::file_id::{FileKind, FileRef};
use anyhow::{Context, Result, ensure};
use grammers_client::{Client, media::Uploaded, tl};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Source {
    Url { url: String },
    FileId { file_id: String },
    Bytes { name: String, data: Vec<u8> },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Media {
    pub kind: FileKind,
    pub source: Source,
    pub mime_type: Option<String>,
    pub duration: f64,
    pub width: i32,
    pub height: i32,
    pub title: Option<String>,
    pub performer: Option<String>,
    pub spoiler: bool,
    pub supports_streaming: bool,
    pub thumbnail_url: Option<String>,
    /// Explicit in-memory JPEG thumbnail for uploaded documents/video/audio.
    pub upload_thumbnail: Option<Thumbnail>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Thumbnail {
    pub name: String,
    pub data: Vec<u8>,
}
impl Default for Media {
    fn default() -> Self {
        Self {
            kind: FileKind::Document,
            source: Source::Url { url: String::new() },
            mime_type: None,
            duration: 0.0,
            width: 0,
            height: 0,
            title: None,
            performer: None,
            spoiler: false,
            supports_streaming: true,
            thumbnail_url: None,
            upload_thumbnail: None,
        }
    }
}
impl Media {
    pub fn validate(&self) -> Result<()> {
        if let Some(thumb) = &self.upload_thumbnail {
            ensure!(
                matches!(self.source, Source::Bytes { .. }) && self.kind != FileKind::Photo,
                "upload thumbnail requires uploaded document media"
            );
            ensure!(
                !thumb.name.is_empty()
                    && thumb.name.len() <= 255
                    && !thumb.name.contains(['/', '\\'])
                    && !thumb.name.chars().any(char::is_control),
                "invalid thumbnail filename"
            );
            ensure!(
                !thumb.data.is_empty() && thumb.data.len() <= 200 * 1024,
                "thumbnail must contain 1 byte–200 KiB"
            );
        }
        if let Some(url) = &self.thumbnail_url {
            let url = reqwest::Url::parse(url)?;
            ensure!(
                url.scheme() == "https" && url.host_str().is_some(),
                "thumbnail requires HTTPS"
            );
        }
        ensure!(
            self.duration.is_finite() && (0.0..=i32::MAX as f64).contains(&self.duration),
            "invalid media duration"
        );
        ensure!(
            (0..=32768).contains(&self.width) && (0..=32768).contains(&self.height),
            "invalid media dimensions"
        );
        if let Some(mime) = &self.mime_type {
            ensure!(
                mime.len() <= 128 && mime.contains('/') && !mime.chars().any(char::is_control),
                "invalid MIME type"
            );
        }
        ensure!(
            self.title.as_ref().is_none_or(|s| s.len() <= 1024)
                && self.performer.as_ref().is_none_or(|s| s.len() <= 1024),
            "audio metadata too large"
        );
        match &self.source {
            Source::Url { url } => {
                let parsed = reqwest::Url::parse(url)?;
                ensure!(
                    parsed.scheme() == "https"
                        && parsed.host_str().is_some()
                        && parsed.username().is_empty()
                        && parsed.password().is_none(),
                    "media URL must be HTTPS with no credentials"
                );
                ensure!(url.len() <= 8192, "media URL too long");
            }
            Source::FileId { file_id } => {
                let file = FileRef::decode(file_id)?;
                ensure!(
                    (self.kind == FileKind::Photo) == (file.kind == FileKind::Photo),
                    "photo/document file ID mismatch"
                );
            }
            Source::Bytes { name, data } => {
                ensure!(
                    !name.is_empty()
                        && name.len() <= 255
                        && !name.chars().any(char::is_control)
                        && !name.contains(['/', '\\']),
                    "upload name must be a filename"
                );
                ensure!(
                    !data.is_empty() && data.len() <= 64 * 1024 * 1024,
                    "in-memory upload must be 1 byte–64 MiB"
                );
            }
        }
        Ok(())
    }
    pub fn mime(&self) -> &str {
        if self.mime_type.is_none()
            && self.kind == FileKind::Animation
            && match &self.source {
                Source::Url { url } => reqwest::Url::parse(url)
                    .ok()
                    .is_some_and(|u| u.path().to_ascii_lowercase().ends_with(".gif")),
                Source::Bytes { name, .. } => name.to_ascii_lowercase().ends_with(".gif"),
                _ => false,
            }
        {
            return "image/gif";
        }
        self.mime_type.as_deref().unwrap_or(match self.kind {
            FileKind::Photo => "image/jpeg",
            FileKind::Video | FileKind::Animation | FileKind::VideoNote => "video/mp4",
            FileKind::Voice => "audio/ogg",
            FileKind::Audio => "audio/mpeg",
            FileKind::Sticker => "image/webp",
            FileKind::Document => "application/octet-stream",
        })
    }
    pub fn from_url(kind: FileKind, url: impl Into<String>) -> Self {
        Self {
            kind,
            source: Source::Url { url: url.into() },
            ..Default::default()
        }
    }
    pub fn attributes(&self, name: &str) -> Vec<tl::enums::DocumentAttribute> {
        let mut attributes = vec![
            tl::types::DocumentAttributeFilename {
                file_name: name.into(),
            }
            .into(),
        ];
        match self.kind {
            FileKind::Audio | FileKind::Voice => attributes.push(
                tl::types::DocumentAttributeAudio {
                    voice: self.kind == FileKind::Voice,
                    duration: self.duration as i32,
                    title: self.title.clone(),
                    performer: self.performer.clone(),
                    waveform: None,
                }
                .into(),
            ),
            FileKind::Video | FileKind::VideoNote | FileKind::Animation => {
                attributes.push(
                    tl::types::DocumentAttributeVideo {
                        round_message: self.kind == FileKind::VideoNote,
                        supports_streaming: self.supports_streaming,
                        nosound: self.kind == FileKind::Animation,
                        duration: self.duration,
                        w: self.width,
                        h: self.height,
                        preload_prefix_size: None,
                        video_start_ts: None,
                        video_codec: None,
                    }
                    .into(),
                );
                if self.kind == FileKind::Animation {
                    attributes.push(tl::enums::DocumentAttribute::Animated);
                }
            }
            FileKind::Sticker => attributes.push(
                tl::types::DocumentAttributeSticker {
                    mask: false,
                    alt: String::new(),
                    stickerset: tl::enums::InputStickerSet::Empty,
                    mask_coords: None,
                }
                .into(),
            ),
            _ => {}
        }
        attributes
    }
    pub fn input(&self, uploaded: Option<Uploaded>) -> Result<tl::enums::InputMedia> {
        self.validate()?;
        Ok(match &self.source {
            Source::FileId { file_id } => {
                let mut media = FileRef::decode(file_id)?.input_media()?;
                match &mut media {
                    tl::enums::InputMedia::Photo(p) => p.spoiler = self.spoiler,
                    tl::enums::InputMedia::Document(d) => d.spoiler = self.spoiler,
                    _ => {}
                }
                media
            }
            Source::Url { url } => {
                if self.kind == FileKind::Photo {
                    tl::types::InputMediaPhotoExternal {
                        spoiler: self.spoiler,
                        url: url.clone(),
                        ttl_seconds: None,
                    }
                    .into()
                } else {
                    tl::types::InputMediaDocumentExternal {
                        spoiler: self.spoiler,
                        url: url.clone(),
                        ttl_seconds: None,
                        video_cover: None,
                        video_timestamp: None,
                    }
                    .into()
                }
            }
            Source::Bytes { name, .. } => {
                let uploaded = uploaded.context("byte media has not been uploaded")?;
                if self.kind == FileKind::Photo {
                    tl::types::InputMediaUploadedPhoto {
                        spoiler: self.spoiler,
                        file: uploaded.raw,
                        stickers: None,
                        ttl_seconds: None,
                        live_photo: false,
                        video: None,
                    }
                    .into()
                } else {
                    tl::types::InputMediaUploadedDocument {
                        nosound_video: self.kind == FileKind::Animation,
                        force_file: self.kind == FileKind::Document,
                        spoiler: self.spoiler,
                        file: uploaded.raw,
                        thumb: None,
                        mime_type: self.mime().into(),
                        attributes: self.attributes(name),
                        stickers: None,
                        ttl_seconds: None,
                        video_cover: None,
                        video_timestamp: None,
                    }
                    .into()
                }
            }
        })
    }
    pub async fn prepare(&self, client: &Client) -> Result<tl::enums::InputMedia> {
        self.validate()?;
        let uploaded = if let Source::Bytes { name, data } = &self.source {
            Some(
                client
                    .upload_stream(&mut data.as_slice(), data.len(), name.clone())
                    .await?,
            )
        } else {
            None
        };
        let mut media = self.input(uploaded)?;
        if let Some(thumb) = &self.upload_thumbnail {
            let upload = client
                .upload_stream(
                    &mut thumb.data.as_slice(),
                    thumb.data.len(),
                    thumb.name.clone(),
                )
                .await?;
            let tl::enums::InputMedia::UploadedDocument(document) = &mut media else {
                anyhow::bail!("thumbnail requires uploaded document");
            };
            document.thumb = Some(upload.raw);
        }
        Ok(media)
    }
    pub fn validate_album(items: &[Self]) -> Result<()> {
        ensure!(
            (2..=10).contains(&items.len()),
            "media group needs 2–10 items"
        );
        let mut class = None;
        for item in items {
            item.validate()?;
            let current = match item.kind {
                FileKind::Photo | FileKind::Video => 0,
                FileKind::Audio => 1,
                FileKind::Document => 2,
                _ => anyhow::bail!("this media type cannot be part of an album"),
            };
            if let Some(class) = class {
                ensure!(
                    class == current,
                    "album must contain only photos/videos, only audio or only documents"
                );
            }
            class = Some(current);
        }
        Ok(())
    }
}

pub fn inline_result(
    id: &str,
    title: &str,
    text: grammers_client::message::InputMessage,
    media: Option<&Media>,
    rich: Option<&str>,
) -> Result<tl::enums::InputBotInlineResult> {
    ensure!(
        !id.is_empty() && id.len() <= 64,
        "inline result ID must be 1–64 bytes"
    );
    let mut article = grammers_client::update::Article::new(title, text).id(id);
    let tl::enums::InputBotInlineMessage::Text(content) = article.raw.send_message else {
        anyhow::bail!("expected text content");
    };
    let Some(media) = media else {
        article.raw.send_message = if let Some(rich) = rich {
            tl::types::InputBotInlineMessageRichMessage {
                rich_message: grammers_client::message::InputRichMessage::html(rich).raw,
                reply_markup: content.reply_markup,
            }
            .into()
        } else {
            content.into()
        };
        return Ok(article.into());
    };
    media.validate()?;
    ensure!(
        rich.is_none(),
        "rich HTML cannot be combined with inline media"
    );
    let send_message: tl::enums::InputBotInlineMessage =
        tl::types::InputBotInlineMessageMediaAuto {
            invert_media: content.invert_media,
            message: content.message,
            entities: content.entities,
            reply_markup: content.reply_markup,
        }
        .into();
    let kind = match media.kind {
        FileKind::Photo => "photo",
        FileKind::Video => "video",
        FileKind::Audio => "audio",
        FileKind::Voice => "voice",
        FileKind::Animation => "gif",
        FileKind::Document => "file",
        FileKind::Sticker => "sticker",
        FileKind::VideoNote => "video",
    };
    match &media.source {
        Source::Url { url } => {
            let web: tl::enums::InputWebDocument = tl::types::InputWebDocument {
                url: url.clone(),
                size: 0,
                mime_type: media.mime().into(),
                attributes: if media.kind == FileKind::Photo {
                    vec![
                        tl::types::DocumentAttributeImageSize {
                            w: media.width,
                            h: media.height,
                        }
                        .into(),
                    ]
                } else {
                    media.attributes("file")
                },
            }
            .into();
            Ok(tl::types::InputBotInlineResult {
                id: id.into(),
                r#type: kind.into(),
                title: Some(title.into()),
                description: None,
                url: None,
                thumb: if let Some(url) = &media.thumbnail_url {
                    Some(
                        tl::types::InputWebDocument {
                            url: url.clone(),
                            size: 0,
                            mime_type: "image/jpeg".into(),
                            attributes: vec![],
                        }
                        .into(),
                    )
                } else if media.kind == FileKind::Photo {
                    Some(web.clone())
                } else {
                    None
                },
                content: Some(web),
                send_message,
            }
            .into())
        }
        Source::FileId { file_id } => {
            let file = FileRef::decode(file_id)?;
            Ok(if file.kind == FileKind::Photo {
                tl::types::InputBotInlineResultPhoto {
                    id: id.into(),
                    r#type: kind.into(),
                    photo: file.input_photo()?,
                    send_message,
                }
                .into()
            } else {
                tl::types::InputBotInlineResultDocument {
                    id: id.into(),
                    r#type: kind.into(),
                    title: Some(title.into()),
                    description: None,
                    document: file.input_document()?,
                    send_message,
                }
                .into()
            })
        }
        Source::Bytes { .. } => anyhow::bail!(
            "inline byte uploads must first be sent/uploaded and referenced by file_id"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_before_any_network() {
        let bad = Media::from_url(FileKind::Photo, "file:///secret");
        assert!(bad.input(None).is_err());
        let m = Media {
            source: Source::Bytes {
                name: "../x".into(),
                data: vec![1],
            },
            ..Default::default()
        };
        assert!(m.validate().is_err());
        assert!(
            Media {
                duration: f64::NAN,
                ..Media::from_url(FileKind::Video, "https://example.com/a.mp4")
            }
            .validate()
            .is_err()
        );
    }
    #[test]
    fn url_and_album_conversion() {
        let photo = Media::from_url(FileKind::Photo, "https://example.com/a.jpg");
        assert!(matches!(
            photo.input(None).unwrap(),
            tl::enums::InputMedia::PhotoExternal(_)
        ));
        let video = Media::from_url(FileKind::Video, "https://example.com/a.mp4");
        assert!(Media::validate_album(&[photo.clone(), video]).is_ok());
        assert!(
            Media::validate_album(&[
                photo.clone(),
                Media::from_url(FileKind::Audio, "https://example.com/a.mp3")
            ])
            .is_err()
        );
        assert!(Media::validate_album(&[photo]).is_err());
    }
    #[test]
    fn inline_media_and_rich_types() {
        let m = Media::from_url(FileKind::Photo, "https://example.com/a.jpg");
        let result = inline_result(
            "a",
            "Photo",
            grammers_client::message::InputMessage::new().html("<b>caption</b>"),
            Some(&m),
            None,
        )
        .unwrap();
        let tl::enums::InputBotInlineResult::Result(r) = result else {
            panic!("expected URL result")
        };
        assert_eq!(r.r#type, "photo");
        assert!(matches!(
            r.send_message,
            tl::enums::InputBotInlineMessage::MediaAuto(_)
        ));
        assert!(
            inline_result(
                "a",
                "",
                grammers_client::message::InputMessage::new(),
                Some(&m),
                Some("<h1>x</h1>")
            )
            .is_err()
        );
    }
    #[test]
    fn uploaded_audio_video_metadata() {
        let a = Media {
            kind: FileKind::Voice,
            duration: 10.0,
            ..Default::default()
        };
        assert!(
            matches!(&a.attributes("voice.ogg")[1],tl::enums::DocumentAttribute::Audio(a) if a.voice&&a.duration==10)
        );
        let v = Media {
            kind: FileKind::VideoNote,
            width: 240,
            height: 240,
            ..Default::default()
        };
        assert!(
            matches!(&v.attributes("clip.mp4")[1],tl::enums::DocumentAttribute::Video(v) if v.round_message&&v.w==240)
        );
    }
}
