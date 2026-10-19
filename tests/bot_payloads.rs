use atarustia::{
    bot_events::raw_event,
    file_id::{FileKind, FileRef},
    message_payload,
};
use grammers_client::tl;
use serde_json::{Value, json};

fn message(media: Option<tl::enums::MessageMedia>) -> tl::types::Message {
    tl::types::Message {
        out: false,
        mentioned: false,
        media_unread: false,
        silent: false,
        post: false,
        from_scheduled: false,
        legacy: false,
        edit_hide: false,
        pinned: true,
        noforwards: true,
        invert_media: false,
        offline: false,
        video_processing_pending: false,
        paid_suggested_post_stars: false,
        paid_suggested_post_ton: false,
        id: 42,
        from_id: Some(tl::types::PeerUser { user_id: 9 }.into()),
        from_boosts_applied: None,
        from_rank: None,
        peer_id: tl::types::PeerUser { user_id: 123 }.into(),
        saved_peer_id: None,
        fwd_from: None,
        via_bot_id: None,
        via_business_bot_id: Some(99),
        guestchat_via_from: None,
        reply_to: None,
        date: 10,
        message: "🦀caption".into(),
        media,
        reply_markup: None,
        entities: Some(vec![
            tl::types::MessageEntityBold {
                offset: 2,
                length: 7,
            }
            .into(),
        ]),
        views: None,
        forwards: None,
        replies: None,
        edit_date: Some(11),
        post_author: None,
        grouped_id: Some(9007199254740993),
        reactions: None,
        restriction_reason: None,
        ttl_period: None,
        quick_reply_shortcut_id: None,
        effect: None,
        factcheck: None,
        report_delivery_until_date: None,
        paid_message_stars: None,
        suggested_post: None,
        schedule_repeat_period: None,
        summary_from_language: None,
        rich_message: None,
    }
}
fn photo() -> tl::enums::MessageMedia {
    tl::types::MessageMediaPhoto {
        spoiler: true,
        live_photo: false,
        photo: Some(
            tl::types::Photo {
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
            .into(),
        ),
        ttl_seconds: Some(30),
        video: None,
    }
    .into()
}
fn document() -> tl::enums::MessageMedia {
    tl::types::MessageMediaDocument {
        nopremium: false,
        spoiler: false,
        video: false,
        round: false,
        voice: true,
        document: Some(
            tl::types::Document {
                id: 30,
                access_hash: 40,
                file_reference: vec![],
                date: 0,
                mime_type: "audio/ogg".into(),
                size: 789,
                thumbs: None,
                video_thumbs: None,
                dc_id: 2,
                attributes: vec![
                    tl::types::DocumentAttributeAudio {
                        voice: true,
                        duration: 7,
                        title: None,
                        performer: None,
                        waveform: Some(vec![1, 2]),
                    }
                    .into(),
                ],
            }
            .into(),
        ),
        alt_documents: None,
        video_cover: None,
        video_timestamp: None,
        ttl_seconds: None,
    }
    .into()
}
fn business(raw: tl::types::Message, edited: bool) -> Value {
    let event = if edited {
        tl::types::UpdateBotEditBusinessMessage {
            connection_id: "connection".into(),
            message: raw.into(),
            reply_to_message: None,
            qts: 1,
        }
        .into()
    } else {
        tl::types::UpdateBotNewBusinessMessage {
            connection_id: "connection".into(),
            message: raw.into(),
            reply_to_message: None,
            qts: 1,
        }
        .into()
    };
    let event = raw_event(&event, 99).unwrap();
    assert_eq!(
        event.name,
        if edited {
            "edited_business_message"
        } else {
            "business_message"
        }
    );
    assert_eq!(
        (event.chat, event.sender, event.message_id),
        (Some(123), Some(9), Some(42))
    );
    assert_eq!(event.data["business_connection_id"], "connection");
    event.data["message"].clone()
}
#[test]
fn business_photo_voice_and_caption_entities_survive() {
    for (media, kind) in [(photo(), FileKind::Photo), (document(), FileKind::Voice)] {
        for edited in [false, true] {
            let data = business(message(Some(media.clone())), edited);
            assert!(data.get("text").is_none());
            assert_eq!(data["caption"], "🦀caption");
            assert_eq!(data["caption_entities"][0]["offset"], 2);
            assert_eq!(data["media"]["type"], json!(kind));
            let decoded =
                FileRef::decode(data["media"]["file"]["file_id"].as_str().unwrap()).unwrap();
            assert_eq!(decoded.kind, kind);
            assert_eq!(decoded.dc, 2);
            assert_eq!(data["media_group_id"], "9007199254740993");
            assert_eq!(data["edit_date"], 11);
            assert_eq!(data["has_protected_content"], true);
        }
    }
}
#[test]
fn contact_live_location_venue_and_dice_are_structured() {
    let point: tl::enums::GeoPoint = tl::types::GeoPoint {
        lat: 55.75,
        long: 37.62,
        access_hash: 0,
        accuracy_radius: Some(12),
    }
    .into();
    let cases: Vec<(tl::enums::MessageMedia, &str)> = vec![
        (
            tl::types::MessageMediaContact {
                phone_number: "+10000000000".into(),
                first_name: "Test".into(),
                last_name: "Contact".into(),
                vcard: "VCARD".into(),
                user_id: 9,
            }
            .into(),
            "contact",
        ),
        (
            tl::types::MessageMediaGeoLive {
                geo: point.clone(),
                heading: Some(90),
                period: 60,
                proximity_notification_radius: Some(20),
            }
            .into(),
            "live_location",
        ),
        (
            tl::types::MessageMediaVenue {
                geo: point,
                title: "Venue".into(),
                address: "Address".into(),
                provider: "foursquare".into(),
                venue_id: "venue-1".into(),
                venue_type: "type".into(),
            }
            .into(),
            "venue",
        ),
        (
            tl::types::MessageMediaDice {
                value: 6,
                emoticon: "🎲".into(),
                game_outcome: None,
            }
            .into(),
            "dice",
        ),
    ];
    for (media, kind) in cases {
        let data = business(message(Some(media)), false);
        assert_eq!(data["media"]["type"], kind);
        assert_eq!(data["text"], "🦀caption");
        assert!(data["media"].get("raw").is_none());
        if kind == "live_location" {
            assert_eq!(data["media"]["location"]["latitude"], 55.75);
            assert_eq!(data["media"]["heading"], 90);
        }
    }
}
#[test]
fn reply_quotes_forwarded_anonymous_sender_and_story_ids_survive() {
    let mut raw = message(None);
    raw.from_id = Some(tl::types::PeerChannel { channel_id: 5 }.into());
    raw.reply_to = Some(
        tl::types::MessageReplyHeader {
            reply_to_scheduled: false,
            forum_topic: true,
            quote: true,
            reply_to_ephemeral: false,
            reply_to_msg_id: Some(30),
            reply_to_peer_id: Some(tl::types::PeerChannel { channel_id: 5 }.into()),
            reply_from: None,
            reply_media: None,
            reply_to_top_id: Some(20),
            quote_text: Some("🦀quote".into()),
            quote_entities: Some(vec![
                tl::types::MessageEntityBold {
                    offset: 2,
                    length: 5,
                }
                .into(),
            ]),
            quote_offset: Some(2),
            todo_item_id: None,
            poll_option: None,
        }
        .into(),
    );
    raw.fwd_from = Some(
        tl::types::MessageFwdHeader {
            imported: false,
            saved_out: false,
            from_id: None,
            from_name: Some("Hidden".into()),
            date: 1,
            channel_post: None,
            post_author: None,
            saved_from_peer: None,
            saved_from_msg_id: None,
            saved_from_id: None,
            saved_from_name: None,
            saved_date: None,
            psa_type: None,
        }
        .into(),
    );
    let data = message_payload::fields(&raw.clone().into());
    assert_eq!(data["sender_id"], -1000000000005i64);
    assert_eq!(data["reply"]["message_thread_id"], 20);
    assert_eq!(data["reply"]["quote_text"], "🦀quote");
    assert_eq!(data["forward_origin"]["sender_user_name"], "Hidden");
    assert!(data["forward_origin"]["sender_id"].is_null());
    raw.reply_to = Some(
        tl::types::MessageReplyStoryHeader {
            peer: tl::types::PeerUser { user_id: 9 }.into(),
            story_id: 88,
        }
        .into(),
    );
    assert_eq!(
        message_payload::fields(&raw.into())["reply"]["story_id"],
        88
    );
}
#[test]
fn unsupported_empty_and_service_messages_have_explicit_shapes() {
    let data = message_payload::media(&tl::enums::MessageMedia::Unsupported);
    assert_eq!(data["type"], "unsupported");
    assert_eq!(data["partial"], true);
    assert!(message_payload::media(&tl::enums::MessageMedia::Empty).is_null());
    let service: tl::enums::Message = tl::types::MessageService {
        out: false,
        mentioned: false,
        media_unread: false,
        reactions_are_possible: false,
        silent: false,
        post: false,
        legacy: false,
        id: 4,
        from_id: None,
        peer_id: tl::types::PeerChat { chat_id: 2 }.into(),
        saved_peer_id: None,
        reply_to: None,
        date: 3,
        action: tl::enums::MessageAction::Empty,
        reactions: None,
        ttl_period: None,
    }
    .into();
    let data = message_payload::fields(&service);
    assert_eq!(data["service"], true);
    assert_eq!(data["date"], 3);
    assert_eq!(data["action"]["type"], "empty");
    assert!(data["action"].get("partial").is_none());
}

#[test]
fn paid_media_preview_and_available_files_are_not_lost() {
    let paid: tl::enums::MessageMedia = tl::types::MessageMediaPaidMedia {
        stars_amount: 50,
        extended_media: vec![
            tl::types::MessageExtendedMediaPreview {
                w: Some(100),
                h: Some(200),
                thumb: None,
                video_duration: Some(7),
            }
            .into(),
            tl::types::MessageExtendedMedia { media: photo() }.into(),
            tl::types::MessageExtendedMedia { media: document() }.into(),
        ],
    }
    .into();
    let data = business(message(Some(paid)), false);
    assert_eq!(data["caption"], "🦀caption");
    assert!(data.get("text").is_none());
    let paid = &data["media"];
    assert_eq!(paid["star_count"], 50);
    assert_eq!(paid["item_count"], 3);
    assert_eq!(paid["items"][0]["type"], "preview");
    assert_eq!(paid["items"][0]["duration"], 7);
    assert!(paid["items"][0].get("file").is_none());
    assert_eq!(paid["items"][1]["type"], "photo");
    assert_eq!(paid["items"][2]["type"], "voice");
    assert!(paid["items"][1]["file"]["file_id"].is_string());
    assert_eq!(paid["partial"], false);
}

#[test]
fn paid_media_recursion_and_item_limits_are_explicit() {
    let preview: tl::enums::MessageExtendedMedia = tl::types::MessageExtendedMediaPreview {
        w: None,
        h: None,
        thumb: None,
        video_duration: None,
    }
    .into();
    let paid = tl::types::MessageMediaPaidMedia {
        stars_amount: 1,
        extended_media: vec![preview; 11],
    }
    .into();
    let data = message_payload::media(&paid);
    assert_eq!(data["items"].as_array().unwrap().len(), 10);
    assert_eq!(data["item_count"], 11);
    assert_eq!(data["partial"], true);
    let mut nested = photo();
    for _ in 0..6 {
        nested = tl::types::MessageMediaPaidMedia {
            stars_amount: 1,
            extended_media: vec![tl::types::MessageExtendedMedia { media: nested }.into()],
        }
        .into();
    }
    let data = message_payload::media(&nested);
    let mut cursor = &data;
    for _ in 0..4 {
        cursor = &cursor["items"][0];
    }
    assert_eq!(cursor["type"], "truncated");
    assert_eq!(cursor["reason"], "media_depth_limit");
    assert_eq!(data["partial"], true);
}

#[test]
fn invoice_extended_media_preserves_preview_and_thumbnail_partial_marker() {
    let invoice = tl::types::MessageMediaInvoice {
        shipping_address_requested: false,
        test: true,
        title: "Invoice".into(),
        description: "Synthetic".into(),
        photo: None,
        receipt_msg_id: None,
        currency: "XTR".into(),
        total_amount: 50,
        start_param: "start".into(),
        extended_media: Some(
            tl::types::MessageExtendedMediaPreview {
                w: Some(100),
                h: Some(100),
                thumb: Some(
                    tl::types::PhotoStrippedSize {
                        r#type: "i".into(),
                        bytes: vec![1, 2, 3],
                    }
                    .into(),
                ),
                video_duration: None,
            }
            .into(),
        ),
    }
    .into();
    let data = message_payload::media(&invoice);
    assert_eq!(data["extended_media"]["type"], "preview");
    assert_eq!(data["extended_media"]["has_thumbnail"], true);
    assert_eq!(data["extended_media"]["partial"], true);
}
#[test]
fn extended_entities_keep_mention_identity_and_quote_type() {
    use atarustia::botapi::MessageEntity;
    for (raw, expected) in [
        (
            tl::types::MessageEntityMentionName {
                offset: 0,
                length: 2,
                user_id: 9,
            }
            .into(),
            "text_mention",
        ),
        (
            tl::types::MessageEntityBlockquote {
                offset: 0,
                length: 2,
                collapsed: true,
            }
            .into(),
            "expandable_blockquote",
        ),
        (
            tl::types::MessageEntityPhone {
                offset: 0,
                length: 2,
            }
            .into(),
            "phone_number",
        ),
    ] {
        let entity = MessageEntity::from_tl(&raw).unwrap();
        assert_eq!(entity.kind, expected);
        if expected == "text_mention" {
            assert_eq!(entity.user_id, Some(9));
        }
        let round: MessageEntity =
            serde_json::from_value(serde_json::to_value(&entity).unwrap()).unwrap();
        assert_eq!(round.extract("🦀").unwrap(), "🦀");
    }
}

#[test]
fn business_reply_message_is_included_without_recursive_fetching() {
    for edited in [false, true] {
        let mut replied = message(Some(photo()));
        replied.id = 30;
        let event = if edited {
            tl::types::UpdateBotEditBusinessMessage {
                connection_id: "connection".into(),
                message: message(None).into(),
                reply_to_message: Some(replied.into()),
                qts: 1,
            }
            .into()
        } else {
            tl::types::UpdateBotNewBusinessMessage {
                connection_id: "connection".into(),
                message: message(None).into(),
                reply_to_message: Some(replied.into()),
                qts: 1,
            }
            .into()
        };
        let event = raw_event(&event, 99).unwrap();
        assert_eq!(event.data["message"]["reply_to_message"]["message_id"], 30);
        assert_eq!(
            event.data["message"]["reply_to_message"]["media"]["type"],
            "photo"
        );
    }
}

#[test]
fn banned_and_restricted_statuses_use_view_permission_not_membership() {
    for (left, view_messages, expected) in [
        (false, true, "kicked"),
        (true, false, "restricted"),
        (false, false, "restricted"),
    ] {
        let member = tl::types::ChannelParticipantBanned {
            left,
            peer: tl::types::PeerUser { user_id: 9 }.into(),
            kicked_by: 1,
            date: 2,
            rank: None,
            banned_rights: tl::types::ChatBannedRights {
                view_messages,
                send_messages: true,
                send_media: false,
                send_stickers: false,
                send_gifs: false,
                send_games: false,
                send_inline: false,
                embed_links: false,
                send_polls: false,
                change_info: false,
                invite_users: false,
                pin_messages: false,
                manage_topics: false,
                send_photos: false,
                send_videos: false,
                send_roundvideos: false,
                send_audios: false,
                send_voices: false,
                send_docs: false,
                send_plain: false,
                edit_rank: false,
                send_reactions: false,
                manage_linked_peers: false,
                until_date: 0,
            }
            .into(),
        }
        .into();
        let update = tl::types::UpdateChannelParticipant {
            via_chatlist: false,
            channel_id: 5,
            date: 2,
            actor_id: 1,
            user_id: 9,
            prev_participant: None,
            new_participant: Some(member),
            invite: None,
            qts: 1,
        }
        .into();
        let event = raw_event(&update, 99).unwrap();
        assert_eq!(event.data["new_chat_member"]["status"], expected);
        assert_eq!(event.data["new_chat_member"]["is_member"], !left);
    }
}
