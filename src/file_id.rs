//! Bot API v4 file identifiers, matching the format emitted by the Python port.
//! IDs are capabilities (contain an access hash/reference): never put them in logs.
use anyhow::{Context, Result, bail, ensure};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use grammers_client::{
    media::{Downloadable, Media},
    tl,
};
use serde::{Deserialize, Serialize};

const REFERENCE: i32 = 1 << 25;
const WEB: i32 = 1 << 24;
const VERSION: u8 = 4;
const SUBVERSION: u8 = 54;
const MAX_ID: usize = 24 * 1024;
const MAX_REFERENCE: usize = 8192;
pub type MediaFile = (FileRef, Option<u64>, Option<String>, Option<String>);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileKind {
    Photo,
    Document,
    Video,
    Audio,
    Voice,
    Animation,
    Sticker,
    VideoNote,
}
impl FileKind {
    pub fn code(self) -> i32 {
        match self {
            Self::Photo => 2,
            Self::Voice => 3,
            Self::Video => 4,
            Self::Document => 5,
            Self::Sticker => 8,
            Self::Audio => 9,
            Self::Animation => 10,
            Self::VideoNote => 13,
        }
    }
    fn from_code(code: i32) -> Result<Self> {
        Ok(match code {
            2 => Self::Photo,
            3 => Self::Voice,
            4 => Self::Video,
            5 | 17 => Self::Document,
            8 => Self::Sticker,
            9 => Self::Audio,
            10 => Self::Animation,
            13 => Self::VideoNote,
            _ => bail!("unsupported file ID type"),
        })
    }
}
#[derive(Clone, PartialEq, Eq)]
pub struct FileRef {
    pub kind: FileKind,
    pub dc: i32,
    pub id: i64,
    pub access_hash: i64,
    pub reference: Vec<u8>,
    pub thumb: String,
}
impl std::fmt::Debug for FileRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileRef")
            .field("kind", &self.kind)
            .field("dc", &self.dc)
            .finish_non_exhaustive()
    }
}
impl FileRef {
    pub fn validate(&self) -> Result<()> {
        ensure!((1..=1000).contains(&self.dc), "invalid file datacenter");
        ensure!(self.id != 0, "invalid media ID");
        ensure!(
            self.reference.len() <= MAX_REFERENCE,
            "file reference too large"
        );
        ensure!(
            self.thumb.is_empty() || self.thumb.len() == 1 && self.thumb.is_ascii(),
            "thumbnail type must be a single ASCII character"
        );
        Ok(())
    }
    pub fn encode(&self) -> Result<String> {
        self.validate()?;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&(self.kind.code() | REFERENCE).to_le_bytes());
        bytes.extend_from_slice(&self.dc.to_le_bytes());
        write_tl_bytes(&mut bytes, &self.reference)?;
        bytes.extend_from_slice(&self.id.to_le_bytes());
        bytes.extend_from_slice(&self.access_hash.to_le_bytes());
        if self.kind == FileKind::Photo {
            bytes.extend_from_slice(&1i32.to_le_bytes());
            bytes.extend_from_slice(&2i32.to_le_bytes());
            bytes.extend_from_slice(
                &(self.thumb.as_bytes().first().copied().unwrap_or(b'x') as i32).to_le_bytes(),
            );
        }
        bytes.extend_from_slice(&[SUBVERSION, VERSION]);
        Ok(URL_SAFE_NO_PAD.encode(rle_encode(&bytes)))
    }
    pub fn decode(id: &str) -> Result<Self> {
        ensure!(
            !id.is_empty() && id.len() <= MAX_ID,
            "invalid file ID length"
        );
        let bytes = URL_SAFE_NO_PAD
            .decode(id)
            .context("invalid file ID encoding")?;
        let bytes = rle_decode(&bytes)?;
        ensure!(bytes.len() >= 26, "truncated file ID");
        ensure!(
            bytes[bytes.len() - 2..] == [SUBVERSION, VERSION],
            "unsupported file ID version"
        );
        let mut reader = Reader::new(&bytes[..bytes.len() - 2]);
        let flags = reader.i32()?;
        ensure!(flags & WEB == 0, "web file IDs are not supported");
        ensure!(
            flags & !(REFERENCE | WEB | 0xff) == 0,
            "unknown file ID flags"
        );
        let kind = FileKind::from_code(flags & 0xff)?;
        let dc = reader.i32()?;
        let reference = if flags & REFERENCE != 0 {
            reader.tl_bytes()?
        } else {
            vec![]
        };
        let media_id = reader.i64()?;
        let access_hash = reader.i64()?;
        let thumb = if kind == FileKind::Photo {
            ensure!(
                reader.i32()? == 1 && reader.i32()? == 2,
                "unsupported photo source"
            );
            let ch = u8::try_from(reader.i32()?).context("invalid photo thumbnail")?;
            ensure!(ch.is_ascii_graphic(), "invalid photo thumbnail");
            char::from(ch).to_string()
        } else {
            String::new()
        };
        ensure!(reader.remaining() == 0, "unexpected file ID trailing data");
        let file = Self {
            kind,
            dc,
            id: media_id,
            access_hash,
            reference,
            thumb,
        };
        file.validate()?;
        Ok(file)
    }
    pub fn unique_id(&self) -> String {
        let mut bytes = 2i32.to_le_bytes().to_vec();
        bytes.extend_from_slice(&self.id.to_le_bytes());
        URL_SAFE_NO_PAD.encode(rle_encode(&bytes))
    }
    pub fn input_photo(&self) -> Result<tl::enums::InputPhoto> {
        ensure!(self.kind == FileKind::Photo, "file is not a photo");
        Ok(tl::types::InputPhoto {
            id: self.id,
            access_hash: self.access_hash,
            file_reference: self.reference.clone(),
        }
        .into())
    }
    pub fn input_document(&self) -> Result<tl::enums::InputDocument> {
        ensure!(self.kind != FileKind::Photo, "file is not a document");
        Ok(tl::types::InputDocument {
            id: self.id,
            access_hash: self.access_hash,
            file_reference: self.reference.clone(),
        }
        .into())
    }
    pub fn input_media(&self) -> Result<tl::enums::InputMedia> {
        self.validate()?;
        Ok(if self.kind == FileKind::Photo {
            tl::types::InputMediaPhoto {
                spoiler: false,
                id: self.input_photo()?,
                ttl_seconds: None,
                live_photo: false,
                video: None,
            }
            .into()
        } else {
            tl::types::InputMediaDocument {
                spoiler: false,
                id: self.input_document()?,
                video_cover: None,
                video_timestamp: None,
                ttl_seconds: None,
                query: None,
            }
            .into()
        })
    }
    pub fn from_media(media: &Media) -> Result<MediaFile> {
        let document = match media {
            Media::Photo(photo) => {
                let Some(tl::enums::Photo::Photo(p)) = &photo.raw.photo else {
                    bail!("empty photo");
                };
                let thumb = p
                    .sizes
                    .iter()
                    .rev()
                    .find_map(|s| match s {
                        tl::enums::PhotoSize::Size(s) => Some(s.r#type.clone()),
                        tl::enums::PhotoSize::Progressive(s) => Some(s.r#type.clone()),
                        _ => None,
                    })
                    .unwrap_or_else(|| "x".into());
                let file = Self {
                    kind: FileKind::Photo,
                    dc: p.dc_id,
                    id: p.id,
                    access_hash: p.access_hash,
                    reference: p.file_reference.clone(),
                    thumb,
                };
                file.validate()?;
                return Ok((
                    file,
                    photo.size().map(|s| s as u64),
                    None,
                    Some("image/jpeg".into()),
                ));
            }
            Media::Document(d) => d,
            Media::Sticker(s) => &s.document,
            _ => bail!("message has no downloadable photo or document"),
        };
        let Some(tl::enums::Document::Document(d)) = &document.raw.document else {
            bail!("empty document");
        };
        let kind = classify(&d.attributes);
        let file = Self {
            kind,
            dc: d.dc_id,
            id: d.id,
            access_hash: d.access_hash,
            reference: d.file_reference.clone(),
            thumb: String::new(),
        };
        file.validate()?;
        Ok((
            file,
            u64::try_from(d.size).ok(),
            document.name().map(str::to_owned),
            Some(d.mime_type.clone()),
        ))
    }
}
impl Downloadable for FileRef {
    fn to_raw_input_location(&self) -> Option<tl::enums::InputFileLocation> {
        Some(if self.kind == FileKind::Photo {
            tl::types::InputPhotoFileLocation {
                id: self.id,
                access_hash: self.access_hash,
                file_reference: self.reference.clone(),
                thumb_size: self.thumb.clone(),
            }
            .into()
        } else {
            tl::types::InputDocumentFileLocation {
                id: self.id,
                access_hash: self.access_hash,
                file_reference: self.reference.clone(),
                thumb_size: self.thumb.clone(),
            }
            .into()
        })
    }
    fn dc_id(&self) -> Option<i32> {
        Some(self.dc)
    }
}
pub fn classify(attributes: &[tl::enums::DocumentAttribute]) -> FileKind {
    if attributes.iter().any(|a| {
        matches!(
            a,
            tl::enums::DocumentAttribute::Sticker(_) | tl::enums::DocumentAttribute::CustomEmoji(_)
        )
    }) {
        return FileKind::Sticker;
    }
    if attributes
        .iter()
        .any(|a| matches!(a, tl::enums::DocumentAttribute::Animated))
    {
        return FileKind::Animation;
    }
    for attr in attributes {
        match attr {
            tl::enums::DocumentAttribute::Audio(a) => {
                return if a.voice {
                    FileKind::Voice
                } else {
                    FileKind::Audio
                };
            }
            tl::enums::DocumentAttribute::Video(v) => {
                return if v.round_message {
                    FileKind::VideoNote
                } else {
                    FileKind::Video
                };
            }
            _ => {}
        }
    }
    FileKind::Document
}
pub fn encode_inline_id(id: &tl::enums::InputBotInlineMessageId) -> String {
    use tl::Serializable;
    URL_SAFE_NO_PAD.encode(&id.to_bytes()[4..])
}
pub fn decode_inline_id(id: &str) -> Result<tl::enums::InputBotInlineMessageId> {
    use tl::Deserializable;
    ensure!(id.len() <= 64, "inline message ID too long");
    let bytes = URL_SAFE_NO_PAD.decode(id)?;
    let mut cursor = tl::deserialize::Cursor::from_slice(&bytes);
    Ok(match bytes.len() {
        20 => tl::types::InputBotInlineMessageId::deserialize(&mut cursor)?.into(),
        24 => tl::types::InputBotInlineMessageId64::deserialize(&mut cursor)?.into(),
        _ => bail!("invalid inline message ID length"),
    })
}

fn rle_encode(bytes: &[u8]) -> Vec<u8> {
    let mut out = vec![];
    let mut zeros = 0u8;
    for byte in bytes {
        if *byte == 0 {
            zeros += 1;
            if zeros == 255 {
                out.extend_from_slice(&[0, zeros]);
                zeros = 0;
            }
        } else {
            if zeros > 0 {
                out.extend_from_slice(&[0, zeros]);
                zeros = 0;
            }
            out.push(*byte);
        }
    }
    if zeros > 0 {
        out.extend_from_slice(&[0, zeros]);
    }
    out
}
fn rle_decode(bytes: &[u8]) -> Result<Vec<u8>> {
    let mut out = vec![];
    let mut pos = 0;
    while pos < bytes.len() {
        let byte = bytes[pos];
        pos += 1;
        if byte == 0 {
            let count = *bytes.get(pos).context("truncated zero run")?;
            ensure!(count > 0, "zero-length run");
            pos += 1;
            ensure!(
                out.len() + usize::from(count) <= MAX_ID,
                "expanded file ID too large"
            );
            out.resize(out.len() + usize::from(count), 0);
        } else {
            ensure!(out.len() < MAX_ID, "expanded file ID too large");
            out.push(byte);
        }
    }
    Ok(out)
}
fn write_tl_bytes(out: &mut Vec<u8>, bytes: &[u8]) -> Result<()> {
    ensure!(bytes.len() <= MAX_REFERENCE, "file reference too long");
    let header = if bytes.len() < 254 {
        out.push(bytes.len() as u8);
        1
    } else {
        out.push(254);
        let n = bytes.len() as u32;
        out.extend_from_slice(&n.to_le_bytes()[..3]);
        4
    };
    out.extend_from_slice(bytes);
    let padding = (4 - (bytes.len() + header) % 4) % 4;
    out.resize(out.len() + padding, 0);
    Ok(())
}
struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}
impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }
    fn remaining(&self) -> usize {
        self.bytes.len() - self.pos
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        ensure!(n <= self.remaining(), "truncated file ID");
        let b = &self.bytes[self.pos..self.pos + n];
        self.pos += n;
        Ok(b)
    }
    fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into()?))
    }
    fn i64(&mut self) -> Result<i64> {
        Ok(i64::from_le_bytes(self.take(8)?.try_into()?))
    }
    fn tl_bytes(&mut self) -> Result<Vec<u8>> {
        let first = self.take(1)?[0];
        let (n, header) = if first == 254 {
            let b = self.take(3)?;
            (
                usize::from(b[0]) | (usize::from(b[1]) << 8) | (usize::from(b[2]) << 16),
                4,
            )
        } else {
            ensure!(first < 254, "invalid TL byte header");
            (usize::from(first), 1)
        };
        ensure!(n <= MAX_REFERENCE, "reference too long");
        let b = self.take(n)?.to_vec();
        let pad = (4 - (n + header) % 4) % 4;
        ensure!(
            self.take(pad)?.iter().all(|b| *b == 0),
            "invalid TL padding"
        );
        Ok(b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn damaged_ids_do_not_panic() {
        for length in 0..512 {
            let bytes: Vec<u8> = (0..length)
                .map(|i| ((i * 31 + length) % 256) as u8)
                .collect();
            let encoded = URL_SAFE_NO_PAD.encode(bytes);
            let _ = FileRef::decode(&encoded);
            let _ = decode_inline_id(&encoded);
        }
    }
    #[test]
    fn python_atarustia_compatibility_vectors() {
        let mut file = FileRef {
            kind: FileKind::Photo,
            dc: 2,
            id: 123456789,
            access_hash: -555,
            reference: vec![0, 1, 2, 255],
            thumb: "w".into(),
        };
        let photo = "AgACAgIAAwQAAQEC_wADFc1bBwAE1f3_______8BAAMCAAN3AAM2BA";
        assert_eq!(file.encode().unwrap(), photo);
        assert_eq!(FileRef::decode(photo).unwrap(), file);
        file.kind = FileKind::Document;
        file.thumb.clear();
        let document = "BQACAgIAAwQAAQEC_wADFc1bBwAE1f3_______82BA";
        assert_eq!(file.encode().unwrap(), document);
        assert_eq!(file.unique_id(), "AgADFc1bBwAE");
        assert_eq!(FileRef::decode(document).unwrap(), file);
        let inline = tl::types::InputBotInlineMessageId64 {
            dc_id: 2,
            owner_id: 123,
            id: 42,
            access_hash: -55,
        }
        .into();
        assert_eq!(
            encode_inline_id(&inline),
            "AgAAAHsAAAAAAAAAKgAAAMn_________"
        );
    }
    #[test]
    fn file_ids_round_trip_all_kinds() {
        for kind in [
            FileKind::Photo,
            FileKind::Document,
            FileKind::Video,
            FileKind::Audio,
            FileKind::Voice,
            FileKind::Animation,
            FileKind::Sticker,
            FileKind::VideoNote,
        ] {
            for size in [0, 3, 253, 254, 800] {
                let file = FileRef {
                    kind,
                    dc: 2,
                    id: 123456789,
                    access_hash: -555,
                    reference: vec![42; size],
                    thumb: if kind == FileKind::Photo {
                        "w".into()
                    } else {
                        String::new()
                    },
                };
                assert_eq!(FileRef::decode(&file.encode().unwrap()).unwrap(), file);
                assert!(file.input_media().is_ok());
            }
        }
    }
    #[test]
    fn bounded_rejection() {
        for id in ["", "!!!", "AAAA", "AA"] {
            assert!(FileRef::decode(id).is_err());
        }
        assert!(rle_decode(&[0]).is_err());
        assert!(rle_decode(&[0, 0]).is_err());
        assert!(FileRef::decode(&"A".repeat(MAX_ID + 1)).is_err());
    }
    #[test]
    fn inline_ids_match_wire_layout() {
        let id: tl::enums::InputBotInlineMessageId = tl::types::InputBotInlineMessageId64 {
            dc_id: 2,
            owner_id: 123,
            id: 42,
            access_hash: -55,
        }
        .into();
        assert_eq!(decode_inline_id(&encode_inline_id(&id)).unwrap(), id);
        assert!(decode_inline_id("AAAA").is_err());
    }
    #[test]
    fn zero_runs_and_unique_id() {
        let bytes = vec![0; 800];
        assert_eq!(rle_decode(&rle_encode(&bytes)).unwrap(), bytes);
        let mut file = FileRef {
            kind: FileKind::Document,
            dc: 2,
            id: 42,
            access_hash: 55,
            reference: vec![],
            thumb: String::new(),
        };
        let unique = file.unique_id();
        file.access_hash = 99;
        file.reference = vec![1, 2, 3];
        assert_eq!(unique, file.unique_id());
    }
}
