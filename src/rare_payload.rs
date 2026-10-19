//! Structured rare media metadata; nested media conversion is supplied with a shared depth budget.
use grammers_client::tl;
use serde_json::{Value, json};

fn peer(p: &tl::enums::Peer) -> i64 {
    grammers_session::types::PeerId::from(p.clone()).bot_api_dialog_id_unchecked()
}
fn text(value: &tl::enums::TextWithEntities) -> Value {
    let tl::enums::TextWithEntities::Entities(t) = value;
    json!({"text":t.text,"entities":t.entities.iter().filter_map(crate::botapi::MessageEntity::from_tl).collect::<Vec<_>>()})
}
pub(crate) fn photo(value: &tl::enums::Photo) -> Option<crate::botapi::File> {
    crate::botapi::File::from_media(&grammers_client::media::Media::Photo(
        grammers_client::media::Photo::from_raw(value.clone()),
    ))
    .ok()
}
pub(crate) fn document(value: &tl::enums::Document) -> Option<crate::botapi::File> {
    crate::botapi::File::from_media(&grammers_client::media::Media::Document(
        grammers_client::media::Document::from_raw_media(tl::types::MessageMediaDocument {
            nopremium: false,
            spoiler: false,
            video: false,
            round: false,
            voice: false,
            document: Some(value.clone()),
            alt_documents: None,
            video_cover: None,
            video_timestamp: None,
            ttl_seconds: None,
        }),
    ))
    .ok()
}
pub fn webpage(value: &tl::enums::WebPage) -> Value {
    webpage_at(value, 0)
}
pub(crate) fn webpage_at(value: &tl::enums::WebPage, depth: usize) -> Value {
    if depth >= 4 {
        return json!({"type":"truncated","partial":true,"reason":"media_depth_limit"});
    }
    match value {
        tl::enums::WebPage::Empty(p) => json!({"state":"empty","id":p.id.to_string(),"url":p.url}),
        tl::enums::WebPage::Pending(p) => {
            json!({"state":"pending","id":p.id.to_string(),"url":p.url,"date":p.date})
        }
        tl::enums::WebPage::NotModified(p) => {
            json!({"state":"not_modified","cached_page_views":p.cached_page_views})
        }
        tl::enums::WebPage::Page(p) => {
            let cached = p.cached_page.as_ref().map(crate::rich_payload::page);
            let attributes = p.attributes.as_ref().map(|v| {
                v.iter()
                    .take(100)
                    .map(|a|if let tl::enums::WebPageAttribute::Story(a)=a {
                        let story=a.story.as_ref().map(|s|story(s,|m|crate::message_payload::media_at(m,depth+1)));
                        let partial=story.as_ref().is_some_and(crate::rich_payload::has_partial);
                        json!({"type":"story","chat_id":peer(&a.peer),"story_id":a.id,"story":story,"partial":partial})
                    }else{crate::gift_payload::web_attribute(a)})
                    .collect::<Vec<_>>()
            });
            let partial = cached
                .as_ref()
                .is_some_and(crate::rich_payload::has_partial)
                || attributes
                    .as_ref()
                    .is_some_and(|v| v.iter().any(crate::rich_payload::has_partial))
                || p.attributes.as_ref().is_some_and(|v| v.len() > 100);
            json!({"state":"loaded","id":p.id.to_string(),"url":p.url,"display_url":p.display_url,"hash":p.hash,"page_type":p.r#type,"site_name":p.site_name,"title":p.title,"description":p.description,"has_large_media":p.has_large_media,"video_cover_photo":p.video_cover_photo,"photo":p.photo.as_ref().and_then(photo),"document":p.document.as_ref().and_then(document),"embed_url":p.embed_url,"embed_type":p.embed_type,"embed_width":p.embed_width,"embed_height":p.embed_height,"duration":p.duration,"author":p.author,"has_cached_page":p.cached_page.is_some(),"cached_page":cached,"attribute_count":p.attributes.as_ref().map(Vec::len),"attributes":attributes,"partial":partial})
        }
    }
}
pub fn story(
    value: &tl::enums::StoryItem,
    nested: impl FnOnce(&tl::enums::MessageMedia) -> Value,
) -> Value {
    match value {
        tl::enums::StoryItem::Deleted(s) => json!({"state":"deleted","id":s.id}),
        tl::enums::StoryItem::Skipped(s) => {
            json!({"state":"skipped","id":s.id,"date":s.date,"expire_date":s.expire_date,"close_friends":s.close_friends,"is_live":s.live})
        }
        tl::enums::StoryItem::Item(s) => {
            let media = nested(&s.media);
            let mut payload = json!({"state":"loaded","id":s.id,"date":s.date,"expire_date":s.expire_date,"sender_id":s.from_id.as_ref().map(peer),"is_pinned":s.pinned,"is_public":s.public,"close_friends":s.close_friends,"is_min":s.min,"has_protected_content":s.noforwards,"edited":s.edited,"contacts":s.contacts,"selected_contacts":s.selected_contacts,"outgoing":s.out,"caption":s.caption,"entities":s.entities.as_ref().map(|v|v.iter().filter_map(crate::botapi::MessageEntity::from_tl).collect::<Vec<_>>()),"media":media,"forward":s.fwd_from.as_ref().map(|f|{let tl::enums::StoryFwdHeader::Header(f)=f;json!({"modified":f.modified,"sender_id":f.from.as_ref().map(peer),"sender_name":f.from_name,"story_id":f.story_id})}),"views":s.views.as_ref().map(crate::story_payload::views),"sent_reaction":s.sent_reaction.as_ref().map(crate::story_payload::reaction),"albums":s.albums.as_ref().map(|v|v.iter().take(100).copied().collect::<Vec<_>>()),"music":s.music.as_ref().and_then(document),"media_area_count":s.media_areas.as_ref().map(Vec::len),"privacy_rule_count":s.privacy.as_ref().map(Vec::len),"media_areas":s.media_areas.as_ref().map(|v|v.iter().take(100).map(crate::story_payload::area).collect::<Vec<_>>()),"privacy":s.privacy.as_ref().map(|v|v.iter().take(100).map(crate::story_payload::privacy).collect::<Vec<_>>())});
            payload["partial"] = json!(
                s.min
                    || crate::rich_payload::has_partial(&payload)
                    || s.media_areas.as_ref().is_some_and(|v| v.len() > 100)
                    || s.privacy.as_ref().is_some_and(|v| v.len() > 100)
                    || s.albums.as_ref().is_some_and(|v| v.len() > 100)
            );
            payload
        }
    }
}
pub fn giveaway(value: &tl::types::MessageMediaGiveaway) -> Value {
    json!({"type":"giveaway","only_new_subscribers":value.only_new_subscribers,"winners_are_visible":value.winners_are_visible,"chat_ids":value.channels.iter().take(100).filter_map(|id|grammers_session::types::PeerId::channel(*id).map(|p|p.bot_api_dialog_id_unchecked())).collect::<Vec<_>>(),"countries":value.countries_iso2,"prize_description":value.prize_description,"winner_count":value.quantity,"premium_subscription_month_count":value.months,"prize_star_count":value.stars,"winners_selection_date":value.until_date,"partial":value.channels.len()>100})
}
pub fn giveaway_results(value: &tl::types::MessageMediaGiveawayResults) -> Value {
    json!({"type":"giveaway_winners","only_new_subscribers":value.only_new_subscribers,"was_refunded":value.refunded,"chat_id":grammers_session::types::PeerId::channel(value.channel_id).map(|p|p.bot_api_dialog_id_unchecked()),"additional_chat_count":value.additional_peers_count,"giveaway_message_id":value.launch_msg_id,"winner_count":value.winners_count,"unclaimed_prize_count":value.unclaimed_count,"winner_user_ids":value.winners.iter().take(100).copied().collect::<Vec<_>>(),"premium_subscription_month_count":value.months,"prize_star_count":value.stars,"prize_description":value.prize_description,"winners_selection_date":value.until_date,"partial":value.winners.len()>100})
}
pub fn checklist(value: &tl::types::MessageMediaToDo) -> Value {
    let tl::enums::TodoList::List(list) = &value.todo;
    json!({"type":"checklist","title":text(&list.title),"others_can_add_tasks":list.others_can_append,"others_can_mark_tasks_as_done":list.others_can_complete,"tasks":list.list.iter().take(100).map(|i|{let tl::enums::TodoItem::Item(i)=i;json!({"id":i.id,"title":text(&i.title)})}).collect::<Vec<_>>(),"completions":value.completions.as_ref().map(|v|v.iter().take(100).map(|c|{let tl::enums::TodoCompletion::Completion(c)=c;json!({"id":c.id,"completed_by":peer(&c.completed_by),"date":c.date})}).collect::<Vec<_>>()),"partial":list.list.len()>100||value.completions.as_ref().is_some_and(|v|v.len()>100)})
}
