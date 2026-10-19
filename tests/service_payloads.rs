use atarustia::{bot_events, message_payload, service_payload};
use grammers_client::tl::{self, Deserializable, Serializable};
use serde_json::{Value, json};

fn service(
    action: tl::enums::MessageAction,
    reply_to: Option<tl::enums::MessageReplyHeader>,
) -> tl::enums::Message {
    tl::types::MessageService {
        out: false,
        mentioned: false,
        media_unread: false,
        reactions_are_possible: false,
        silent: false,
        post: false,
        legacy: false,
        id: 42,
        from_id: Some(tl::types::PeerUser { user_id: 9 }.into()),
        peer_id: tl::types::PeerChat { chat_id: 123 }.into(),
        saved_peer_id: None,
        reply_to,
        date: 10,
        action,
        reactions: None,
        ttl_period: None,
    }
    .into()
}
fn reply(id: i32) -> tl::enums::MessageReplyHeader {
    tl::types::MessageReplyHeader {
        reply_to_scheduled: false,
        forum_topic: true,
        quote: false,
        reply_to_ephemeral: false,
        reply_to_msg_id: Some(id),
        reply_to_peer_id: None,
        reply_from: None,
        reply_media: None,
        reply_to_top_id: Some(20),
        quote_text: None,
        quote_entities: None,
        quote_offset: None,
        todo_item_id: None,
        poll_option: None,
    }
    .into()
}
#[test]
fn original_python_action_wire_decodes_into_expected_fields() {
    let fixtures: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/python_service_actions.json")).unwrap();
    for fixture in fixtures {
        let bytes = hex::decode(fixture["hex"].as_str().unwrap()).unwrap();
        let mut cursor = tl::deserialize::Cursor::from_slice(&bytes);
        let action = tl::enums::MessageAction::deserialize(&mut cursor).unwrap();
        assert_eq!(action.to_bytes(), bytes);
        assert_eq!(
            service_payload::action(&action, None),
            fixture["expected"],
            "{}",
            fixture["name"]
        );
        let payload = message_payload::fields(&service(action, None));
        assert_eq!(payload["action"], fixture["expected"]);
        assert_eq!(payload["service"], true);
        assert_eq!(payload["chat_id"], -123);
    }
}
#[test]
fn service_context_and_pinned_message_are_preserved_in_business_events() {
    let message = service(tl::enums::MessageAction::PinMessage, Some(reply(30)));
    for edited in [false, true] {
        let update = if edited {
            tl::types::UpdateBotEditBusinessMessage {
                connection_id: "connection".into(),
                message: message.clone(),
                reply_to_message: None,
                qts: 1,
            }
            .into()
        } else {
            tl::types::UpdateBotNewBusinessMessage {
                connection_id: "connection".into(),
                message: message.clone(),
                reply_to_message: None,
                qts: 1,
            }
            .into()
        };
        let event = bot_events::raw_event(&update, 99).unwrap();
        assert_eq!(
            (event.chat, event.sender, event.message_id),
            (Some(-123), Some(9), Some(42))
        );
        assert_eq!(event.data["message"]["action"]["message_id"], 30);
        assert_eq!(event.data["message"]["reply"]["message_thread_id"], 20);
    }
}
#[test]
fn membership_migration_and_simple_actions_have_structured_types() {
    let cases: Vec<(tl::enums::MessageAction, &str)> = vec![
        (
            tl::types::MessageActionChatCreate {
                title: "Group".into(),
                users: vec![9],
            }
            .into(),
            "chat_created",
        ),
        (
            tl::types::MessageActionChannelCreate {
                title: "Channel".into(),
            }
            .into(),
            "channel_created",
        ),
        (
            tl::types::MessageActionChatEditTitle {
                title: "New title".into(),
            }
            .into(),
            "chat_title_changed",
        ),
        (
            tl::types::MessageActionChatDeleteUser { user_id: 9 }.into(),
            "member_removed",
        ),
        (
            tl::types::MessageActionChatJoinedByLink { inviter_id: 1 }.into(),
            "joined_by_link",
        ),
        (
            tl::enums::MessageAction::ChatJoinedByRequest,
            "joined_by_request",
        ),
        (
            tl::enums::MessageAction::ChatDeletePhoto,
            "chat_photo_deleted",
        ),
        (
            tl::types::MessageActionChannelMigrateFrom {
                title: "Old".into(),
                chat_id: 123,
            }
            .into(),
            "chat_migrated_from",
        ),
        (tl::enums::MessageAction::HistoryClear, "history_cleared"),
        (
            tl::enums::MessageAction::ScreenshotTaken,
            "screenshot_taken",
        ),
        (tl::enums::MessageAction::ContactSignUp, "contact_signed_up"),
        (
            tl::types::MessageActionCustomAction {
                message: "Custom".into(),
            }
            .into(),
            "custom_action",
        ),
    ];
    for (action, kind) in cases {
        let payload = service_payload::action(&action, None);
        assert_eq!(payload["type"], kind);
        assert!(payload.get("raw").is_none());
        if kind == "chat_migrated_from" {
            assert_eq!(payload["chat_id"], -123);
        }
    }
}
#[test]
fn topic_edit_preserves_absence_false_and_emoji_removal_distinctly() {
    let absent = tl::types::MessageActionTopicEdit {
        title: None,
        icon_emoji_id: None,
        closed: None,
        hidden: None,
    }
    .into();
    let edited = tl::types::MessageActionTopicEdit {
        title: Some(String::new()),
        icon_emoji_id: Some(0),
        closed: Some(false),
        hidden: Some(true),
    }
    .into();
    let a = service_payload::action(&absent, None);
    let b = service_payload::action(&edited, None);
    assert!(a["is_closed"].is_null());
    assert_eq!(b["is_closed"], false);
    assert!(a["icon_custom_emoji_id"].is_null());
    assert_eq!(b["icon_custom_emoji_id"], "0");
    assert_eq!(b["name"], "");
    assert_eq!(b["is_hidden"], true);
}
#[test]
fn payment_order_info_refunds_and_binary_payload_are_lossless() {
    let charge: tl::enums::PaymentCharge = tl::types::PaymentCharge {
        id: "tg".into(),
        provider_charge_id: "provider".into(),
    }
    .into();
    let action = tl::types::MessageActionPaymentSentMe {
        recurring_init: false,
        recurring_used: true,
        currency: "XTR".into(),
        total_amount: 123,
        payload: vec![0, 255, 1],
        info: Some(
            tl::types::PaymentRequestedInfo {
                name: Some("Synthetic".into()),
                phone: Some("+10000000000".into()),
                email: None,
                shipping_address: Some(
                    tl::types::PostAddress {
                        street_line1: "Line".into(),
                        street_line2: String::new(),
                        city: "City".into(),
                        state: String::new(),
                        country_iso2: "RU".into(),
                        post_code: "123".into(),
                    }
                    .into(),
                ),
            }
            .into(),
        ),
        shipping_option_id: Some("standard".into()),
        charge: charge.clone(),
        subscription_until_date: None,
    }
    .into();
    let payload = service_payload::action(&action, None);
    assert_eq!(payload["invoice_payload"], "AP8B");
    assert_eq!(
        payload["order_info"]["shipping_address"]["country_code"],
        "RU"
    );
    assert_eq!(payload["is_first_recurring"], false);
    assert_eq!(payload["is_recurring"], true);
    let refund = tl::types::MessageActionPaymentRefunded {
        peer: tl::types::PeerUser { user_id: 9 }.into(),
        currency: "XTR".into(),
        total_amount: 123,
        payload: Some(vec![0, 255, 1]),
        charge,
    }
    .into();
    let refund = service_payload::action(&refund, None);
    assert_eq!(refund["invoice_payload"], "AP8B");
    assert_eq!(refund["charge"]["telegram_payment_charge_id"], "tg");
    let sent = tl::types::MessageActionPaymentSent {
        recurring_init: false,
        recurring_used: false,
        currency: "XTR".into(),
        total_amount: 123,
        invoice_slug: Some("invoice".into()),
        subscription_until_date: None,
    }
    .into();
    assert_eq!(
        service_payload::action(&sent, None)["invoice_slug"],
        "invoice"
    );
}
#[test]
fn call_ids_game_ids_and_proximity_preserve_large_ids_without_access_hashes() {
    let call: tl::enums::InputGroupCall = tl::types::InputGroupCall {
        id: 9007199254740993,
        access_hash: 654,
    }
    .into();
    let started = tl::types::MessageActionGroupCall {
        call: call.clone(),
        duration: None,
    }
    .into();
    let ended = tl::types::MessageActionGroupCall {
        call: call.clone(),
        duration: Some(0),
    }
    .into();
    assert_eq!(
        service_payload::action(&started, None)["type"],
        "video_chat_started"
    );
    assert_eq!(
        service_payload::action(&ended, None)["type"],
        "video_chat_ended"
    );
    let scheduled = tl::types::MessageActionGroupCallScheduled {
        call: call.clone(),
        schedule_date: 123,
    }
    .into();
    let p = service_payload::action(&scheduled, None);
    assert_eq!(p["call"]["id"], "9007199254740993");
    assert!(p["call"].get("access_hash").is_none());
    let invited = tl::types::MessageActionInviteToGroupCall {
        call,
        users: vec![9, 10],
    }
    .into();
    assert_eq!(
        service_payload::action(&invited, None)["user_ids"],
        json!([9, 10])
    );
    let game = tl::types::MessageActionGameScore {
        game_id: 9007199254740993,
        score: 7,
    }
    .into();
    assert_eq!(
        service_payload::action(&game, None)["game_id"],
        "9007199254740993"
    );
    let proximity = tl::types::MessageActionGeoProximityReached {
        from_id: tl::types::PeerUser { user_id: 9 }.into(),
        to_id: tl::types::PeerChannel { channel_id: 5 }.into(),
        distance: 20,
    }
    .into();
    assert_eq!(
        service_payload::action(&proximity, None)["watcher_id"],
        -1000000000005i64
    );
    let phone = tl::types::MessageActionPhoneCall {
        video: true,
        call_id: 9007199254740993,
        reason: Some(tl::enums::PhoneCallDiscardReason::Busy),
        duration: None,
    }
    .into();
    assert_eq!(service_payload::action(&phone, None)["reason"], "busy");
}
#[test]
fn unsupported_action_remains_explicit_without_sensitive_raw_dump() {
    let action = tl::types::MessageActionSecureValuesSentMe {
        values: vec![],
        credentials: tl::types::SecureCredentialsEncrypted {
            data: b"private-data".to_vec(),
            hash: b"private-hash".to_vec(),
            secret: b"private-secret".to_vec(),
        }
        .into(),
    }
    .into();
    let payload = service_payload::action(&action, None);
    assert_eq!(payload["type"], "passport_data_received");
    assert_eq!(payload["partial"], true);
    assert_eq!(payload["redacted"], true);
    assert!(
        !serde_json::to_string(&payload)
            .unwrap()
            .contains("private-secret")
    );
    assert!(payload.get("raw").is_none());
}

#[test]
fn forum_reply_without_top_id_uses_topic_root_as_thread_id() {
    let tl::enums::MessageReplyHeader::Header(mut header) = reply(30) else {
        panic!();
    };
    header.reply_to_top_id = None;
    let message = service(tl::enums::MessageAction::PinMessage, Some(header.into()));
    let payload = message_payload::fields(&message);
    assert_eq!(payload["reply"]["message_thread_id"], 30);
    assert_eq!(payload["action"]["message_id"], 30);
}

#[test]
fn photo_boost_and_giveaway_actions_preserve_payloads() {
    let photo = tl::types::Photo {
        has_stickers: false,
        id: 123456789,
        access_hash: -555,
        file_reference: vec![1, 2, 255],
        date: 0,
        sizes: vec![
            tl::types::PhotoSize {
                r#type: "w".into(),
                w: 100,
                h: 100,
                size: 789,
            }
            .into(),
        ],
        video_sizes: None,
        dc_id: 2,
    }
    .into();
    let action = tl::types::MessageActionChatEditPhoto { photo }.into();
    let payload = service_payload::action(&action, None);
    assert_eq!(payload["type"], "chat_photo_changed");
    let file =
        atarustia::file_id::FileRef::decode(payload["photo"]["file_id"].as_str().unwrap()).unwrap();
    assert_eq!(file.id, 123456789);
    assert_eq!(file.kind, atarustia::file_id::FileKind::Photo);
    let boost = tl::types::MessageActionBoostApply { boosts: 3 }.into();
    assert_eq!(service_payload::action(&boost, None)["boost_count"], 3);
    let launch = tl::types::MessageActionGiveawayLaunch { stars: Some(50) }.into();
    assert_eq!(
        service_payload::action(&launch, None)["prize_star_count"],
        50
    );
    let results = tl::types::MessageActionGiveawayResults {
        stars: true,
        winners_count: 5,
        unclaimed_count: 2,
    }
    .into();
    assert_eq!(
        service_payload::action(&results, None)["unclaimed_prize_count"],
        2
    );
}
