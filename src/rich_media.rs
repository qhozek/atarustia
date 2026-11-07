//! Resolve rich HTML photos before inline delivery. No visible staging messages.
use anyhow::{Result, ensure};
use grammers_client::{Client, tl};
use html5ever::{
    tendril::StrTendril,
    tokenizer::{BufferQueue, Token, TokenSink, TokenSinkResult, Tokenizer},
};
use std::{cell::RefCell, sync::LazyLock};

static IMAGES: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(concat!(
        r"(?s:<!--.*?-->)|(?i-u:<pre)\b[^>]*>(?s:.*?)(?i-u:</pre>)|",
        r"(?i-u:<code)\b[^>]*>(?s:.*?)(?i-u:</code>)|",
        r#"(?P<image>(?i-u:<img)\b(?:[^>\"']|\"[^\"]*\"|'[^']*')*>)"#
    ))
    .unwrap()
});
#[derive(Clone, Debug)]
pub struct Photo {
    pub tag: String,
    pub url: String,
    attrs: Vec<(String, String)>,
}
pub fn photos(source: &str) -> Result<Vec<Photo>> {
    ensure!(
        source.len() <= 128 * 1024,
        "rich HTML source exceeds 128 KiB"
    );
    struct Sink(RefCell<Vec<(String, String)>>);
    impl TokenSink for Sink {
        type Handle = ();
        fn process_token(&self, token: Token, _: u64) -> TokenSinkResult<()> {
            if let Token::TagToken(tag) = token
                && &*tag.name == "img"
            {
                *self.0.borrow_mut() = tag
                    .attrs
                    .iter()
                    .map(|attr| (attr.name.local.to_string(), attr.value.to_string()))
                    .collect();
            }
            TokenSinkResult::Continue
        }
    }
    let mut result = vec![];
    for capture in IMAGES.captures_iter(source) {
        let Some(tag) = capture.name("image") else {
            continue;
        };
        let tokenizer = Tokenizer::new(Sink(RefCell::new(vec![])), Default::default());
        let input = BufferQueue::default();
        input.push_back(StrTendril::from(tag.as_str()));
        let _ = tokenizer.feed(&input);
        tokenizer.end();
        let attrs = tokenizer.sink.0.into_inner();
        if let Some(url) = attrs
            .iter()
            .find(|(name, _)| name == "src")
            .map(|(_, value)| value.clone())
            && (url.starts_with("https://") || url.starts_with("http://"))
        {
            // Same HTTPS/credential policy as form media; Telegram performs the fetch.
            crate::media::Media::from_url(crate::file_id::FileKind::Photo, &url).validate()?;
            result.push(Photo {
                tag: tag.as_str().into(),
                url,
                attrs,
            });
        }
    }
    ensure!(
        result.len() <= 50,
        "rich message exceeds 50 photo attachments"
    );
    Ok(result)
}

/// Pure binding step. Media bytes/references remain in files, never in the HTML.
pub fn bind(
    source: &str,
    photos: &[(Photo, tl::enums::InputPhoto)],
) -> tl::enums::InputRichMessage {
    let nonce = crate::web::random_token();
    let mut replacements = vec![];
    let files = photos
        .iter()
        .enumerate()
        .map(|(index, (photo, input))| {
            let id = format!("ataraxia_photo_{}_{index}", &nonce[..16]);
            let attrs = photo
                .attrs
                .iter()
                .map(|(name, value)| {
                    format!(
                        " {name}=\"{}\"",
                        crate::output::escape(&if name == "src" {
                            format!("tg://photo?id={id}")
                        } else {
                            value.clone()
                        })
                    )
                })
                .collect::<String>();
            replacements.push((photo.tag.clone(), format!("<img{attrs}/>")));
            tl::types::InputRichFilePhoto {
                id,
                photo: input.clone(),
            }
            .into()
        })
        .collect::<Vec<_>>();
    let mut index = 0;
    let html = IMAGES
        .replace_all(source, |capture: &regex::Captures<'_>| {
            if let Some(tag) = capture.name("image")
                && let Some((before, after)) = replacements.get(index)
                && tag.as_str() == before
            {
                index += 1;
                after.clone()
            } else {
                capture[0].to_owned()
            }
        })
        .into_owned();
    tl::types::InputRichMessageHtml {
        rtl: false,
        noautolink: false,
        html,
        files: (!files.is_empty()).then_some(files),
    }
    .into()
}

pub async fn prepare(client: &Client, rich: &mut tl::enums::InputRichMessage) -> Result<()> {
    let tl::enums::InputRichMessage::Html(content) = rich else {
        return Ok(());
    };
    let photos = photos(&content.html)?;
    ensure!(
        photos.len() + content.files.as_ref().map_or(0, Vec::len) <= 50,
        "rich message exceeds 50 media attachments"
    );
    let mut stored = vec![];
    for photo in photos {
        let media = if photo.url == crate::info::BANNER {
            crate::media::Media {
                kind: crate::file_id::FileKind::Photo,
                source: crate::media::Source::Bytes {
                    name: "ataraxia.png".into(),
                    data: include_bytes!("../assets/ataraxia.png").to_vec(),
                },
                ..Default::default()
            }
        } else {
            crate::media::Media::from_url(crate::file_id::FileKind::Photo, &photo.url)
        };
        let result = client
            .invoke(&tl::functions::messages::UploadMedia {
                business_connection_id: None,
                peer: tl::enums::InputPeer::PeerSelf,
                media: media.prepare(client).await?,
            })
            .await?;
        let tl::enums::InputMedia::Photo(input) = crate::bot_requests::stored_media(result)? else {
            anyhow::bail!("rich photo upload returned a document")
        };
        stored.push((photo, input.id));
    }
    if !stored.is_empty() {
        let tl::enums::InputRichMessage::Html(mut bound) = bind(&content.html, &stored) else {
            unreachable!()
        };
        bound.rtl = content.rtl;
        bound.noautolink = content.noautolink;
        let mut files = content.files.take().unwrap_or_default();
        files.extend(bound.files.take().unwrap_or_default());
        bound.files = Some(files);
        *content = bound;
    }
    Ok(())
}
