//! Structured service actions. IDs are preserved without fetching peer/message objects.
use base64::{Engine as _, engine::general_purpose::STANDARD};
use grammers_client::tl;
use serde_json::{Value, json};

fn peer(p: &tl::enums::Peer) -> i64 {
    grammers_session::types::PeerId::from(p.clone()).bot_api_dialog_id_unchecked()
}
fn charge(value: &tl::enums::PaymentCharge) -> Value {
    let tl::enums::PaymentCharge::Charge(c) = value;
    json!({"telegram_payment_charge_id":c.id,"provider_payment_charge_id":c.provider_charge_id})
}
pub(crate) fn call(value: &tl::enums::InputGroupCall) -> Value {
    match value {
        tl::enums::InputGroupCall::Call(c) => json!({"id":c.id.to_string()}),
        tl::enums::InputGroupCall::Slug(c) => json!({"slug":c.slug}),
        tl::enums::InputGroupCall::InviteMessage(c) => json!({"invite_message_id":c.msg_id}),
    }
}
pub fn action(value: &tl::enums::MessageAction, reply_id: Option<i32>) -> Value {
    use tl::enums::MessageAction as A;
    match value {
        A::Empty => json!({"type":"empty"}),
        A::SetChatTheme(a) => json!({"type":"chat_theme_changed","theme":match &a.theme {
            tl::enums::ChatTheme::Theme(t)=>json!({"type":"emoji","emoticon":t.emoticon}),
            tl::enums::ChatTheme::UniqueGift(t)=>json!({"type":"unique_gift","gift":crate::gift_payload::gift(&t.gift),"settings":t.theme_settings.iter().take(100).map(crate::gift_payload::theme).collect::<Vec<_>>(),"partial":t.theme_settings.len()>100}),
        }}),
        A::SetChatWallPaper(a) => {
            json!({"type":"chat_wallpaper_changed","same":a.same,"for_both":a.for_both,"wallpaper":crate::gift_payload::wallpaper(&a.wallpaper)})
        }
        A::NewCreatorPending(a) => json!({"type":"new_creator_pending","user_id":a.new_creator_id}),
        A::ChangeCreator(a) => json!({"type":"creator_changed","user_id":a.new_creator_id}),
        A::NoForwardsToggle(a) => {
            json!({"type":"protected_content_changed","previous":a.prev_value,"current":a.new_value})
        }
        A::NoForwardsRequest(a) => {
            json!({"type":"protected_content_requested","expired":a.expired,"previous":a.prev_value,"current":a.new_value})
        }
        A::ManagedBotCreated(a) => json!({"type":"managed_bot_created","bot_id":a.bot_id}),
        A::ChangeCommunity(a) => {
            json!({"type":"community_changed","community_id":a.community_id.map(|n|n.to_string())})
        }
        A::ChatJoinedViaCommunity(a) => {
            json!({"type":"joined_via_community","community_id":a.community_id.to_string()})
        }
        A::SuggestBirthday(a) => {
            let tl::enums::Birthday::Birthday(b) = &a.birthday;
            json!({"type":"birthday_suggested","day":b.day,"month":b.month,"year":b.year})
        }
        A::ConferenceCall(a) => {
            json!({"type":"conference_call","missed":a.missed,"active":a.active,"video":a.video,"call_id":a.call_id.to_string(),"duration":a.duration,"other_participants":a.other_participants.as_ref().map(|v|v.iter().take(100).map(peer).collect::<Vec<_>>()),"partial":a.other_participants.as_ref().is_some_and(|v|v.len()>100)})
        }
        A::GiftTon(a) => {
            json!({"type":"gift_ton","currency":a.currency,"amount":a.amount.to_string(),"crypto_currency":a.crypto_currency,"crypto_amount":a.crypto_amount.to_string(),"transaction_id":a.transaction_id})
        }
        A::StarGiftPurchaseOffer(a) => {
            json!({"type":"star_gift_purchase_offer","accepted":a.accepted,"declined":a.declined,"gift":crate::gift_payload::gift(&a.gift),"price":crate::gift_payload::amount(&a.price),"expires_at":a.expires_at})
        }
        A::StarGiftPurchaseOfferDeclined(a) => {
            json!({"type":"star_gift_purchase_offer_declined","expired":a.expired,"gift":crate::gift_payload::gift(&a.gift),"price":crate::gift_payload::amount(&a.price)})
        }
        A::SecureValuesSent(a) => {
            json!({"type":"passport_data_sent","value_types":a.types.iter().take(100).map(|t|match t {
            tl::enums::SecureValueType::PersonalDetails=>"personal_details",tl::enums::SecureValueType::Passport=>"passport",tl::enums::SecureValueType::DriverLicense=>"driver_license",tl::enums::SecureValueType::IdentityCard=>"identity_card",tl::enums::SecureValueType::InternalPassport=>"internal_passport",tl::enums::SecureValueType::Address=>"address",tl::enums::SecureValueType::UtilityBill=>"utility_bill",tl::enums::SecureValueType::BankStatement=>"bank_statement",tl::enums::SecureValueType::RentalAgreement=>"rental_agreement",tl::enums::SecureValueType::PassportRegistration=>"passport_registration",tl::enums::SecureValueType::TemporaryRegistration=>"temporary_registration",tl::enums::SecureValueType::Phone=>"phone",tl::enums::SecureValueType::Email=>"email",
        }).collect::<Vec<_>>(),"partial":a.types.len()>100})
        }
        A::SecureValuesSentMe(a) => {
            json!({"type":"passport_data_received","value_count":a.values.len(),"redacted":true,"omitted_fields":["values","credentials"],"partial":true})
        }
        A::RequestedPeer(a) => {
            json!({"type":"peers_shared","button_id":a.button_id,"peer_ids":a.peers.iter().take(100).map(peer).collect::<Vec<_>>(),"partial":a.peers.len()>100})
        }
        A::RequestedPeerSentMe(a) => {
            json!({"type":"peers_shared","button_id":a.button_id,"peers":a.peers.iter().take(100).map(|p|match p {
            tl::enums::RequestedPeer::User(p)=>json!({"type":"user","id":p.user_id,"first_name":p.first_name,"last_name":p.last_name,"username":p.username,"photo":p.photo.as_ref().and_then(crate::rare_payload::photo)}),
            tl::enums::RequestedPeer::Chat(p)=>json!({"type":"group","id":-p.chat_id,"title":p.title,"photo":p.photo.as_ref().and_then(crate::rare_payload::photo)}),
            tl::enums::RequestedPeer::Channel(p)=>json!({"type":"channel","id":grammers_session::types::PeerId::channel(p.channel_id).map(|id|id.bot_api_dialog_id_unchecked()),"title":p.title,"username":p.username,"photo":p.photo.as_ref().and_then(crate::rare_payload::photo)}),
        }).collect::<Vec<_>>(),"partial":a.peers.len()>100})
        }
        A::SuggestedPostApproval(a) => {
            json!({"type":"suggested_post_approval","rejected":a.rejected,"balance_too_low":a.balance_too_low,"reject_comment":a.reject_comment,"schedule_date":a.schedule_date,"price":a.price.as_ref().map(crate::gift_payload::amount)})
        }
        A::SuggestedPostSuccess(a) => {
            json!({"type":"suggested_post_success","price":crate::gift_payload::amount(&a.price)})
        }
        A::SuggestedPostRefund(a) => {
            json!({"type":"suggested_post_refund","payer_initiated":a.payer_initiated})
        }
        A::GiftPremium(a) => {
            json!({"type":"gift_premium","currency":a.currency,"amount":a.amount.to_string(),"days":a.days,"crypto_currency":a.crypto_currency,"crypto_amount":a.crypto_amount.map(|n|n.to_string()),"message":a.message.as_ref().map(crate::gift_payload::text)})
        }
        A::GiftCode(a) => {
            json!({"type":"gift_code","via_giveaway":a.via_giveaway,"unclaimed":a.unclaimed,"boost_peer":a.boost_peer.as_ref().map(peer),"days":a.days,"slug":a.slug,"currency":a.currency,"amount":a.amount.map(|n|n.to_string()),"crypto_currency":a.crypto_currency,"crypto_amount":a.crypto_amount.map(|n|n.to_string()),"message":a.message.as_ref().map(crate::gift_payload::text)})
        }
        A::GiftStars(a) => {
            json!({"type":"gift_stars","currency":a.currency,"amount":a.amount.to_string(),"stars":a.stars.to_string(),"crypto_currency":a.crypto_currency,"crypto_amount":a.crypto_amount.map(|n|n.to_string()),"transaction_id":a.transaction_id})
        }
        A::PrizeStars(a) => {
            json!({"type":"prize_stars","unclaimed":a.unclaimed,"stars":a.stars.to_string(),"transaction_id":a.transaction_id,"boost_peer":peer(&a.boost_peer),"giveaway_message_id":a.giveaway_msg_id})
        }
        A::StarGift(a) => {
            json!({"type":"star_gift","name_hidden":a.name_hidden,"saved":a.saved,"converted":a.converted,"upgraded":a.upgraded,"refunded":a.refunded,"can_upgrade":a.can_upgrade,"prepaid_upgrade":a.prepaid_upgrade,"upgrade_separate":a.upgrade_separate,"auction_acquired":a.auction_acquired,"gift":crate::gift_payload::gift(&a.gift),"message":a.message.as_ref().map(crate::gift_payload::text),"convert_stars":a.convert_stars.map(|n|n.to_string()),"upgrade_message_id":a.upgrade_msg_id,"upgrade_stars":a.upgrade_stars.map(|n|n.to_string()),"from_id":a.from_id.as_ref().map(peer),"peer_id":a.peer.as_ref().map(peer),"saved_id":a.saved_id.map(|n|n.to_string()),"prepaid_upgrade_hash":a.prepaid_upgrade_hash,"gift_message_id":a.gift_msg_id,"to_id":a.to_id.as_ref().map(peer),"gift_number":a.gift_num})
        }
        A::StarGiftUnique(a) => {
            json!({"type":"unique_star_gift","upgrade":a.upgrade,"transferred":a.transferred,"saved":a.saved,"refunded":a.refunded,"prepaid_upgrade":a.prepaid_upgrade,"assigned":a.assigned,"from_offer":a.from_offer,"craft":a.craft,"name_hidden":a.name_hidden,"gift":crate::gift_payload::gift(&a.gift),"can_export_at":a.can_export_at,"transfer_stars":a.transfer_stars.map(|n|n.to_string()),"from_id":a.from_id.as_ref().map(peer),"peer_id":a.peer.as_ref().map(peer),"saved_id":a.saved_id.map(|n|n.to_string()),"resale_amount":a.resale_amount.as_ref().map(crate::gift_payload::amount),"can_transfer_at":a.can_transfer_at,"can_resell_at":a.can_resell_at,"drop_original_details_stars":a.drop_original_details_stars.map(|n|n.to_string()),"can_craft_at":a.can_craft_at,"message":a.message.as_ref().map(crate::gift_payload::text)})
        }
        A::TodoCompletions(a) => {
            json!({"type":"checklist_tasks_done","completed_task_ids":a.completed,"incomplete_task_ids":a.incompleted})
        }
        A::TodoAppendTasks(a) => {
            json!({"type":"checklist_tasks_added","tasks":a.list.iter().take(100).map(|t|{let tl::enums::TodoItem::Item(t)=t;let tl::enums::TextWithEntities::Entities(title)=&t.title;json!({"id":t.id,"title":title.text,"entities":title.entities.iter().filter_map(crate::botapi::MessageEntity::from_tl).collect::<Vec<_>>()})}).collect::<Vec<_>>(),"partial":a.list.len()>100})
        }
        A::PaidMessagesPrice(a) => {
            json!({"type":"paid_message_price_changed","star_count":a.stars,"broadcast_messages_allowed":a.broadcast_messages_allowed})
        }
        A::PaidMessagesRefunded(a) => {
            json!({"type":"paid_messages_refunded","message_count":a.count,"star_count":a.stars})
        }
        A::ChatCreate(a) => json!({"type":"chat_created","title":a.title,"user_ids":a.users}),
        A::ChannelCreate(a) => json!({"type":"channel_created","title":a.title}),
        A::ChatEditTitle(a) => json!({"type":"chat_title_changed","title":a.title}),
        A::ChatEditPhoto(a) => {
            let native = grammers_client::media::Media::Photo(
                grammers_client::media::Photo::from_raw(a.photo.clone()),
            );
            json!({"type":"chat_photo_changed","photo":crate::botapi::File::from_media(&native).ok()})
        }
        A::ChatDeletePhoto => json!({"type":"chat_photo_deleted"}),
        A::ChatAddUser(a) => json!({"type":"members_added","user_ids":a.users}),
        A::ChatDeleteUser(a) => json!({"type":"member_removed","user_id":a.user_id}),
        A::ChatJoinedByLink(a) => json!({"type":"joined_by_link","inviter_id":a.inviter_id}),
        A::ChatJoinedByRequest => json!({"type":"joined_by_request"}),
        A::ChatMigrateTo(a) => {
            json!({"type":"chat_migrated_to","chat_id":peer(&tl::types::PeerChannel{channel_id:a.channel_id}.into())})
        }
        A::ChannelMigrateFrom(a) => {
            json!({"type":"chat_migrated_from","title":a.title,"chat_id":peer(&tl::types::PeerChat{chat_id:a.chat_id}.into())})
        }
        A::PinMessage => json!({"type":"message_pinned","message_id":reply_id}),
        A::HistoryClear => json!({"type":"history_cleared"}),
        A::TopicCreate(a) => {
            json!({"type":"forum_topic_created","name":a.title,"title_missing":a.title_missing,"icon_color":a.icon_color,"icon_custom_emoji_id":a.icon_emoji_id.map(|n|n.to_string())})
        }
        A::TopicEdit(a) => {
            json!({"type":"forum_topic_edited","name":a.title,"icon_custom_emoji_id":a.icon_emoji_id.map(|n|n.to_string()),"is_closed":a.closed,"is_hidden":a.hidden})
        }
        A::SetMessagesTtl(a) => {
            json!({"type":"message_auto_delete_timer_changed","message_auto_delete_time":a.period,"auto_setting_from":a.auto_setting_from})
        }
        A::WebViewDataSentMe(a) => {
            json!({"type":"web_app_data","button_text":a.text,"data":a.data})
        }
        A::WebViewDataSent(a) => json!({"type":"web_app_data_sent","button_text":a.text}),
        A::PaymentSentMe(a) => {
            json!({"type":"successful_payment","currency":a.currency,"total_amount":a.total_amount,"invoice_payload":STANDARD.encode(&a.payload),"invoice_payload_encoding":"base64","order_info":a.info.as_ref().map(crate::bot_events::payment_info),"shipping_option_id":a.shipping_option_id,"charge":charge(&a.charge),"is_first_recurring":a.recurring_init,"is_recurring":a.recurring_used||a.recurring_init,"subscription_expiration_date":a.subscription_until_date})
        }
        A::PaymentSent(a) => {
            json!({"type":"payment_sent","currency":a.currency,"total_amount":a.total_amount,"invoice_slug":a.invoice_slug,"is_first_recurring":a.recurring_init,"is_recurring":a.recurring_used||a.recurring_init,"subscription_expiration_date":a.subscription_until_date})
        }
        A::PaymentRefunded(a) => {
            json!({"type":"refunded_payment","peer_id":peer(&a.peer),"currency":a.currency,"total_amount":a.total_amount,"invoice_payload":a.payload.as_ref().map(|p|STANDARD.encode(p)),"invoice_payload_encoding":"base64","charge":charge(&a.charge)})
        }
        A::GameScore(a) => {
            json!({"type":"game_score","game_id":a.game_id.to_string(),"score":a.score})
        }
        A::GeoProximityReached(a) => {
            json!({"type":"proximity_alert_triggered","traveler_id":peer(&a.from_id),"watcher_id":peer(&a.to_id),"distance":a.distance})
        }
        A::GroupCall(a) => {
            json!({"type":if a.duration.is_some(){"video_chat_ended"}else{"video_chat_started"},"call":call(&a.call),"duration":a.duration})
        }
        A::GroupCallScheduled(a) => {
            json!({"type":"video_chat_scheduled","call":call(&a.call),"start_date":a.schedule_date})
        }
        A::InviteToGroupCall(a) => {
            json!({"type":"video_chat_participants_invited","call":call(&a.call),"user_ids":a.users})
        }
        A::PhoneCall(a) => {
            json!({"type":"phone_call","call_id":a.call_id.to_string(),"is_video":a.video,"duration":a.duration,"reason":a.reason.as_ref().map(|r|match r {tl::enums::PhoneCallDiscardReason::Missed=>"missed",tl::enums::PhoneCallDiscardReason::Disconnect=>"disconnect",tl::enums::PhoneCallDiscardReason::Hangup=>"hangup",tl::enums::PhoneCallDiscardReason::Busy=>"busy",tl::enums::PhoneCallDiscardReason::MigrateConferenceCall(_)=>"migrate_conference_call"})})
        }
        A::BotAllowed(a) => {
            json!({"type":"write_access_allowed","domain":a.domain,"from_attachment_menu":a.attach_menu,"from_request":a.from_request,"has_web_app":a.app.is_some(),"app":a.app.as_ref().map(|app|match app {tl::enums::BotApp::NotModified=>json!({"state":"not_modified"}),tl::enums::BotApp::App(app)=>json!({"state":"loaded","id":app.id.to_string(),"short_name":app.short_name,"title":app.title,"description":app.description,"photo":crate::rare_payload::photo(&app.photo),"document":app.document.as_ref().and_then(crate::rare_payload::document),"hash":app.hash.to_string()})})})
        }
        A::BoostApply(a) => json!({"type":"chat_boost_added","boost_count":a.boosts}),
        A::GiveawayLaunch(a) => json!({"type":"giveaway_created","prize_star_count":a.stars}),
        A::GiveawayResults(a) => {
            json!({"type":"giveaway_completed","is_star_giveaway":a.stars,"winner_count":a.winners_count,"unclaimed_prize_count":a.unclaimed_count})
        }
        A::CustomAction(a) => json!({"type":"custom_action","text":a.message}),
        A::ScreenshotTaken => json!({"type":"screenshot_taken"}),
        A::ContactSignUp => json!({"type":"contact_signed_up"}),
        other => {
            use tl::Serializable;
            // Keep only a constructor ID, never dump secure-value/gift/auth payloads.
            let mut prefix = ConstructorPrefix::default();
            other.serialize(&mut prefix);
            json!({"type":"unsupported","constructor_id":u32::from_le_bytes(prefix.bytes),"partial":true})
        }
    }
}
#[derive(Default)]
struct ConstructorPrefix {
    bytes: [u8; 4],
    len: usize,
}
impl Extend<u8> for ConstructorPrefix {
    fn extend<T: IntoIterator<Item = u8>>(&mut self, iter: T) {
        if self.len == 4 {
            return;
        }
        for byte in iter.into_iter().take(4 - self.len) {
            self.bytes[self.len] = byte;
            self.len += 1;
        }
    }
}
