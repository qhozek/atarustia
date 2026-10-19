//! Shared, offline conversion of raw MTProto message fields for bot event consumers.
//! Peer names require a native peer cache; this layer deliberately preserves IDs instead.
use crate::botapi::{File, MessageEntity};
use grammers_client::{media::Media, tl};
use serde_json::{Value, json};

fn peer(p: &tl::enums::Peer) -> i64 {
    grammers_session::types::PeerId::from(p.clone()).bot_api_dialog_id_unchecked()
}
fn entities(values: &[tl::enums::MessageEntity]) -> Vec<MessageEntity> {
    values.iter().filter_map(MessageEntity::from_tl).collect()
}
fn file(media: &tl::enums::MessageMedia) -> Option<File> {
    Media::from_raw(media.clone()).and_then(|m| File::from_media(&m).ok())
}
pub(crate) fn location(point: &tl::enums::GeoPoint) -> Value {
    match point {
        tl::enums::GeoPoint::Point(p) => {
            json!({"latitude":p.lat,"longitude":p.long,"horizontal_accuracy":p.accuracy_radius})
        }
        tl::enums::GeoPoint::Empty => Value::Null,
    }
}

/// Unsupported constructors are identified, never formatted as Debug strings or silently lost.
pub fn media(media: &tl::enums::MessageMedia) -> Value {
    media_at(media, 0)
}
pub(crate) fn media_at(media: &tl::enums::MessageMedia, depth: usize) -> Value {
    use tl::enums::MessageMedia as M;
    if depth >= 4 {
        return json!({"type":"truncated","partial":true,"reason":"media_depth_limit"});
    }
    match media {
        M::Photo(p) => {
            json!({"type":"photo","file":file(media),"has_media_spoiler":p.spoiler,"ttl_seconds":p.ttl_seconds,"live_photo":p.live_photo,"has_video":p.video.is_some()})
        }
        M::Document(d) => {
            let file = file(media);
            let kind = file
                .as_ref()
                .and_then(|f| crate::file_id::FileRef::decode(&f.file_id).ok())
                .map(|f| f.kind);
            json!({"type":kind.unwrap_or(crate::file_id::FileKind::Document),"file":file,"has_media_spoiler":d.spoiler,"ttl_seconds":d.ttl_seconds,"video_timestamp":d.video_timestamp,"has_video_cover":d.video_cover.is_some()})
        }
        M::Contact(c) => {
            json!({"type":"contact","phone_number":c.phone_number,"first_name":c.first_name,"last_name":c.last_name,"user_id":c.user_id,"vcard":c.vcard})
        }
        M::Geo(g) => json!({"type":"location","location":location(&g.geo)}),
        M::GeoLive(g) => {
            json!({"type":"live_location","location":location(&g.geo),"live_period":g.period,"heading":g.heading,"proximity_alert_radius":g.proximity_notification_radius})
        }
        M::Venue(v) => {
            json!({"type":"venue","location":location(&v.geo),"title":v.title,"address":v.address,"provider":v.provider,"venue_id":v.venue_id,"venue_type":v.venue_type})
        }
        M::Poll(p) => {
            json!({"type":"poll","poll":crate::bot_events::poll(&p.poll),"results":crate::bot_events::poll_results(&p.results),"has_attached_media":p.attached_media.is_some()})
        }
        M::Dice(d) => json!({"type":"dice","emoji":d.emoticon,"value":d.value}),
        M::Invoice(i) => {
            json!({"type":"invoice","title":i.title,"description":i.description,"currency":i.currency,"total_amount":i.total_amount,"start_parameter":i.start_param,"is_test":i.test,"shipping_address_requested":i.shipping_address_requested,"receipt_message_id":i.receipt_msg_id,"has_photo":i.photo.is_some(),"has_extended_media":i.extended_media.is_some(),"extended_media":i.extended_media.as_ref().map(|e|extended_media(e,depth+1))})
        }
        M::Game(g) => {
            let tl::enums::Game::Game(g) = &g.game;
            json!({"type":"game","id":g.id.to_string(),"short_name":g.short_name,"title":g.title,"description":g.description,"has_document":g.document.is_some()})
        }
        M::Story(s) => {
            let story = s
                .story
                .as_ref()
                .map(|s| crate::rare_payload::story(s, |m| media_at(m, depth + 1)));
            let partial = story.is_none()
                || story
                    .as_ref()
                    .and_then(|s| s.get("partial"))
                    .and_then(Value::as_bool)
                    == Some(true);
            json!({"type":"story","chat_id":peer(&s.peer),"story_id":s.id,"via_mention":s.via_mention,"story":story,"partial":partial})
        }
        M::WebPage(p) => {
            let page = crate::rare_payload::webpage_at(&p.webpage, depth);
            json!({"type":"web_page","force_large_media":p.force_large_media,"force_small_media":p.force_small_media,"manual":p.manual,"safe":p.safe,"partial":page.get("partial").and_then(Value::as_bool).unwrap_or(false),"page":page})
        }
        M::Giveaway(g) => crate::rare_payload::giveaway(g),
        M::GiveawayResults(g) => crate::rare_payload::giveaway_results(g),
        M::ToDo(t) => crate::rare_payload::checklist(t),
        M::VideoStream(v) => {
            json!({"type":"video_stream","is_rtmp":v.rtmp_stream,"call":crate::service_payload::call(&v.call)})
        }
        M::PaidMedia(p) => {
            let items = p
                .extended_media
                .iter()
                .take(10)
                .map(|e| extended_media(e, depth + 1))
                .collect::<Vec<_>>();
            let partial = p.extended_media.len() > 10
                || items
                    .iter()
                    .any(|item| item.get("partial").and_then(Value::as_bool) == Some(true));
            json!({"type":"paid_media","star_count":p.stars_amount,"item_count":p.extended_media.len(),"items":items,"partial":partial})
        }
        M::Empty => Value::Null,
        other => {
            use tl::Identifiable;
            let constructor = match other {
                M::Unsupported => tl::types::MessageMediaUnsupported::CONSTRUCTOR_ID,
                _ => unreachable!("handled media constructor"),
            };
            json!({"type":"unsupported","constructor_id":constructor,"partial":true})
        }
    }
}
fn extended_media(value: &tl::enums::MessageExtendedMedia, depth: usize) -> Value {
    match value {
        tl::enums::MessageExtendedMedia::Preview(p) => {
            json!({"type":"preview","width":p.w,"height":p.h,"duration":p.video_duration,"has_thumbnail":p.thumb.is_some(),"partial":p.thumb.is_some()})
        }
        tl::enums::MessageExtendedMedia::Media(m) => media_at(&m.media, depth),
    }
}
fn reply(value: &tl::enums::MessageReplyHeader) -> Value {
    match value {
        tl::enums::MessageReplyHeader::Header(r) => {
            let thread_id = if r.forum_topic {
                r.reply_to_top_id.or(r.reply_to_msg_id)
            } else {
                r.reply_to_top_id
            };
            json!({"message_id":r.reply_to_msg_id,"chat_id":r.reply_to_peer_id.as_ref().map(peer),"message_thread_id":thread_id,"is_topic_message":r.forum_topic,"is_scheduled":r.reply_to_scheduled,"quote_text":r.quote_text,"quote_entities":r.quote_entities.as_deref().map(entities),"quote_position":r.quote_offset,"quote_is_manual":r.quote,"has_reply_media":r.reply_media.is_some()})
        }
        tl::enums::MessageReplyHeader::MessageReplyStoryHeader(r) => {
            json!({"story_id":r.story_id,"chat_id":peer(&r.peer)})
        }
    }
}
fn forward(value: &tl::enums::MessageFwdHeader) -> Value {
    let tl::enums::MessageFwdHeader::Header(f) = value;
    json!({"date":f.date,"sender_id":f.from_id.as_ref().map(peer),"sender_user_name":f.from_name,"message_id":f.channel_post,"author_signature":f.post_author,"is_imported":f.imported,"saved_from_chat_id":f.saved_from_peer.as_ref().map(peer),"saved_from_message_id":f.saved_from_msg_id})
}

pub fn fields(raw: &tl::enums::Message) -> Value {
    match raw {
        tl::enums::Message::Message(m) => {
            let is_caption = matches!(
                &m.media,
                Some(
                    tl::enums::MessageMedia::Photo(_)
                        | tl::enums::MessageMedia::Document(_)
                        | tl::enums::MessageMedia::PaidMedia(_)
                )
            );
            let mut value = json!({"message_id":m.id,"chat_id":peer(&m.peer_id),"sender_id":m.from_id.as_ref().map(peer),"outgoing":m.out,"date":m.date,"edit_date":m.edit_date,"media_group_id":m.grouped_id.map(|n|n.to_string()),"has_protected_content":m.noforwards,"author_signature":m.post_author,"via_bot_id":m.via_bot_id,"via_business_bot_id":m.via_business_bot_id,"reply":m.reply_to.as_ref().map(reply),"forward_origin":m.fwd_from.as_ref().map(forward),"media":m.media.as_ref().map(media),"is_pinned":m.pinned,"ttl_period":m.ttl_period});
            let entity_list = m.entities.as_deref().map(entities).unwrap_or_default();
            value["reply_markup"] = m
                .reply_markup
                .as_ref()
                .map(crate::keyboard_payload::from_tl)
                .unwrap_or(Value::Null);
            value["show_caption_above_media"] = json!(m.invert_media);
            value["rich_message"] = m
                .rich_message
                .as_ref()
                .map(crate::rich_payload::from_tl)
                .unwrap_or(Value::Null);
            if is_caption {
                value["caption"] = json!(m.message);
                value["caption_entities"] = json!(entity_list);
            } else {
                value["text"] = json!(m.message);
                value["entities"] = json!(entity_list);
            }
            value
        }
        tl::enums::Message::Service(m) => {
            let reply_id = m.reply_to.as_ref().and_then(|r| {
                if let tl::enums::MessageReplyHeader::Header(r) = r {
                    r.reply_to_msg_id
                } else {
                    None
                }
            });
            json!({"message_id":m.id,"chat_id":peer(&m.peer_id),"sender_id":m.from_id.as_ref().map(peer),"service":true,"date":m.date,"outgoing":m.out,"reply":m.reply_to.as_ref().map(reply),"action":crate::service_payload::action(&m.action,reply_id)})
        }
        _ => Value::Null,
    }
}
