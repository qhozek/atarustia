//! Bot API media objects. File capabilities use the same codec as native send/download.
use crate::{
    botapi::{File, MessageEntity},
    file_id::{FileKind, FileRef},
};
use grammers_client::tl;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PhotoSize {
    pub file_id: String,
    pub file_unique_id: String,
    pub width: i32,
    pub height: i32,
    pub file_size: Option<u64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Document {
    #[serde(flatten)]
    pub file: File,
    pub thumbnail: Option<PhotoSize>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Video {
    #[serde(flatten)]
    pub document: Document,
    pub width: i32,
    pub height: i32,
    pub duration: f64,
    pub cover: Vec<PhotoSize>,
    pub start_timestamp: Option<i32>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Animation {
    #[serde(flatten)]
    pub document: Document,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub duration: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Audio {
    #[serde(flatten)]
    pub document: Document,
    pub duration: i32,
    pub performer: Option<String>,
    pub title: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Voice {
    #[serde(flatten)]
    pub file: File,
    pub duration: i32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VideoNote {
    #[serde(flatten)]
    pub file: File,
    pub length: i32,
    pub duration: f64,
    pub thumbnail: Option<PhotoSize>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Sticker {
    #[serde(flatten)]
    pub file: File,
    #[serde(rename = "type")]
    pub kind: String,
    pub width: i32,
    pub height: i32,
    pub is_animated: bool,
    pub is_video: bool,
    pub thumbnail: Option<PhotoSize>,
    pub emoji: Option<String>,
    pub set_name: Option<String>,
    pub custom_emoji_id: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Location {
    pub latitude: f64,
    pub longitude: f64,
    pub horizontal_accuracy: Option<i32>,
    pub live_period: Option<i32>,
    pub heading: Option<i32>,
    pub proximity_alert_radius: Option<i32>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Venue {
    pub location: Option<Location>,
    pub title: String,
    pub address: String,
    pub foursquare_id: Option<String>,
    pub foursquare_type: Option<String>,
    pub google_place_id: Option<String>,
    pub google_place_type: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Contact {
    pub phone_number: String,
    pub first_name: String,
    pub last_name: Option<String>,
    pub user_id: Option<i64>,
    pub vcard: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Dice {
    pub emoji: String,
    pub value: i32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PollOption {
    pub text: String,
    pub text_entities: Vec<MessageEntity>,
    pub voter_count: Option<i32>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Poll {
    pub id: String,
    pub question: String,
    pub question_entities: Vec<MessageEntity>,
    pub options: Vec<PollOption>,
    pub total_voter_count: Option<i32>,
    pub is_closed: bool,
    pub is_anonymous: bool,
    #[serde(rename = "type")]
    pub kind: String,
    pub allows_multiple_answers: bool,
    pub open_period: Option<i32>,
    pub close_date: Option<i32>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Fields {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub photo: Vec<PhotoSize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub animation: Option<Animation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio: Option<Audio>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document: Option<Document>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sticker: Option<Sticker>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video: Option<Video>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video_note: Option<VideoNote>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice: Option<Voice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contact: Option<Contact>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dice: Option<Dice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub poll: Option<Poll>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub venue: Option<Venue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<Location>,
}
pub fn photo_sizes(value: &tl::enums::Photo) -> Vec<PhotoSize> {
    let tl::enums::Photo::Photo(p) = value else {
        return vec![];
    };
    p.sizes
        .iter()
        .filter_map(|s| {
            let (thumb, width, height, size) = match s {
                tl::enums::PhotoSize::Size(s) => {
                    (s.r#type.clone(), s.w, s.h, u64::try_from(s.size).ok())
                }
                tl::enums::PhotoSize::Progressive(s) => (
                    s.r#type.clone(),
                    s.w,
                    s.h,
                    s.sizes.iter().max().and_then(|n| u64::try_from(*n).ok()),
                ),
                _ => return None,
            };
            let reference = FileRef {
                kind: FileKind::Photo,
                dc: p.dc_id,
                id: p.id,
                access_hash: p.access_hash,
                reference: p.file_reference.clone(),
                thumb,
            };
            Some(PhotoSize {
                file_id: reference.encode().ok()?,
                file_unique_id: reference.unique_id(),
                width,
                height,
                file_size: size,
            })
        })
        .collect()
}
fn thumb(document: &tl::types::Document) -> Option<PhotoSize> {
    // Distinct document-thumbnail file IDs are not supported by the current codec.
    // Do not expose a full-document ID as a thumbnail capability.
    let _ = document;
    None
}
fn location(value: &tl::enums::GeoPoint) -> Option<Location> {
    let tl::enums::GeoPoint::Point(p) = value else {
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
impl Fields {
    pub fn from_tl(value: Option<&tl::enums::MessageMedia>) -> Self {
        let mut result = Self::default();
        let Some(value) = value else {
            return result;
        };
        match value {
            tl::enums::MessageMedia::Photo(p) => {
                result.photo = p.photo.as_ref().map(photo_sizes).unwrap_or_default()
            }
            tl::enums::MessageMedia::Document(d) => {
                let Some(tl::enums::Document::Document(raw)) = &d.document else {
                    return result;
                };
                let native = grammers_client::media::Media::from_raw(value.clone());
                let Some(file) = native.as_ref().and_then(|m| File::from_media(m).ok()) else {
                    return result;
                };
                let thumbnail = thumb(raw);
                let document = Document {
                    file: file.clone(),
                    thumbnail: thumbnail.clone(),
                };
                let video = raw.attributes.iter().find_map(|a| {
                    if let tl::enums::DocumentAttribute::Video(v) = a {
                        Some(v)
                    } else {
                        None
                    }
                });
                let audio = raw.attributes.iter().find_map(|a| {
                    if let tl::enums::DocumentAttribute::Audio(v) = a {
                        Some(v)
                    } else {
                        None
                    }
                });
                match crate::file_id::classify(&raw.attributes) {
                    FileKind::Sticker => {
                        let mut width = video.map(|v| v.w).unwrap_or(0);
                        let mut height = video.map(|v| v.h).unwrap_or(0);
                        let mut emoji = None;
                        let mut set_name = None;
                        let mut custom = false;
                        let mut mask = false;
                        for a in &raw.attributes {
                            match a {
                                tl::enums::DocumentAttribute::ImageSize(s) => {
                                    width = s.w;
                                    height = s.h;
                                }
                                tl::enums::DocumentAttribute::Sticker(s) => {
                                    emoji = (!s.alt.is_empty()).then(|| s.alt.clone());
                                    mask = s.mask;
                                    if let tl::enums::InputStickerSet::ShortName(s) = &s.stickerset
                                    {
                                        set_name = Some(s.short_name.clone());
                                    }
                                }
                                tl::enums::DocumentAttribute::CustomEmoji(s) => {
                                    custom = true;
                                    emoji = (!s.alt.is_empty()).then(|| s.alt.clone());
                                    if let tl::enums::InputStickerSet::ShortName(s) = &s.stickerset
                                    {
                                        set_name = Some(s.short_name.clone());
                                    }
                                }
                                _ => {}
                            }
                        }
                        result.sticker = Some(Sticker {
                            file,
                            kind: if custom {
                                "custom_emoji"
                            } else if mask {
                                "mask"
                            } else {
                                "regular"
                            }
                            .into(),
                            width,
                            height,
                            is_animated: raw.mime_type == "application/x-tgsticker",
                            is_video: raw.mime_type == "video/webm",
                            thumbnail,
                            emoji,
                            set_name,
                            custom_emoji_id: custom.then(|| raw.id.to_string()),
                        });
                    }
                    FileKind::Animation => {
                        result.animation = Some(Animation {
                            document,
                            width: video.map(|v| v.w),
                            height: video.map(|v| v.h),
                            duration: video.map(|v| v.duration).unwrap_or(0.),
                        })
                    }
                    FileKind::VideoNote => {
                        if let Some(v) = video {
                            result.video_note = Some(VideoNote {
                                file,
                                length: v.w,
                                duration: v.duration,
                                thumbnail,
                            });
                        }
                    }
                    FileKind::Video => {
                        if let Some(v) = video {
                            result.video = Some(Video {
                                document,
                                width: v.w,
                                height: v.h,
                                duration: v.duration,
                                cover: d.video_cover.as_ref().map(photo_sizes).unwrap_or_default(),
                                start_timestamp: d.video_timestamp,
                            });
                        }
                    }
                    FileKind::Voice => {
                        if let Some(a) = audio {
                            result.voice = Some(Voice {
                                file,
                                duration: a.duration,
                            });
                        }
                    }
                    FileKind::Audio => {
                        if let Some(a) = audio {
                            result.audio = Some(Audio {
                                document,
                                duration: a.duration,
                                performer: a.performer.clone(),
                                title: a.title.clone(),
                            });
                        }
                    }
                    _ => result.document = Some(document),
                }
            }
            tl::enums::MessageMedia::Contact(c) => {
                result.contact = Some(Contact {
                    phone_number: c.phone_number.clone(),
                    first_name: c.first_name.clone(),
                    last_name: (!c.last_name.is_empty()).then(|| c.last_name.clone()),
                    user_id: (c.user_id != 0).then_some(c.user_id),
                    vcard: (!c.vcard.is_empty()).then(|| c.vcard.clone()),
                })
            }
            tl::enums::MessageMedia::Dice(d) => {
                result.dice = Some(Dice {
                    emoji: d.emoticon.clone(),
                    value: d.value,
                })
            }
            tl::enums::MessageMedia::Geo(g) => result.location = location(&g.geo),
            tl::enums::MessageMedia::GeoLive(g) => {
                result.location = location(&g.geo);
                if let Some(l) = &mut result.location {
                    l.live_period = Some(g.period);
                    l.heading = g.heading;
                    l.proximity_alert_radius = g.proximity_notification_radius;
                }
            }
            tl::enums::MessageMedia::Venue(v) => {
                result.venue = Some(Venue {
                    location: location(&v.geo),
                    title: v.title.clone(),
                    address: v.address.clone(),
                    foursquare_id: (v.provider == "foursquare").then(|| v.venue_id.clone()),
                    foursquare_type: (v.provider == "foursquare").then(|| v.venue_type.clone()),
                    google_place_id: (v.provider == "gplaces").then(|| v.venue_id.clone()),
                    google_place_type: (v.provider == "gplaces").then(|| v.venue_type.clone()),
                })
            }
            tl::enums::MessageMedia::Poll(p) => {
                let tl::enums::Poll::Poll(poll) = &p.poll;
                let tl::enums::PollResults::Results(results) = &p.results;
                let tl::enums::TextWithEntities::Entities(question) = &poll.question;
                result.poll = Some(Poll {
                    id: poll.id.to_string(),
                    question: question.text.clone(),
                    question_entities: question
                        .entities
                        .iter()
                        .filter_map(MessageEntity::from_tl)
                        .collect(),
                    options: poll
                        .answers
                        .iter()
                        .filter_map(|a| {
                            let tl::enums::PollAnswer::Answer(a) = a else {
                                return None;
                            };
                            let tl::enums::TextWithEntities::Entities(t) = &a.text;
                            let count = results.results.as_ref().and_then(|v| {
                                v.iter().find_map(|r| {
                                    let tl::enums::PollAnswerVoters::Voters(r) = r;
                                    (r.option == a.option).then_some(r.voters).flatten()
                                })
                            });
                            Some(PollOption {
                                text: t.text.clone(),
                                text_entities: t
                                    .entities
                                    .iter()
                                    .filter_map(MessageEntity::from_tl)
                                    .collect(),
                                voter_count: count,
                            })
                        })
                        .collect(),
                    total_voter_count: results.total_voters,
                    is_closed: poll.closed,
                    is_anonymous: !poll.public_voters,
                    kind: if poll.quiz { "quiz" } else { "regular" }.into(),
                    allows_multiple_answers: poll.multiple_choice,
                    open_period: poll.close_period,
                    close_date: poll.close_date,
                });
            }
            _ => {}
        }
        result
    }
    pub fn content_type(&self) -> Option<&'static str> {
        if self.animation.is_some() {
            Some("animation")
        } else if self.audio.is_some() {
            Some("audio")
        } else if self.document.is_some() {
            Some("document")
        } else if !self.photo.is_empty() {
            Some("photo")
        } else if self.sticker.is_some() {
            Some("sticker")
        } else if self.video.is_some() {
            Some("video")
        } else if self.video_note.is_some() {
            Some("video_note")
        } else if self.voice.is_some() {
            Some("voice")
        } else if self.contact.is_some() {
            Some("contact")
        } else if self.dice.is_some() {
            Some("dice")
        } else if self.poll.is_some() {
            Some("poll")
        } else if self.venue.is_some() {
            Some("venue")
        } else if self.location.is_some() {
            Some("location")
        } else {
            None
        }
    }
}
