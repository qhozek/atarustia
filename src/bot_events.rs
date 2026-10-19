//! Stable event names and serializable payloads for the MTProto bot event stream.
use base64::{Engine as _, engine::general_purpose::STANDARD};
use grammers_client::{tl, update::Update};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BotEvent {
    pub name: String,
    pub chat: Option<i64>,
    pub sender: Option<i64>,
    pub message_id: Option<i32>,
    pub data: Value,
}
fn peer(peer: &tl::enums::Peer) -> i64 {
    grammers_session::types::PeerId::from(peer.clone()).bot_api_dialog_id_unchecked()
}
fn user(peer: &tl::enums::Peer) -> Option<i64> {
    if let tl::enums::Peer::User(u) = peer {
        Some(u.user_id)
    } else {
        None
    }
}
pub(crate) fn reactions(values: &[tl::enums::Reaction]) -> Value {
    Value::Array(
        values
            .iter()
            .map(|r| match r {
                tl::enums::Reaction::Emoji(r) => json!({"type":"emoji","emoji":r.emoticon}),
                tl::enums::Reaction::CustomEmoji(r) => {
                    json!({"type":"custom_emoji","custom_emoji_id":r.document_id.to_string()})
                }
                tl::enums::Reaction::Paid => json!({"type":"paid"}),
                _ => json!({"type":"empty"}),
            })
            .collect(),
    )
}
fn message(raw: &tl::enums::Message) -> (Option<i64>, Option<i64>, Option<i32>, Value) {
    match raw {
        tl::enums::Message::Message(m) => (
            Some(peer(&m.peer_id)),
            m.from_id.as_ref().and_then(user),
            Some(m.id),
            crate::message_payload::fields(raw),
        ),
        tl::enums::Message::Service(m) => (
            Some(peer(&m.peer_id)),
            m.from_id.as_ref().and_then(user),
            Some(m.id),
            crate::message_payload::fields(raw),
        ),
        _ => (None, None, None, Value::Null),
    }
}
pub fn from_update(update: &Update, bot_id: i64) -> Option<BotEvent> {
    match update {
        Update::NewMessage(m) | Update::MessageEdited(m) => {
            let edited = matches!(update, Update::MessageEdited(_));
            let name = match (m.peer(), edited) {
                (Some(grammers_client::peer::Peer::Channel(_)), false) => "channel_post",
                (Some(grammers_client::peer::Peer::Channel(_)), true) => "edited_channel_post",
                (_, true) => "edited_message",
                _ => "bot_message",
            };
            Some(BotEvent {
                name: name.into(),
                chat: Some(m.peer_id().bot_api_dialog_id_unchecked()),
                sender: m
                    .sender_id()
                    .filter(|p| p.kind() == grammers_session::types::PeerKind::User)
                    .map(|p| p.bare_id_unchecked()),
                message_id: Some(m.id()),
                data: serde_json::to_value(crate::botapi::Message::from_native(m)).ok()?,
            })
        }
        _ => raw_event(update.raw(), bot_id),
    }
}
pub fn raw_event(update: &tl::enums::Update, bot_id: i64) -> Option<BotEvent> {
    use tl::enums::Update as U;
    let (name, chat, sender, id, data) = match update {
        U::BotStopped(r) => (
            "bot_stopped",
            Some(r.user_id),
            Some(r.user_id),
            None,
            json!({"user_id":r.user_id,"date":r.date,"stopped":r.stopped}),
        ),
        U::BotInlineSend(r) => (
            "chosen_inline_result",
            None,
            Some(r.user_id),
            None,
            json!({"user_id":r.user_id,"query":r.query,"result_id":r.id,"inline_message_id":r.msg_id.as_ref().map(crate::file_id::encode_inline_id),"location":r.geo.as_ref().map(crate::message_payload::location)}),
        ),
        U::BotInlineQuery(r) => (
            "inline_query",
            None,
            Some(r.user_id),
            None,
            json!({"id":r.query_id.to_string(),"user_id":r.user_id,"query":r.query,"offset":r.offset,"location":r.geo.as_ref().map(crate::message_payload::location),"has_peer_type":r.peer_type.is_some()}),
        ),
        U::BotCallbackQuery(r) => (
            "callback_query",
            Some(peer(&r.peer)),
            Some(r.user_id),
            Some(r.msg_id),
            json!({"id":r.query_id.to_string(),"user_id":r.user_id,"chat_id":peer(&r.peer),"message_id":r.msg_id,"chat_instance":r.chat_instance.to_string(),"data":r.data.as_ref().and_then(|d|std::str::from_utf8(d).ok()),"data_base64":r.data.as_ref().map(|d|STANDARD.encode(d)),"game_short_name":r.game_short_name}),
        ),
        U::InlineBotCallbackQuery(r) => (
            "callback_query",
            None,
            Some(r.user_id),
            None,
            json!({"id":r.query_id.to_string(),"user_id":r.user_id,"inline_message_id":crate::file_id::encode_inline_id(&r.msg_id),"chat_instance":r.chat_instance.to_string(),"data":r.data.as_ref().and_then(|d|std::str::from_utf8(d).ok()),"data_base64":r.data.as_ref().map(|d|STANDARD.encode(d)),"game_short_name":r.game_short_name}),
        ),
        U::Story(r) => (
            "story",
            Some(peer(&r.peer)),
            user(&r.peer),
            None,
            json!({"chat_id":peer(&r.peer),"story":crate::rare_payload::story(&r.story,crate::message_payload::media)}),
        ),
        U::BotMessageReaction(r) => (
            "message_reaction",
            Some(peer(&r.peer)),
            user(&r.actor),
            Some(r.msg_id),
            json!({"chat_id":peer(&r.peer),"message_id":r.msg_id,"actor_id":peer(&r.actor),"date":r.date,"old_reaction":reactions(&r.old_reactions),"new_reaction":reactions(&r.new_reactions)}),
        ),
        U::BotMessageReactions(r) => (
            "message_reaction_count",
            Some(peer(&r.peer)),
            None,
            Some(r.msg_id),
            json!({"chat_id":peer(&r.peer),"message_id":r.msg_id,"date":r.date,"reactions":r.reactions.iter().map(|r|{let tl::enums::ReactionCount::Count(r)=r;json!({"reaction":reactions(std::slice::from_ref(&r.reaction)),"total_count":r.count})}).collect::<Vec<_>>()}),
        ),
        U::BotDeleteBusinessMessage(r) => (
            "deleted_business_messages",
            Some(peer(&r.peer)),
            None,
            None,
            json!({"business_connection_id":r.connection_id,"chat_id":peer(&r.peer),"message_ids":r.messages}),
        ),
        U::BotNewBusinessMessage(r) => {
            let (chat, sender, id, mut data) = message(&r.message);
            if let Some(reply) = &r.reply_to_message
                && data.is_object()
            {
                data["reply_to_message"] = crate::message_payload::fields(reply);
            }
            let mut object = crate::botapi::Message::from_tl(
                &r.message,
                crate::botapi::Chat {
                    id: chat.unwrap_or(0),
                    kind: if chat.is_some_and(|id| id > 0) {
                        "private"
                    } else {
                        "unknown"
                    }
                    .into(),
                    ..Default::default()
                },
                None,
            );
            object.business_connection_id = Some(r.connection_id.clone());
            if let Some(reply) = &r.reply_to_message {
                object.reply_to_message = Some(Box::new(crate::botapi::Message::from_tl(
                    reply,
                    object.chat.clone(),
                    None,
                )));
            }
            data["object"] = json!(object);
            (
                "business_message",
                chat,
                sender,
                id,
                json!({"business_connection_id":r.connection_id,"message":data}),
            )
        }
        U::BotEditBusinessMessage(r) => {
            let (chat, sender, id, mut data) = message(&r.message);
            if let Some(reply) = &r.reply_to_message
                && data.is_object()
            {
                data["reply_to_message"] = crate::message_payload::fields(reply);
            }
            let mut object = crate::botapi::Message::from_tl(
                &r.message,
                crate::botapi::Chat {
                    id: chat.unwrap_or(0),
                    kind: if chat.is_some_and(|id| id > 0) {
                        "private"
                    } else {
                        "unknown"
                    }
                    .into(),
                    ..Default::default()
                },
                None,
            );
            object.business_connection_id = Some(r.connection_id.clone());
            if let Some(reply) = &r.reply_to_message {
                object.reply_to_message = Some(Box::new(crate::botapi::Message::from_tl(
                    reply,
                    object.chat.clone(),
                    None,
                )));
            }
            data["object"] = json!(object);
            (
                "edited_business_message",
                chat,
                sender,
                id,
                json!({"business_connection_id":r.connection_id,"message":data}),
            )
        }
        U::ChannelParticipant(r) => {
            let chat = grammers_session::types::PeerId::channel(r.channel_id)?
                .bot_api_dialog_id_unchecked();
            (
                if r.user_id == bot_id {
                    "my_chat_member"
                } else {
                    "chat_member"
                },
                Some(chat),
                Some(r.actor_id),
                None,
                json!({"chat_id":chat,"user_id":r.user_id,"actor_id":r.actor_id,"date":r.date,"via_chatlist":r.via_chatlist,"old_chat_member":r.prev_participant.as_ref().map(participant),"new_chat_member":r.new_participant.as_ref().map(participant),"invite_link":r.invite.as_ref().map(invite)}),
            )
        }
        U::MessagePollVote(r) => (
            "poll_answer",
            None,
            user(&r.peer),
            None,
            json!({"poll_id":r.poll_id.to_string(),"voter_id":peer(&r.peer),"options":r.options.iter().map(|b|STANDARD.encode(b)).collect::<Vec<_>>(),"positions":r.positions}),
        ),
        U::MessagePoll(r) => (
            "poll",
            r.peer.as_ref().map(peer),
            None,
            r.msg_id,
            json!({"poll_id":r.poll_id.to_string(),"chat_id":r.peer.as_ref().map(peer),"message_id":r.msg_id,"message_thread_id":r.top_msg_id,"poll":r.poll.as_ref().map(poll),"results":poll_results(&r.results)}),
        ),
        U::BotPrecheckoutQuery(r) => (
            "pre_checkout_query",
            None,
            Some(r.user_id),
            None,
            json!({"id":r.query_id.to_string(),"user_id":r.user_id,"currency":r.currency,"total_amount":r.total_amount,"invoice_payload":STANDARD.encode(&r.payload),"shipping_option_id":r.shipping_option_id,"order_info":r.info.as_ref().map(payment_info)}),
        ),
        U::BotShippingQuery(r) => (
            "shipping_query",
            None,
            Some(r.user_id),
            None,
            json!({"id":r.query_id.to_string(),"user_id":r.user_id,"invoice_payload":STANDARD.encode(&r.payload),"shipping_address":post_address(&r.shipping_address)}),
        ),
        U::BotBusinessConnect(r) => {
            let tl::enums::BotBusinessConnection::Connection(c) = &r.connection;
            (
                "business_connection",
                None,
                Some(c.user_id),
                None,
                json!({"id":c.connection_id,"user_id":c.user_id,"dc_id":c.dc_id,"date":c.date,"is_enabled":!c.disabled,"rights":c.rights.as_ref().map(business_rights)}),
            )
        }
        U::BotChatBoost(r) => {
            let tl::enums::Boost::Boost(b) = &r.boost;
            (
                "chat_boost",
                Some(peer(&r.peer)),
                b.user_id,
                None,
                json!({"chat_id":peer(&r.peer),"boost":{"boost_id":b.id,"user_id":b.user_id,"add_date":b.date,"expiration_date":b.expires,"gift":b.gift,"giveaway":b.giveaway,"unclaimed":b.unclaimed,"giveaway_message_id":b.giveaway_msg_id,"used_gift_slug":b.used_gift_slug,"multiplier":b.multiplier,"stars":b.stars}}),
            )
        }
        U::BotChatInviteRequester(r) => (
            "chat_join_request",
            Some(peer(&r.peer)),
            Some(r.user_id),
            None,
            json!({"chat_id":peer(&r.peer),"user_id":r.user_id,"date":r.date,"bio":r.about,"invite_link":invite(&r.invite),"query_id":r.query_id.map(|id|id.to_string())}),
        ),
        U::BotPurchasedPaidMedia(r) => (
            "purchased_paid_media",
            None,
            Some(r.user_id),
            None,
            json!({"user_id":r.user_id,"paid_media_payload":r.payload}),
        ),
        U::ChatParticipant(r) => (
            if r.user_id == bot_id {
                "my_chat_member"
            } else {
                "chat_member"
            },
            Some(-r.chat_id),
            Some(r.actor_id),
            None,
            json!({"chat_id":-r.chat_id,"user_id":r.user_id,"actor_id":r.actor_id,"date":r.date,"old_chat_member":r.prev_participant.as_ref().map(chat_participant),"new_chat_member":r.new_participant.as_ref().map(chat_participant),"invite_link":r.invite.as_ref().map(invite)}),
        ),
        _ => return None,
    };
    Some(BotEvent {
        name: name.into(),
        chat,
        sender,
        message_id: id,
        data,
    })
}
fn post_address(address: &tl::enums::PostAddress) -> Value {
    let tl::enums::PostAddress::Address(a) = address;
    json!({"street_line1":a.street_line1,"street_line2":a.street_line2,"city":a.city,"state":a.state,"country_code":a.country_iso2,"post_code":a.post_code})
}
fn text_entities(text: &tl::enums::TextWithEntities) -> Value {
    let tl::enums::TextWithEntities::Entities(t) = text;
    json!({"text":t.text,"entities":t.entities.iter().filter_map(crate::botapi::MessageEntity::from_tl).collect::<Vec<_>>()})
}
pub(crate) fn poll(value: &tl::enums::Poll) -> Value {
    let tl::enums::Poll::Poll(p) = value;
    json!({"id":p.id.to_string(),"is_closed":p.closed,"is_anonymous":!p.public_voters,"allows_multiple_answers":p.multiple_choice,"type":if p.quiz{"quiz"}else{"regular"},"open_answers":p.open_answers,"revoting_disabled":p.revoting_disabled,"shuffle_answers":p.shuffle_answers,"hide_results_until_close":p.hide_results_until_close,"creator":p.creator,"subscribers_only":p.subscribers_only,"question":text_entities(&p.question),"options":p.answers.iter().map(poll_answer).collect::<Vec<_>>(),"open_period":p.close_period,"close_date":p.close_date,"countries":p.countries_iso2})
}
fn poll_answer(answer: &tl::enums::PollAnswer) -> Value {
    match answer {
        tl::enums::PollAnswer::Answer(a) => {
            json!({"text":text_entities(&a.text),"option":STANDARD.encode(&a.option),"added_by":a.added_by.as_ref().map(peer),"date":a.date,"has_media":a.media.is_some()})
        }
        tl::enums::PollAnswer::InputPollAnswer(a) => {
            json!({"text":text_entities(&a.text),"has_media":a.media.is_some(),"input_only":true})
        }
    }
}
pub(crate) fn poll_results(value: &tl::enums::PollResults) -> Value {
    let tl::enums::PollResults::Results(r) = value;
    json!({"is_partial":r.min,"has_unread_votes":r.has_unread_votes,"can_view_stats":r.can_view_stats,"total_voter_count":r.total_voters,"recent_voters":r.recent_voters.as_ref().map(|v|v.iter().map(peer).collect::<Vec<_>>()),"explanation":r.solution,"explanation_entities":r.solution_entities.as_ref().map(|v|v.iter().filter_map(crate::botapi::MessageEntity::from_tl).collect::<Vec<_>>()),"has_explanation_media":r.solution_media.is_some(),"options":r.results.as_ref().map(|v|v.iter().map(|a|{let tl::enums::PollAnswerVoters::Voters(a)=a;json!({"option":STANDARD.encode(&a.option),"voter_count":a.voters,"chosen":a.chosen,"correct":a.correct,"recent_voters":a.recent_voters.as_ref().map(|v|v.iter().map(peer).collect::<Vec<_>>())})}).collect::<Vec<_>>())})
}
pub(crate) fn payment_info(value: &tl::enums::PaymentRequestedInfo) -> Value {
    let tl::enums::PaymentRequestedInfo::Info(i) = value;
    json!({"name":i.name,"phone_number":i.phone,"email":i.email,"shipping_address":i.shipping_address.as_ref().map(post_address)})
}
pub(crate) fn admin_rights(value: &tl::enums::ChatAdminRights) -> Value {
    let tl::enums::ChatAdminRights::Rights(r) = value;
    json!({"can_change_info":r.change_info,"can_post_messages":r.post_messages,"can_edit_messages":r.edit_messages,"can_delete_messages":r.delete_messages,"can_restrict_members":r.ban_users,"can_invite_users":r.invite_users,"can_pin_messages":r.pin_messages,"can_promote_members":r.add_admins,"is_anonymous":r.anonymous,"can_manage_video_chats":r.manage_call,"can_manage_chat":r.other,"can_manage_topics":r.manage_topics,"can_post_stories":r.post_stories,"can_edit_stories":r.edit_stories,"can_delete_stories":r.delete_stories,"can_manage_direct_messages":r.manage_direct_messages,"can_manage_ranks":r.manage_ranks,"can_manage_linked_peers":r.manage_linked_peers,"can_manage_welcome_messages":r.manage_welcome_messages})
}
fn restrictions(value: &tl::enums::ChatBannedRights) -> Value {
    let tl::enums::ChatBannedRights::Rights(r) = value;
    json!({"until_date":r.until_date,"can_read_messages":!r.view_messages,"can_send_messages":!r.send_messages,"can_send_media_messages":!r.send_media,"can_send_stickers":!r.send_stickers,"can_send_animations":!r.send_gifs,"can_send_games":!r.send_games,"can_use_inline_bots":!r.send_inline,"can_add_web_page_previews":!r.embed_links,"can_send_polls":!r.send_polls,"can_change_info":!r.change_info,"can_invite_users":!r.invite_users,"can_pin_messages":!r.pin_messages,"can_manage_topics":!r.manage_topics,"can_send_photos":!r.send_photos,"can_send_videos":!r.send_videos,"can_send_video_notes":!r.send_roundvideos,"can_send_audios":!r.send_audios,"can_send_voice_notes":!r.send_voices,"can_send_documents":!r.send_docs,"can_send_plain":!r.send_plain,"can_edit_rank":!r.edit_rank,"can_send_reactions":!r.send_reactions,"can_manage_linked_peers":!r.manage_linked_peers})
}
pub(crate) fn participant(p: &tl::enums::ChannelParticipant) -> Value {
    match p {
        tl::enums::ChannelParticipant::Participant(p) => {
            json!({"status":"member","user_id":p.user_id,"date":p.date,"subscription_until_date":p.subscription_until_date,"rank":p.rank})
        }
        tl::enums::ChannelParticipant::ParticipantSelf(p) => {
            json!({"status":"member","user_id":p.user_id,"date":p.date,"inviter_id":p.inviter_id,"via_join_request":p.via_request,"subscription_until_date":p.subscription_until_date,"rank":p.rank})
        }
        tl::enums::ChannelParticipant::Creator(p) => {
            json!({"status":"creator","user_id":p.user_id,"rights":admin_rights(&p.admin_rights),"rank":p.rank})
        }
        tl::enums::ChannelParticipant::Admin(p) => {
            json!({"status":"administrator","user_id":p.user_id,"can_be_edited":p.can_edit,"is_self":p.is_self,"inviter_id":p.inviter_id,"promoted_by":p.promoted_by,"date":p.date,"rights":admin_rights(&p.admin_rights),"rank":p.rank})
        }
        tl::enums::ChannelParticipant::Banned(p) => {
            let tl::enums::ChatBannedRights::Rights(r) = &p.banned_rights;
            json!({"status":if r.view_messages{"kicked"}else{"restricted"},"is_member":!p.left,"peer_id":peer(&p.peer),"kicked_by":p.kicked_by,"date":p.date,"restrictions":restrictions(&p.banned_rights),"rank":p.rank})
        }
        tl::enums::ChannelParticipant::Left(p) => json!({"status":"left","peer_id":peer(&p.peer)}),
    }
}
pub(crate) fn chat_participant(p: &tl::enums::ChatParticipant) -> Value {
    match p {
        tl::enums::ChatParticipant::Participant(p) => {
            json!({"status":"member","user_id":p.user_id,"inviter_id":p.inviter_id,"date":p.date,"rank":p.rank})
        }
        tl::enums::ChatParticipant::Admin(p) => {
            json!({"status":"administrator","user_id":p.user_id,"inviter_id":p.inviter_id,"date":p.date,"rank":p.rank})
        }
        tl::enums::ChatParticipant::Creator(p) => {
            json!({"status":"creator","user_id":p.user_id,"rank":p.rank})
        }
    }
}
fn business_rights(rights: &tl::enums::BusinessBotRights) -> Value {
    let tl::enums::BusinessBotRights::Rights(r) = rights;
    json!({"can_reply":r.reply,"can_read_messages":r.read_messages,"can_delete_sent_messages":r.delete_sent_messages,"can_delete_all_messages":r.delete_received_messages,"can_edit_name":r.edit_name,"can_edit_bio":r.edit_bio,"can_edit_profile_photo":r.edit_profile_photo,"can_edit_username":r.edit_username,"can_view_gifts":r.view_gifts,"can_sell_gifts":r.sell_gifts,"can_change_gift_settings":r.change_gift_settings,"can_transfer_and_upgrade_gifts":r.transfer_and_upgrade_gifts,"can_transfer_stars":r.transfer_stars,"can_manage_stories":r.manage_stories})
}
pub(crate) fn invite(invite: &tl::enums::ExportedChatInvite) -> Value {
    match invite {
        tl::enums::ExportedChatInvite::ChatInviteExported(i) => {
            json!({"invite_link":i.link,"creator_id":i.admin_id,"creates_join_request":i.request_needed,"is_primary":i.permanent,"is_revoked":i.revoked,"date":i.date,"expire_date":i.expire_date,"member_limit":i.usage_limit,"pending_join_request_count":i.requested,"name":i.title})
        }
        _ => Value::Null,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn business_connection_boost_and_paid_media_are_structured() {
        let business = tl::types::UpdateBotBusinessConnect {
            connection: tl::types::BotBusinessConnection {
                disabled: true,
                connection_id: "connection-1".into(),
                user_id: 9,
                dc_id: 2,
                date: 10,
                rights: None,
            }
            .into(),
            qts: 1,
        }
        .into();
        let event = raw_event(&business, 1).unwrap();
        assert_eq!(event.sender, Some(9));
        assert_eq!(event.data["is_enabled"], false);
        assert!(event.data.get("raw").is_none());
        let boost = tl::types::UpdateBotChatBoost {
            peer: tl::types::PeerChannel { channel_id: 5 }.into(),
            boost: tl::types::Boost {
                gift: false,
                giveaway: true,
                unclaimed: false,
                id: "boost-1".into(),
                user_id: Some(9),
                giveaway_msg_id: Some(7),
                date: 10,
                expires: 20,
                used_gift_slug: None,
                multiplier: Some(2),
                stars: None,
            }
            .into(),
            qts: 2,
        }
        .into();
        let event = raw_event(&boost, 1).unwrap();
        assert_eq!(event.chat, Some(-1000000000005));
        assert_eq!(event.data["boost"]["boost_id"], "boost-1");
        let paid = tl::types::UpdateBotPurchasedPaidMedia {
            user_id: 9,
            payload: "invoice".into(),
            qts: 3,
        }
        .into();
        assert_eq!(
            raw_event(&paid, 1).unwrap().data["paid_media_payload"],
            "invoice"
        );
    }
    #[test]
    fn shipping_address_fields_are_not_debug_strings() {
        let shipping = tl::types::UpdateBotShippingQuery {
            query_id: 5,
            user_id: 9,
            payload: vec![1],
            shipping_address: tl::types::PostAddress {
                street_line1: "line1".into(),
                street_line2: String::new(),
                city: "city".into(),
                state: String::new(),
                country_iso2: "RU".into(),
                post_code: "123".into(),
            }
            .into(),
        }
        .into();
        let event = raw_event(&shipping, 1).unwrap();
        assert_eq!(event.data["shipping_address"]["city"], "city");
        assert_eq!(event.data["shipping_address"]["country_code"], "RU");
    }
    #[test]
    fn member_status_and_poll_text_are_structured() {
        let participant: tl::enums::ChannelParticipant = tl::types::ChannelParticipant {
            user_id: 9,
            date: 1,
            subscription_until_date: None,
            rank: Some("member".into()),
        }
        .into();
        assert_eq!(super::participant(&participant)["status"], "member");
        let update = tl::types::UpdateMessagePoll {
            peer: None,
            msg_id: None,
            top_msg_id: None,
            poll_id: 42,
            poll: Some(
                tl::types::Poll {
                    id: 42,
                    closed: false,
                    public_voters: false,
                    multiple_choice: false,
                    quiz: false,
                    open_answers: false,
                    revoting_disabled: false,
                    shuffle_answers: false,
                    hide_results_until_close: false,
                    creator: false,
                    subscribers_only: false,
                    question: tl::types::TextWithEntities {
                        text: "Question".into(),
                        entities: vec![],
                    }
                    .into(),
                    answers: vec![
                        tl::types::PollAnswer {
                            text: tl::types::TextWithEntities {
                                text: "Option".into(),
                                entities: vec![],
                            }
                            .into(),
                            option: vec![1],
                            media: None,
                            added_by: None,
                            date: None,
                        }
                        .into(),
                    ],
                    close_period: None,
                    close_date: None,
                    countries_iso2: None,
                    hash: 0,
                }
                .into(),
            ),
            results: tl::types::PollResults {
                min: false,
                has_unread_votes: false,
                can_view_stats: false,
                results: None,
                total_voters: Some(5),
                recent_voters: None,
                solution: None,
                solution_entities: None,
                solution_media: None,
            }
            .into(),
        }
        .into();
        let event = raw_event(&update, 1).unwrap();
        assert_eq!(event.data["poll"]["question"]["text"], "Question");
        assert_eq!(event.data["results"]["total_voter_count"], 5);
        assert!(event.data["poll"].is_object());
    }
    #[test]
    fn business_deletion_has_ids_not_unbounded_debug() {
        let raw = tl::types::UpdateBotDeleteBusinessMessage {
            connection_id: "business-1".into(),
            peer: tl::types::PeerUser { user_id: 5 }.into(),
            messages: vec![1, 2],
            qts: 3,
        }
        .into();
        let event = raw_event(&raw, 7).unwrap();
        assert_eq!(event.name, "deleted_business_messages");
        assert_eq!(event.data["business_connection_id"], "business-1");
        assert_eq!(event.data["message_ids"], serde_json::json!([1, 2]));
    }
    #[test]
    fn reaction_payload_and_owner_identity() {
        let update = tl::types::UpdateBotMessageReaction {
            peer: tl::types::PeerChannel { channel_id: 5 }.into(),
            msg_id: 7,
            date: 9,
            actor: tl::types::PeerUser { user_id: 123 }.into(),
            old_reactions: vec![],
            new_reactions: vec![
                tl::types::ReactionEmoji {
                    emoticon: "🦀".into(),
                }
                .into(),
            ],
            qts: 1,
        }
        .into();
        let event = raw_event(&update, 99).unwrap();
        assert_eq!(event.name, "message_reaction");
        assert_eq!(event.chat, Some(-1000000000005));
        assert_eq!(event.sender, Some(123));
        assert_eq!(event.data["new_reaction"][0]["emoji"], "🦀");
    }
    #[test]
    fn poll_is_not_a_chat_target() {
        let update = tl::types::UpdateMessagePollVote {
            poll_id: 123,
            peer: tl::types::PeerUser { user_id: 7 }.into(),
            options: vec![vec![0, 255]],
            positions: vec![1],
            qts: 2,
        }
        .into();
        let event = raw_event(&update, 1).unwrap();
        assert!(event.chat.is_none());
        assert_eq!(event.name, "poll_answer");
        assert_eq!(event.data["options"][0], "AP8=");
    }
}
