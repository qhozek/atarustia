//! Serializable inline input content shared by inline, guest and web-app answers.
use anyhow::{Result, ensure};
use grammers_client::{
    message::{InputMessage, InputRichMessage},
    tl,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeoPoint {
    pub latitude: f64,
    pub longitude: f64,
    #[serde(default)]
    pub horizontal_accuracy: Option<f64>,
}
impl GeoPoint {
    pub fn to_tl(&self) -> Result<tl::enums::InputGeoPoint> {
        ensure!(
            self.latitude.is_finite() && (-90.0..=90.0).contains(&self.latitude),
            "latitude must be finite and between -90 and 90"
        );
        ensure!(
            self.longitude.is_finite() && (-180.0..=180.0).contains(&self.longitude),
            "longitude must be finite and between -180 and 180"
        );
        ensure!(
            self.horizontal_accuracy
                .is_none_or(|n| n.is_finite() && (0.0..=1500.0).contains(&n)),
            "horizontal accuracy must be finite and between 0 and 1500 metres"
        );
        Ok(tl::types::InputGeoPoint {
            lat: self.latitude,
            long: self.longitude,
            // Match original Python conversion: a false-y zero means absent, fractions truncate.
            accuracy_radius: self
                .horizontal_accuracy
                .filter(|n| *n != 0.0)
                .map(|n| n as i32),
        }
        .into())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum VenueProvider {
    Foursquare {
        id: String,
        #[serde(default)]
        category: String,
    },
    GooglePlaces {
        id: String,
        #[serde(default)]
        category: String,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum InputContent {
    Caption {
        caption: crate::bot_requests::Text,
        #[serde(default)]
        show_above_media: bool,
    },
    FormattedText {
        text: crate::bot_requests::Text,
        #[serde(default)]
        preview: crate::bot_requests::Preview,
    },
    RichText {
        rich: crate::bot_edits::RichText,
    },
    Text {
        text: String,
        #[serde(default)]
        html: bool,
        #[serde(default)]
        disable_web_page_preview: bool,
    },
    WebPage {
        text: String,
        #[serde(default)]
        html: bool,
        url: String,
        #[serde(default)]
        is_disabled: bool,
        #[serde(default)]
        prefer_large_media: bool,
        #[serde(default)]
        prefer_small_media: bool,
        #[serde(default)]
        show_above_text: bool,
    },
    Rich {
        html: String,
    },
    Contact {
        phone_number: String,
        first_name: String,
        #[serde(default)]
        last_name: String,
        #[serde(default)]
        vcard: String,
    },
    Location {
        point: GeoPoint,
        #[serde(default)]
        live_period: Option<i32>,
        #[serde(default)]
        heading: Option<i32>,
        #[serde(default)]
        proximity_alert_radius: Option<i32>,
    },
    Venue {
        point: GeoPoint,
        title: String,
        address: String,
        #[serde(default)]
        provider: Option<VenueProvider>,
    },
}
fn text_limit(value: &str, min: usize, max: usize, label: &str) -> Result<()> {
    let length = value.encode_utf16().count();
    ensure!(
        (min..=max).contains(&length),
        "{label} must contain {min}–{max} UTF-16 units"
    );
    Ok(())
}
fn text_content(text: &str, html: bool) -> Result<tl::types::InputBotInlineMessageText> {
    ensure!(text.len() <= 64 * 1024, "inline text source exceeds 64 KiB");
    let input = if html {
        InputMessage::new().html(text)
    } else {
        InputMessage::new().text(text)
    };
    let article = grammers_client::update::Article::new("", input);
    let tl::enums::InputBotInlineMessage::Text(mut content) = article.raw.send_message else {
        unreachable!("article text");
    };
    text_limit(&content.message, 1, 4096, "inline text")?;
    if content.entities.as_ref().is_some_and(Vec::is_empty) {
        content.entities = None;
    }
    Ok(content)
}
impl InputContent {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Text { .. }
            | Self::Caption { .. }
            | Self::WebPage { .. }
            | Self::Rich { .. }
            | Self::FormattedText { .. }
            | Self::RichText { .. } => "article",
            Self::Contact { .. } => "contact",
            Self::Location { .. } => "geo",
            Self::Venue { .. } => "venue",
        }
    }
    pub fn to_tl(
        &self,
        keyboard: &[Vec<crate::markup::Key>],
    ) -> Result<tl::enums::InputBotInlineMessage> {
        let markup = if keyboard.is_empty() {
            None
        } else {
            Some(crate::markup::keyboard(keyboard)?.raw)
        };
        Ok(match self {
            Self::Caption {
                caption,
                show_above_media,
            } => {
                let (message, entities) = caption.parse(1024, true)?;
                tl::types::InputBotInlineMessageMediaAuto {
                    message,
                    entities,
                    invert_media: *show_above_media,
                    reply_markup: markup,
                }
                .into()
            }
            Self::RichText { rich } => tl::types::InputBotInlineMessageRichMessage {
                rich_message: rich.to_tl()?,
                reply_markup: markup,
            }
            .into(),
            Self::FormattedText { text, preview } => {
                let (message, entities) = text.parse(4096, false)?;
                if preview.media()?.is_some() {
                    tl::types::InputBotInlineMessageMediaWebPage {
                        invert_media: preview.show_above_text,
                        force_large_media: preview.prefer_large_media,
                        force_small_media: preview.prefer_small_media,
                        optional: true,
                        message,
                        entities,
                        url: preview.url.clone().expect("validated preview URL"),
                        reply_markup: markup,
                    }
                    .into()
                } else {
                    tl::types::InputBotInlineMessageText {
                        no_webpage: preview.is_disabled,
                        invert_media: preview.show_above_text,
                        message,
                        entities,
                        reply_markup: markup,
                    }
                    .into()
                }
            }
            Self::Text {
                text,
                html,
                disable_web_page_preview,
            } => {
                let mut content = text_content(text, *html)?;
                content.no_webpage = *disable_web_page_preview;
                content.reply_markup = markup;
                content.into()
            }
            Self::WebPage {
                text,
                html,
                url,
                is_disabled,
                prefer_large_media,
                prefer_small_media,
                show_above_text,
            } => {
                ensure!(
                    !(*prefer_large_media && *prefer_small_media),
                    "preview cannot prefer both large and small media"
                );
                ensure!(url.len() <= 8192, "preview URL too long");
                let parsed = reqwest::Url::parse(url)?;
                ensure!(
                    matches!(parsed.scheme(), "https" | "http") && parsed.host_str().is_some(),
                    "preview URL must be HTTP(S)"
                );
                let mut content = text_content(text, *html)?;
                if *is_disabled {
                    content.no_webpage = true;
                    content.invert_media = *show_above_text;
                    content.reply_markup = markup;
                    content.into()
                } else {
                    tl::types::InputBotInlineMessageMediaWebPage {
                        invert_media: *show_above_text,
                        force_large_media: *prefer_large_media,
                        force_small_media: *prefer_small_media,
                        optional: true,
                        message: content.message,
                        entities: content.entities,
                        url: url.clone(),
                        reply_markup: markup,
                    }
                    .into()
                }
            }
            Self::Rich { html } => {
                ensure!(
                    !html.is_empty() && html.len() <= 64 * 1024,
                    "rich HTML must contain 1 byte–64 KiB"
                );
                tl::types::InputBotInlineMessageRichMessage {
                    rich_message: InputRichMessage::html(html).raw,
                    reply_markup: markup,
                }
                .into()
            }
            Self::Contact {
                phone_number,
                first_name,
                last_name,
                vcard,
            } => {
                text_limit(phone_number, 1, 128, "phone number")?;
                ensure!(
                    !phone_number.chars().any(char::is_control),
                    "phone number contains control characters"
                );
                text_limit(first_name, 1, 64, "contact first name")?;
                text_limit(last_name, 0, 64, "contact last name")?;
                ensure!(vcard.len() <= 2048, "vCard exceeds 2048 bytes");
                tl::types::InputBotInlineMessageMediaContact {
                    phone_number: phone_number.clone(),
                    first_name: first_name.clone(),
                    last_name: last_name.clone(),
                    vcard: vcard.clone(),
                    reply_markup: markup,
                }
                .into()
            }
            Self::Location {
                point,
                live_period,
                heading,
                proximity_alert_radius,
            } => {
                ensure!(
                    live_period.is_none_or(|n| (60..=86400).contains(&n) || n == i32::MAX),
                    "live period must be 60–86400 seconds or i32::MAX"
                );
                ensure!(
                    heading.is_none_or(|n| (1..=360).contains(&n)),
                    "heading must be 1–360 degrees"
                );
                ensure!(
                    proximity_alert_radius.is_none_or(|n| (1..=100000).contains(&n)),
                    "proximity alert radius must be 1–100000 metres"
                );
                tl::types::InputBotInlineMessageMediaGeo {
                    geo_point: point.to_tl()?,
                    heading: *heading,
                    period: *live_period,
                    proximity_notification_radius: *proximity_alert_radius,
                    reply_markup: markup,
                }
                .into()
            }
            Self::Venue {
                point,
                title,
                address,
                provider,
            } => {
                text_limit(title, 1, 256, "venue title")?;
                text_limit(address, 1, 1024, "venue address")?;
                let (name, id, category) = match provider {
                    Some(VenueProvider::Foursquare { id, category }) => {
                        ("foursquare", id.as_str(), category.as_str())
                    }
                    Some(VenueProvider::GooglePlaces { id, category }) => {
                        ("gplaces", id.as_str(), category.as_str())
                    }
                    None => ("", "", ""),
                };
                text_limit(
                    id,
                    usize::from(provider.is_some()),
                    256,
                    "venue provider ID",
                )?;
                text_limit(category, 0, 256, "venue category")?;
                tl::types::InputBotInlineMessageMediaVenue {
                    geo_point: point.to_tl()?,
                    title: title.clone(),
                    address: address.clone(),
                    provider: name.into(),
                    venue_id: id.into(),
                    venue_type: category.into(),
                    reply_markup: markup,
                }
                .into()
            }
        })
    }
}

/// Build one result, optionally retaining cached/URL media while replacing the sent content.
#[allow(clippy::too_many_arguments)]
pub fn result(
    id: &str,
    title: &str,
    description: Option<&str>,
    url: Option<&str>,
    thumbnail_url: Option<&str>,
    media: Option<&crate::media::Media>,
    content: &InputContent,
    keyboard: &[Vec<crate::markup::Key>],
) -> Result<tl::enums::InputBotInlineResult> {
    ensure!(
        !id.is_empty() && id.len() <= 64,
        "inline result ID must be 1–64 bytes"
    );
    text_limit(title, 1, 256, "result title")?;
    if let Some(text) = description {
        text_limit(text, 0, 1024, "result description")?;
    }
    for url in [url, thumbnail_url].into_iter().flatten() {
        ensure!(url.len() <= 8192, "result URL too long");
        let parsed = reqwest::Url::parse(url)?;
        ensure!(
            parsed.scheme() == "https" && parsed.host_str().is_some(),
            "result URL must be HTTPS"
        );
    }
    let send_message = content.to_tl(keyboard)?;
    ensure!(
        !matches!(content, InputContent::Caption { .. }) || media.is_some(),
        "caption input content requires media"
    );
    if let Some(media) = media {
        ensure!(
            !matches!(
                content,
                InputContent::Rich { .. } | InputContent::RichText { .. }
            ),
            "rich content cannot be combined with inline media"
        );
        let mut result =
            crate::media::inline_result(id, title, InputMessage::new(), Some(media), None)?;
        match &mut result {
            tl::enums::InputBotInlineResult::Result(r) => {
                r.send_message = send_message;
                r.description = description.map(str::to_owned);
                r.url = url.map(str::to_owned);
                if let Some(thumb) = thumbnail_url {
                    r.thumb = Some(thumbnail(thumb));
                }
            }
            tl::enums::InputBotInlineResult::Photo(r) => {
                ensure!(
                    url.is_none() && thumbnail_url.is_none() && description.is_none(),
                    "cached photo result has no URL/thumbnail/description fields"
                );
                r.send_message = send_message;
            }
            tl::enums::InputBotInlineResult::Document(r) => {
                ensure!(
                    url.is_none() && thumbnail_url.is_none(),
                    "cached document result has no URL/thumbnail fields"
                );
                r.send_message = send_message;
                r.description = description.map(str::to_owned);
            }
            _ => anyhow::bail!("unsupported inline result constructor"),
        }
        Ok(result)
    } else {
        Ok(tl::types::InputBotInlineResult {
            id: id.into(),
            r#type: content.kind().into(),
            title: Some(title.into()),
            description: description.map(str::to_owned),
            url: url.map(str::to_owned),
            thumb: thumbnail_url.map(thumbnail),
            content: None,
            send_message,
        }
        .into())
    }
}
fn thumbnail(url: &str) -> tl::enums::InputWebDocument {
    tl::types::InputWebDocument {
        url: url.into(),
        size: 0,
        mime_type: "image/jpeg".into(),
        attributes: vec![],
    }
    .into()
}
