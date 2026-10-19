//! Story privacy/areas/reactions and peer summaries, without fetching private peer data.
use grammers_client::tl;
use serde_json::{Value, json};
fn peer(value: &tl::enums::Peer) -> i64 {
    grammers_session::types::PeerId::from(value.clone()).bot_api_dialog_id_unchecked()
}
pub fn chat(value: &tl::enums::Chat) -> Value {
    match value {
        tl::enums::Chat::Empty(c) => json!({"type":"empty","id":-c.id}),
        tl::enums::Chat::Chat(c) => {
            json!({"type":"group","id":-c.id,"title":c.title,"creator":c.creator,"left":c.left,"deactivated":c.deactivated})
        }
        tl::enums::Chat::Forbidden(c) => {
            json!({"type":"group","id":-c.id,"title":c.title,"forbidden":true})
        }
        tl::enums::Chat::Channel(c) => {
            json!({"type":if c.megagroup{"supergroup"}else{"channel"},"id":grammers_session::types::PeerId::channel(c.id).map(|id|id.bot_api_dialog_id_unchecked()),"title":c.title,"username":c.username,"verified":c.verified,"restricted":c.restricted,"left":c.left})
        }
        tl::enums::Chat::ChannelForbidden(c) => {
            json!({"type":if c.megagroup{"supergroup"}else{"channel"},"id":grammers_session::types::PeerId::channel(c.id).map(|id|id.bot_api_dialog_id_unchecked()),"title":c.title,"forbidden":true})
        }
        tl::enums::Chat::Community(c) => {
            json!({"type":"community","bare_id":c.id.to_string(),"title":c.title,"creator":c.creator,"left":c.left,"is_min":c.min,"collapsed_in_dialogs":c.collapsed_in_dialogs,"partial":c.min||c.admin_rights.is_some()||c.default_banned_rights.is_some()})
        }
        tl::enums::Chat::CommunityForbidden(c) => {
            json!({"type":"community","bare_id":c.id.to_string(),"title":c.title,"forbidden":true})
        }
    }
}
pub fn reaction(value: &tl::enums::Reaction) -> Value {
    match value {
        tl::enums::Reaction::Empty => json!({"type":"empty"}),
        tl::enums::Reaction::Emoji(r) => json!({"type":"emoji","emoji":r.emoticon}),
        tl::enums::Reaction::CustomEmoji(r) => {
            json!({"type":"custom_emoji","custom_emoji_id":r.document_id.to_string()})
        }
        tl::enums::Reaction::Paid => json!({"type":"paid"}),
    }
}
pub fn views(value: &tl::enums::StoryViews) -> Value {
    let tl::enums::StoryViews::Views(v) = value;
    json!({"has_viewers":v.has_viewers,"view_count":v.views_count,"forward_count":v.forwards_count,"reaction_count":v.reactions_count,"recent_viewers":v.recent_viewers.as_ref().map(|v|v.iter().take(100).copied().collect::<Vec<_>>()),"reactions":v.reactions.as_ref().map(|v|v.iter().take(100).map(|r|{let tl::enums::ReactionCount::Count(r)=r;json!({"reaction":reaction(&r.reaction),"count":r.count,"chosen_order":r.chosen_order})}).collect::<Vec<_>>()),"partial":v.recent_viewers.as_ref().is_some_and(|v|v.len()>100)||v.reactions.as_ref().is_some_and(|v|v.len()>100)})
}
pub fn privacy(value: &tl::enums::PrivacyRule) -> Value {
    use tl::enums::PrivacyRule as P;
    match value {
        P::PrivacyValueAllowContacts => json!({"type":"allow_contacts"}),
        P::PrivacyValueAllowAll => json!({"type":"allow_all"}),
        P::PrivacyValueDisallowContacts => json!({"type":"disallow_contacts"}),
        P::PrivacyValueDisallowAll => json!({"type":"disallow_all"}),
        P::PrivacyValueAllowCloseFriends => json!({"type":"allow_close_friends"}),
        P::PrivacyValueAllowPremium => json!({"type":"allow_premium"}),
        P::PrivacyValueAllowBots => json!({"type":"allow_bots"}),
        P::PrivacyValueDisallowBots => json!({"type":"disallow_bots"}),
        P::PrivacyValueAllowUsers(v) => {
            json!({"type":"allow_users","user_ids":v.users.iter().take(100).copied().collect::<Vec<_>>(),"partial":v.users.len()>100})
        }
        P::PrivacyValueDisallowUsers(v) => {
            json!({"type":"disallow_users","user_ids":v.users.iter().take(100).copied().collect::<Vec<_>>(),"partial":v.users.len()>100})
        }
        P::PrivacyValueAllowChatParticipants(v) => {
            json!({"type":"allow_chat_participants","chat_ids":v.chats.iter().take(100).copied().collect::<Vec<_>>(),"partial":v.chats.len()>100})
        }
        P::PrivacyValueDisallowChatParticipants(v) => {
            json!({"type":"disallow_chat_participants","chat_ids":v.chats.iter().take(100).copied().collect::<Vec<_>>(),"partial":v.chats.len()>100})
        }
    }
}
fn coordinates(value: &tl::enums::MediaAreaCoordinates) -> Value {
    let tl::enums::MediaAreaCoordinates::Coordinates(c) = value;
    json!({"x":c.x,"y":c.y,"width":c.w,"height":c.h,"rotation":c.rotation,"radius":c.radius})
}
pub fn area(value: &tl::enums::MediaArea) -> Value {
    use tl::enums::MediaArea as A;
    match value {
        A::Venue(a) => {
            json!({"type":"venue","coordinates":coordinates(&a.coordinates),"location":crate::message_payload::location(&a.geo),"title":a.title,"address":a.address,"provider":a.provider,"venue_id":a.venue_id,"venue_type":a.venue_type})
        }
        A::InputMediaAreaVenue(a) => {
            json!({"type":"input_venue","coordinates":coordinates(&a.coordinates),"query_id":a.query_id.to_string(),"result_id":a.result_id})
        }
        A::GeoPoint(a) => {
            json!({"type":"location","coordinates":coordinates(&a.coordinates),"location":crate::message_payload::location(&a.geo),"address":a.address.as_ref().map(|a|{let tl::enums::GeoPointAddress::Address(a)=a;json!({"country_code":a.country_iso2,"state":a.state,"city":a.city,"street":a.street})})})
        }
        A::SuggestedReaction(a) => {
            json!({"type":"suggested_reaction","coordinates":coordinates(&a.coordinates),"dark":a.dark,"flipped":a.flipped,"reaction":reaction(&a.reaction)})
        }
        A::ChannelPost(a) => {
            json!({"type":"channel_post","coordinates":coordinates(&a.coordinates),"chat_id":peer(&tl::types::PeerChannel{channel_id:a.channel_id}.into()),"message_id":a.msg_id})
        }
        A::InputMediaAreaChannelPost(a) => {
            json!({"type":"input_channel_post","coordinates":coordinates(&a.coordinates),"chat_id":match &a.channel{tl::enums::InputChannel::Channel(c)=>grammers_session::types::PeerId::channel(c.channel_id).map(|id|id.bot_api_dialog_id_unchecked()),_=>None},"message_id":a.msg_id})
        }
        A::Url(a) => json!({"type":"url","coordinates":coordinates(&a.coordinates),"url":a.url}),
        A::Weather(a) => {
            json!({"type":"weather","coordinates":coordinates(&a.coordinates),"emoji":a.emoji,"temperature_c":a.temperature_c,"color":a.color})
        }
        A::StarGift(a) => {
            json!({"type":"star_gift","coordinates":coordinates(&a.coordinates),"slug":a.slug})
        }
    }
}
