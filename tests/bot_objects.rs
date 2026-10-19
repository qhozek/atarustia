use atarustia::{
    bot_requests::{SendOptions, Text},
    bot_response,
    botapi::{ApiError, Chat, ChatFullInfo, ErrorKind, Message, User},
};
use grammers_client::tl::{self, Deserializable, Serializable};
use serde_json::{Value, json};

fn fixtures() -> Vec<Value> {
    serde_json::from_str(include_str!("fixtures/python_objects.json")).unwrap()
}
fn wire(name: &str) -> Vec<u8> {
    hex::decode(
        fixtures().iter().find(|v| v["name"] == name).unwrap()["hex"]
            .as_str()
            .unwrap(),
    )
    .unwrap()
}
fn user() -> User {
    let b = wire("user");
    let mut c = tl::deserialize::Cursor::from_slice(&b);
    User::from_tl(&tl::enums::User::deserialize(&mut c).unwrap()).unwrap()
}
fn chat() -> Chat {
    let b = wire("forum");
    let mut c = tl::deserialize::Cursor::from_slice(&b);
    Chat::from_tl(&tl::enums::Chat::deserialize(&mut c).unwrap())
}
fn message(name: &str) -> tl::enums::Message {
    let b = wire(name);
    let mut c = tl::deserialize::Cursor::from_slice(&b);
    tl::enums::Message::deserialize(&mut c).unwrap()
}

#[test]
fn original_user_and_chat_fields_names_mentions_and_active_usernames() {
    let u = user();
    assert_eq!(u.username.as_deref(), Some("alice"));
    assert_eq!(u.language_code.as_deref(), Some("ru"));
    assert_eq!(u.is_premium, Some(true));
    assert_eq!(u.full_name(), "Alice & <Smith>");
    assert_eq!(u.url(), "tg://user?id=9");
    assert_eq!(
        u.mention_html(None),
        "<a href=\"tg://user?id=9\">Alice &amp; &lt;Smith&gt;</a>"
    );
    assert!(u.mention_html(Some("<name>")).contains("&lt;name&gt;"));
    let c = chat();
    assert_eq!(c.kind, "supergroup");
    assert_eq!(c.is_forum, Some(true));
    assert_eq!(c.username.as_deref(), Some("forum"));
    assert_eq!(c.full_name(), "Forum");
    let private = Chat::from_user(&u);
    assert_eq!(private.full_name(), u.full_name());
    assert!(private.title.is_none());
    let empty = User::from_tl(&tl::types::UserEmpty { id: 10 }.into()).unwrap();
    assert_eq!(empty.id, 10);
    assert_eq!(empty.full_name(), "");
    assert_eq!(empty.can_join_groups, None);
}
#[test]
fn photo_message_has_typed_sizes_caption_entities_and_topic_root() {
    let raw = message("photo_message");
    assert_eq!(raw.to_bytes(), wire("photo_message"));
    let m = Message::from_tl(&raw, chat(), Some(user()));
    assert_eq!(m.content_type(), "photo");
    assert_eq!(m.caption.as_deref(), Some("caption"));
    assert!(m.text.is_none());
    assert_eq!(m.media.photo.len(), 2);
    assert_eq!(
        (
            m.media.photo[1].width,
            m.media.photo[1].height,
            m.media.photo[1].file_size
        ),
        (300, 400, Some(2000))
    );
    let photo = atarustia::file_id::FileRef::decode(&m.media.photo[1].file_id).unwrap();
    assert_eq!(photo.thumb, "w");
    assert_eq!(photo.id, 123456789);
    assert_eq!(m.message_thread_id, Some(30));
    assert_eq!(m.is_topic_message, Some(true));
    assert!(m.reply_to_message.is_none());
    assert_eq!(m.media_group_id.as_deref(), Some("9007199254740993"));
    assert_eq!(m.html_text().unwrap(), "<b>caption</b>");
    let round: Message = serde_json::from_value(serde_json::to_value(&m).unwrap()).unwrap();
    assert_eq!(round.media.photo.len(), 2);
    assert_eq!(round.html_text().unwrap(), m.html_text().unwrap());
}
#[test]
fn voice_message_keeps_fields_instead_of_only_generic_file() {
    let m = Message::from_tl(&message("voice_message"), Chat::from_user(&user()), None);
    assert_eq!(m.content_type(), "voice");
    let voice = m.media.voice.as_ref().unwrap();
    assert_eq!(voice.duration, 7);
    assert_eq!(voice.file.mime_type.as_deref(), Some("audio/ogg"));
    assert_eq!(voice.file.file_size, Some(789));
    assert_eq!(m.caption.as_deref(), Some("voice caption"));
    let json = serde_json::to_value(m).unwrap();
    assert!(json["voice"]["file_id"].is_string());
    assert!(json.get("document").is_none());
}
#[test]
fn message_answer_and_reply_option_defaults_preserve_explicit_overrides() {
    let m = Message::from_tl(&message("photo_message"), chat(), Some(user()));
    let answer = m.thread_options(SendOptions::default(), false);
    assert_eq!(answer.message_thread_id, Some(30));
    assert_eq!(answer.reply_to_message_id, None);
    let reply = m.thread_options(SendOptions::default(), true);
    assert_eq!(reply.message_thread_id, Some(30));
    assert_eq!(reply.reply_to_message_id, Some(42));
    let explicit = m.thread_options(
        SendOptions {
            reply_to_message_id: Some(10),
            message_thread_id: Some(11),
            ..Default::default()
        },
        true,
    );
    assert_eq!(explicit.reply_to_message_id, Some(10));
    assert_eq!(explicit.message_thread_id, Some(11));
}
#[test]
fn full_info_promotes_original_fields_from_response_details() {
    let mut info = ChatFullInfo {
        chat: chat(),
        description: Some("Description".into()),
        details: json!({"invite_link":{"invite_link":"https://t.me/+synthetic"},"linked_chat_id":-1000000000456i64,"has_protected_content":true,"available_reactions_limit":5,"accent_color_id":7}),
        ..Default::default()
    };
    info.promote_details();
    assert_eq!(info.invite_link.as_deref(), Some("https://t.me/+synthetic"));
    assert_eq!(info.has_protected_content, Some(true));
    assert_eq!(info.max_reaction_count, Some(5));
    assert_eq!(info.accent_color_id, Some(7));
    assert_eq!(info.full_name(), "Forum");
    info.chat = Chat::from_user(&user());
    info.promote_details();
    assert_eq!(info.bio.as_deref(), Some("Description"));
}
#[test]
fn response_object_preserves_typed_media_and_cached_entities() {
    let raw_user = {
        let b = wire("user");
        let mut c = tl::deserialize::Cursor::from_slice(&b);
        tl::enums::User::deserialize(&mut c).unwrap()
    };
    let raw_chat = {
        let b = wire("forum");
        let mut c = tl::deserialize::Cursor::from_slice(&b);
        tl::enums::Chat::deserialize(&mut c).unwrap()
    };
    let result = tl::types::Updates {
        updates: vec![
            tl::types::UpdateMessageId {
                id: 42,
                random_id: 17,
            }
            .into(),
            tl::types::UpdateNewChannelMessage {
                message: message("photo_message"),
                pts: 1,
                pts_count: 1,
            }
            .into(),
        ],
        users: vec![raw_user],
        chats: vec![raw_chat],
        date: 123,
        seq: 1,
    }
    .into();
    let peer: grammers_session::types::PeerRef = tl::types::InputPeerChannel {
        channel_id: 123,
        access_hash: 456,
    }
    .into();
    let sent = bot_response::sent(&result, peer, 17, "caption", None).unwrap();
    let m = sent.object().unwrap();
    assert_eq!(
        m.from_user.as_ref().unwrap().username.as_deref(),
        Some("alice")
    );
    assert_eq!(m.chat.is_forum, Some(true));
    assert_eq!(m.media.photo.len(), 2);
    let mut corrupted = sent;
    corrupted.data["object"]["message_id"] = json!(43);
    assert_eq!(
        corrupted.object().unwrap_err().kind,
        ErrorKind::InvalidResponse
    );
}
#[tokio::test]
async fn bound_object_methods_keep_error_contract_without_serializing_bot_handle() {
    use std::sync::Arc;
    let session = Arc::new(grammers_session::storages::MemorySession::default());
    let pool = grammers_client::SenderPool::new(session, 123);
    let bot = atarustia::bot::Bot {
        client: grammers_client::Client::new(pool.handle.clone()),
        id: 1,
        username: "fixture".into(),
        session: Some(pool.handle.session.clone()),
    };
    let m = Message {
        message_id: 42,
        chat: Chat {
            id: -123,
            kind: "group".into(),
            ..Default::default()
        },
        ..Default::default()
    };
    let error: ApiError = m
        .bind(&bot)
        .reply(&Text::default(), Default::default())
        .await
        .unwrap_err();
    assert_eq!(error.method, "reply");
    assert_eq!(error.kind, ErrorKind::BadRequest);
    let error = m
        .bind(&bot)
        .edit_caption(
            Text {
                text: "x".repeat(1025),
                ..Default::default()
            },
            false,
            None,
        )
        .await
        .unwrap_err();
    assert_eq!(error.method, "edit_caption");
    assert_eq!(error.kind, ErrorKind::BadRequest);
    let serialized = serde_json::to_string(&m).unwrap();
    assert!(!serialized.contains("fixture"));
    assert!(!serialized.contains("access_hash"));
}

fn document(attributes: Vec<tl::enums::DocumentAttribute>, mime: &str) -> tl::enums::MessageMedia {
    tl::types::MessageMediaDocument {
        nopremium: false,
        spoiler: false,
        video: false,
        round: false,
        voice: false,
        document: Some(
            tl::types::Document {
                id: 55,
                access_hash: 66,
                file_reference: vec![],
                date: 0,
                mime_type: mime.into(),
                size: 789,
                thumbs: None,
                video_thumbs: None,
                dc_id: 2,
                attributes,
            }
            .into(),
        ),
        alt_documents: None,
        video_cover: None,
        video_timestamp: Some(3),
        ttl_seconds: None,
    }
    .into()
}
#[test]
fn document_media_families_expose_metadata_and_serializable_file_ids() {
    let video = |round_message| {
        tl::types::DocumentAttributeVideo {
            round_message,
            supports_streaming: true,
            nosound: false,
            duration: 7.5,
            w: 320,
            h: 240,
            preload_prefix_size: None,
            video_start_ts: None,
            video_codec: None,
        }
        .into()
    };
    let filename = || {
        tl::types::DocumentAttributeFilename {
            file_name: "file.bin".into(),
        }
        .into()
    };
    let cases = vec![
        (
            document(vec![filename()], "application/octet-stream"),
            "document",
        ),
        (
            document(vec![video(false), filename()], "video/mp4"),
            "video",
        ),
        (document(vec![video(true)], "video/mp4"), "video_note"),
        (
            document(
                vec![tl::enums::DocumentAttribute::Animated, video(false)],
                "video/mp4",
            ),
            "animation",
        ),
        (
            document(
                vec![
                    tl::types::DocumentAttributeAudio {
                        voice: false,
                        duration: 7,
                        title: Some("Song".into()),
                        performer: Some("Artist".into()),
                        waveform: None,
                    }
                    .into(),
                ],
                "audio/mpeg",
            ),
            "audio",
        ),
        (
            document(
                vec![
                    tl::types::DocumentAttributeSticker {
                        mask: false,
                        alt: "★".into(),
                        stickerset: tl::types::InputStickerSetShortName {
                            short_name: "Set".into(),
                        }
                        .into(),
                        mask_coords: None,
                    }
                    .into(),
                    tl::types::DocumentAttributeImageSize { w: 512, h: 512 }.into(),
                ],
                "image/webp",
            ),
            "sticker",
        ),
        (
            document(
                vec![
                    tl::types::DocumentAttributeCustomEmoji {
                        free: true,
                        text_color: false,
                        alt: "★".into(),
                        stickerset: tl::enums::InputStickerSet::Empty,
                    }
                    .into(),
                ],
                "application/x-tgsticker",
            ),
            "sticker",
        ),
    ];
    for (raw, kind) in cases {
        let fields = atarustia::object_media::Fields::from_tl(Some(&raw));
        assert_eq!(fields.content_type(), Some(kind));
        let data = serde_json::to_value(&fields).unwrap();
        assert!(data[kind]["file_id"].is_string());
        assert_eq!(data[kind]["file_size"], 789);
        if kind == "video" {
            assert_eq!(data[kind]["duration"], 7.5);
            assert_eq!(data[kind]["start_timestamp"], 3);
        }
        if kind == "audio" {
            assert_eq!(data[kind]["performer"], "Artist");
        }
        if kind == "sticker" && data[kind]["type"] == "custom_emoji" {
            assert_eq!(data[kind]["custom_emoji_id"], "55");
            assert_eq!(data[kind]["is_animated"], true);
        }
        let round: atarustia::object_media::Fields = serde_json::from_value(data).unwrap();
        assert_eq!(round.content_type(), Some(kind));
    }
}
#[test]
fn contacts_live_locations_venue_and_dice_are_typed_and_empty_media_is_absent() {
    let geo: tl::enums::GeoPoint = tl::types::GeoPoint {
        lat: 1.,
        long: 2.,
        access_hash: 123,
        accuracy_radius: Some(5),
    }
    .into();
    let cases = vec![
        (
            tl::types::MessageMediaContact {
                phone_number: "+1".into(),
                first_name: "Name".into(),
                last_name: String::new(),
                vcard: String::new(),
                user_id: 0,
            }
            .into(),
            "contact",
        ),
        (
            tl::types::MessageMediaGeoLive {
                geo: geo.clone(),
                heading: Some(90),
                period: 60,
                proximity_notification_radius: Some(20),
            }
            .into(),
            "location",
        ),
        (
            tl::types::MessageMediaVenue {
                geo,
                title: "Venue".into(),
                address: "Address".into(),
                provider: "gplaces".into(),
                venue_id: "place".into(),
                venue_type: "food".into(),
            }
            .into(),
            "venue",
        ),
        (
            tl::types::MessageMediaDice {
                value: 6,
                emoticon: "dice".into(),
                game_outcome: None,
            }
            .into(),
            "dice",
        ),
    ];
    for (raw, kind) in cases {
        let fields = atarustia::object_media::Fields::from_tl(Some(&raw));
        assert_eq!(fields.content_type(), Some(kind));
        let data = serde_json::to_value(fields).unwrap();
        if kind == "contact" {
            assert!(data[kind]["user_id"].is_null());
        }
        if kind == "location" {
            assert_eq!(data[kind]["live_period"], 60);
            assert_eq!(data[kind]["heading"], 90);
        }
        if kind == "venue" {
            assert_eq!(data[kind]["google_place_id"], "place");
            assert!(data[kind]["foursquare_id"].is_null());
        }
    }
    assert!(
        atarustia::object_media::Fields::from_tl(Some(&tl::enums::MessageMedia::Empty))
            .content_type()
            .is_none()
    );
}
#[test]
fn mention_user_and_date_format_match_original_precedence() {
    let entity:atarustia::botapi::MessageEntity=serde_json::from_value(json!({"type":"text_mention","offset":0,"length":1,"user":{"id":9,"is_bot":false,"first_name":"Name","last_name":null,"username":null}})).unwrap();
    assert!(matches!(
        entity.to_tl("x").unwrap(),
        tl::enums::MessageEntity::MentionName(_)
    ));
    let date = tl::types::MessageEntityFormattedDate {
        relative: false,
        short_time: true,
        long_time: true,
        short_date: true,
        long_date: true,
        day_of_week: true,
        offset: 0,
        length: 1,
        date: 123,
    }
    .into();
    let converted = atarustia::botapi::MessageEntity::from_tl(&date).unwrap();
    assert_eq!(converted.date_time_format.as_deref(), Some("wDT"));
    let mut m = Message {
        text: Some("🦀hello".into()),
        ..Default::default()
    };
    m.entities
        .push(serde_json::from_value(json!({"type":"bold","offset":1,"length":1})).unwrap());
    assert_eq!(m.html_text().unwrap_err().kind, ErrorKind::BadRequest);
}

#[tokio::test]
async fn callback_inline_and_chosen_objects_validate_identity_and_answer_contract() {
    use atarustia::botapi::{CallbackQuery, ChosenInlineResult, InlineQuery};
    let raw = tl::types::UpdateBotCallbackQuery {
        query_id: 17,
        user_id: 9,
        peer: tl::types::PeerUser { user_id: 9 }.into(),
        msg_id: 42,
        chat_instance: 9007199254740993,
        data: Some(vec![0, 255, 1]),
        game_short_name: None,
    }
    .into();
    let callback = CallbackQuery::from_tl(&raw, user(), Some(Chat::from_user(&user()))).unwrap();
    assert!(callback.data.is_none());
    assert_eq!(callback.data_base64.as_deref(), Some("AP8B"));
    assert_eq!(callback.message.as_ref().unwrap().message_id, 42);
    assert!(
        CallbackQuery::from_tl(
            &raw,
            User {
                id: 10,
                ..Default::default()
            },
            None
        )
        .is_err()
    );
    let inline = tl::types::UpdateBotInlineQuery {
        query_id: 18,
        user_id: 9,
        query: "query".into(),
        geo: None,
        peer_type: Some(tl::enums::InlineQueryPeerType::Megagroup),
        offset: "next".into(),
    };
    let query = InlineQuery::from_tl(&inline, user()).unwrap();
    assert_eq!(query.chat_type.as_deref(), Some("supergroup"));
    let chosen = tl::types::UpdateBotInlineSend {
        user_id: 9,
        query: "query".into(),
        geo: None,
        id: "result".into(),
        msg_id: None,
    };
    assert_eq!(
        ChosenInlineResult::from_tl(&chosen, user())
            .unwrap()
            .result_id,
        "result"
    );
    let pool = grammers_client::SenderPool::new(
        std::sync::Arc::new(grammers_session::storages::MemorySession::default()),
        123,
    );
    let bot = atarustia::bot::Bot {
        client: grammers_client::Client::new(pool.handle.clone()),
        id: 1,
        username: "fixture".into(),
        session: Some(pool.handle.session.clone()),
    };
    let error = callback
        .answer(
            &bot,
            &atarustia::bot_requests::CallbackAnswer {
                text: Some("x".repeat(201)),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
    assert_eq!(error.method, "callback_answer");
    assert_eq!(error.kind, ErrorKind::BadRequest);
    let error = query
        .answer(
            &bot,
            &[],
            &atarustia::bot_requests::InlineAnswer {
                cache_time: -1,
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
    assert_eq!(error.method, "inline_query_answer");
    assert_eq!(error.kind, ErrorKind::BadRequest);
}

#[test]
fn service_members_sender_chat_and_full_info_field_access_are_available() {
    let raw = tl::types::MessageService {
        out: false,
        mentioned: false,
        media_unread: false,
        reactions_are_possible: false,
        silent: false,
        post: false,
        legacy: false,
        id: 42,
        from_id: Some(tl::types::PeerChannel { channel_id: 123 }.into()),
        peer_id: tl::types::PeerChannel { channel_id: 123 }.into(),
        saved_peer_id: None,
        reply_to: None,
        date: 1,
        action: tl::types::MessageActionChatAddUser { users: vec![9] }.into(),
        reactions: None,
        ttl_period: None,
    }
    .into();
    let m = Message::from_tl(&raw, chat(), None);
    assert_eq!(m.new_chat_members[0].id, 9);
    assert_eq!(m.sender_chat.as_ref().unwrap().id, -1000000000123i64);
    assert!(m.from_user.is_none());
    let info = ChatFullInfo {
        chat: chat(),
        ..Default::default()
    };
    assert_eq!(info.id, -1000000000123i64);
    assert_eq!(info.is_forum, Some(true));
    let photo = Message::from_tl(&message("photo_message"), chat(), None);
    assert_eq!(photo.photo.len(), 2);
}

#[test]
fn short_send_response_and_business_event_expose_the_same_typed_media() {
    let tl::enums::Message::Message(m) = message("voice_message") else {
        panic!();
    };
    let result = tl::types::UpdateShortSentMessage {
        out: true,
        id: 43,
        pts: 1,
        pts_count: 1,
        date: 123,
        media: m.media.clone(),
        entities: None,
        ttl_period: None,
    }
    .into();
    let peer: grammers_session::types::PeerRef = tl::types::InputPeerUser {
        user_id: 9,
        access_hash: 456,
    }
    .into();
    let sent = bot_response::sent(&result, peer, 17, "voice caption", None).unwrap();
    assert_eq!(sent.object().unwrap().voice.as_ref().unwrap().duration, 7);
    let update = tl::types::UpdateBotNewBusinessMessage {
        connection_id: "business".into(),
        message: message("voice_message"),
        reply_to_message: None,
        qts: 1,
    }
    .into();
    let event = atarustia::bot_events::raw_event(&update, 1).unwrap();
    let object: Message = serde_json::from_value(event.data["message"]["object"].clone()).unwrap();
    assert_eq!(object.business_connection_id.as_deref(), Some("business"));
    assert_eq!(object.voice.as_ref().unwrap().duration, 7);
}

#[test]
fn plain_html_formatting_escapes_text_without_entities() {
    let object = Message {
        text: Some("<unsafe> & text".into()),
        ..Default::default()
    };
    let html = object.html_text().unwrap();
    assert!(html.contains("&lt;unsafe&gt;"));
    assert!(html.contains("&amp;"));
}

#[test]
fn html_formatting_nests_equal_ranges_escapes_link_attributes_and_rejects_crossing() {
    let entity = |value| serde_json::from_value(value).unwrap();
    let mut m = Message {
        text: Some("🦀<&".into()),
        entities: vec![
            entity(json!({"type":"bold","offset":0,"length":4})),
            entity(json!({"type":"italic","offset":0,"length":4})),
            entity(
                json!({"type":"text_link","offset":2,"length":2,"url":"https://example.com/?x=1&y=2"}),
            ),
        ],
        ..Default::default()
    };
    assert_eq!(
        m.html_text().unwrap(),
        "<b><i>🦀<a href=\"https://example.com/?x=1&amp;y=2\">&lt;&amp;</a></i></b>"
    );
    m.text = Some("abcdef".into());
    m.entities = vec![
        entity(json!({"type":"bold","offset":0,"length":4})),
        entity(json!({"type":"italic","offset":2,"length":4})),
    ];
    assert_eq!(m.html_text().unwrap_err().kind, ErrorKind::BadRequest);
}
