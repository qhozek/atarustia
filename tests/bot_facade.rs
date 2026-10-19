use atarustia::{
    bot_edits::RichText,
    bot_requests::{
        self, CallbackAnswer, Edit, InlineAnswer, InlineButton, ParseMode, Preview, SendOptions,
        SendRequest, Text,
    },
    bot_response,
    botapi::{ApiError, ErrorKind, InlineResult, MessageEntity},
};
use grammers_client::{
    InvocationError,
    tl::{self, Deserializable, Serializable},
};
use grammers_session::types::PeerRef;
use serde_json::json;

fn peer() -> PeerRef {
    tl::types::InputPeerUser {
        user_id: 9,
        access_hash: 456,
    }
    .into()
}
fn text() -> Text {
    Text {
        text: "<b>🦀hello</b>".into(),
        parse_mode: ParseMode::Html,
        entities: None,
    }
}
fn raw_message(id: i32, chat: i64) -> tl::enums::Message {
    tl::types::MessageService {
        out: true,
        mentioned: false,
        media_unread: false,
        reactions_are_possible: false,
        silent: false,
        post: false,
        legacy: false,
        id,
        from_id: None,
        peer_id: tl::types::PeerUser { user_id: chat }.into(),
        saved_peer_id: None,
        reply_to: None,
        date: 123,
        action: tl::enums::MessageAction::Empty,
        reactions: None,
        ttl_period: None,
    }
    .into()
}
fn updates(values: Vec<tl::enums::Update>) -> tl::enums::Updates {
    tl::types::Updates {
        updates: values,
        users: vec![],
        chats: vec![],
        date: 123,
        seq: 1,
    }
    .into()
}
fn new(id: i32, chat: i64) -> tl::enums::Update {
    tl::types::UpdateNewMessage {
        message: raw_message(id, chat),
        pts: 1,
        pts_count: 1,
    }
    .into()
}
fn ack(id: i32, random_id: i64) -> tl::enums::Update {
    tl::types::UpdateMessageId { id, random_id }.into()
}

#[test]
fn send_options_replies_protection_effect_and_preview_have_exact_fields() {
    let options = SendOptions {
        disable_notification: true,
        protect_content: true,
        reply_to_message_id: Some(42),
        message_thread_id: Some(30),
        message_effect_id: Some(123),
        ..Default::default()
    };
    let SendRequest::Text(request) =
        bot_requests::send(peer(), &text(), None, &Preview::default(), &options, 17).unwrap()
    else {
        panic!();
    };
    assert!(request.silent && request.noforwards);
    assert_eq!(request.random_id, 17);
    assert_eq!(request.effect, Some(123));
    assert_eq!(request.message, "🦀hello");
    let Some(tl::enums::InputReplyTo::Message(reply)) = request.reply_to else {
        panic!();
    };
    assert_eq!(reply.reply_to_msg_id, 42);
    assert_eq!(reply.top_msg_id, Some(30));
    let preview = Preview {
        url: Some("https://example.com".into()),
        prefer_large_media: true,
        show_above_text: true,
        ..Default::default()
    };
    let SendRequest::Media(request) =
        bot_requests::send(peer(), &text(), None, &preview, &options, 17).unwrap()
    else {
        panic!();
    };
    let tl::enums::InputMedia::WebPage(media) = request.media else {
        panic!();
    };
    assert!(media.force_large_media && media.optional);
    assert!(request.invert_media);
    let rich = RichText::Markdown {
        source: "**hello**".into(),
    };
    let SendRequest::Text(request) = bot_requests::send(
        peer(),
        &Text::default(),
        Some(&rich),
        &Preview::default(),
        &options,
        17,
    )
    .unwrap() else {
        panic!();
    };
    assert_eq!(request.message, "");
    assert!(request.entities.is_none() && request.rich_message.is_some());
}
#[test]
fn topic_only_and_input_validation_happen_before_rpc() {
    let options = SendOptions {
        message_thread_id: Some(30),
        ..Default::default()
    };
    let Some(tl::enums::InputReplyTo::Message(reply)) = options.reply() else {
        panic!();
    };
    assert_eq!(reply.reply_to_msg_id, 30);
    assert_eq!(reply.top_msg_id, None);
    assert!(
        SendOptions {
            reply_to_message_id: Some(0),
            ..Default::default()
        }
        .validate()
        .is_err()
    );
    assert!(
        SendOptions {
            business_connection_id: Some("bad\nID".into()),
            ..Default::default()
        }
        .validate()
        .is_err()
    );
    assert!(
        Preview {
            prefer_large_media: true,
            prefer_small_media: true,
            ..Default::default()
        }
        .media()
        .is_err()
    );
    assert!(
        Text {
            text: "🦀".repeat(2049),
            ..Default::default()
        }
        .parse(4096, false)
        .is_err()
    );
    assert!(Text::default().parse(4096, false).is_err());
    assert!(Text::default().parse(1024, true).is_ok());
}
#[test]
fn explicit_entities_override_parse_mode_and_validate_utf16_surrogates() {
    let entity: MessageEntity =
        serde_json::from_value(json!({"type":"bold","offset":2,"length":5})).unwrap();
    let source = Text {
        text: "🦀hello".into(),
        parse_mode: ParseMode::Html,
        entities: Some(vec![entity.clone()]),
    };
    let (parsed, entities) = source.parse(4096, false).unwrap();
    assert_eq!(parsed, "🦀hello");
    assert_eq!(
        MessageEntity::from_tl(&entities.unwrap()[0])
            .unwrap()
            .extract(&parsed)
            .unwrap(),
        "hello"
    );
    let mut invalid = entity;
    invalid.offset = 1;
    invalid.length = 1;
    assert!(invalid.to_tl("🦀hello").is_err());
    let markdown = Text {
        text: "**hello**".into(),
        parse_mode: ParseMode::Markdown,
        entities: None,
    };
    let (parsed, entities) = markdown.parse(4096, false).unwrap();
    assert_eq!(parsed, "hello");
    assert!(matches!(
        entities.unwrap()[0],
        tl::enums::MessageEntity::Bold(_)
    ));
    for entity in [
        json!({"type":"custom_emoji","offset":0,"length":2,"custom_emoji_id":"123"}),
        json!({"type":"text_mention","offset":0,"length":2,"user_id":9}),
        json!({"type":"date_time","offset":0,"length":2,"unix_time":123,"extra":{"relative":true}}),
        json!({"type":"diff_replace","offset":0,"length":2,"extra":{"old_text":"old"}}),
    ] {
        let entity: MessageEntity = serde_json::from_value(entity).unwrap();
        let converted = entity.to_tl("🦀").unwrap();
        let round = MessageEntity::from_tl(&converted).unwrap();
        assert_eq!(round.kind, entity.kind);
    }
}
#[test]
fn edit_fields_distinguish_clear_preserve_preview_and_rich() {
    let fields = Edit::Keyboard.fields(None).unwrap();
    assert!(fields.text.is_none() && fields.entities.is_none() && fields.markup.is_some());
    let fields = Edit::Caption {
        caption: Text::default(),
        show_above_media: true,
    }
    .fields(None)
    .unwrap();
    assert_eq!(fields.text.as_deref(), Some(""));
    assert_eq!(fields.entities, Some(vec![]));
    assert!(fields.invert);
    let fields = Edit::Rich {
        rich: RichText::Html {
            source: "<h2>Rich</h2>".into(),
        },
    }
    .fields(None)
    .unwrap();
    assert!(fields.text.is_none() && fields.rich.is_some());
    let request = fields.chat(peer(), 42).unwrap();
    let bytes = request.to_bytes();
    let mut c = tl::deserialize::Cursor::from_slice(&bytes[4..]);
    let round = tl::functions::messages::EditMessage::deserialize(&mut c).unwrap();
    assert!(round.message.is_none() && round.rich_message.is_some());
    assert!(
        Edit::Text {
            text: Text::default(),
            preview: Preview::default()
        }
        .fields(None)
        .is_err()
    );
}
#[test]
fn inline_callback_and_game_options_are_not_dropped() {
    let article = InlineResult::Article {
        id: "one".into(),
        title: "Test".into(),
        text: "hello".into(),
        html: false,
    };
    let options = InlineAnswer {
        cache_time: 0,
        is_personal: true,
        next_offset: Some("next".into()),
        button: Some(InlineButton::Start {
            text: "Open".into(),
            parameter: "start_me".into(),
        }),
        gallery: Some(false),
    };
    let request = options.request(9, &[article]).unwrap();
    assert!(request.private && !request.gallery);
    assert_eq!(request.next_offset.as_deref(), Some("next"));
    assert!(request.switch_pm.is_some());
    let callback = CallbackAnswer {
        text: Some("Done".into()),
        show_alert: true,
        url: Some("https://example.com".into()),
        cache_time: 30,
    }
    .request(123)
    .unwrap();
    assert!(callback.alert);
    assert_eq!(callback.cache_time, 30);
    assert_eq!(callback.url.as_deref(), Some("https://example.com"));
    let game = InlineResult::Game {
        id: "game".into(),
        short_name: "MyGame".into(),
        keyboard: vec![],
    };
    assert!(matches!(
        game.to_tl().unwrap(),
        tl::enums::InputBotInlineResult::Game(_)
    ));
    assert!(
        InlineAnswer {
            next_offset: Some("x".repeat(65)),
            ..Default::default()
        }
        .request(1, &[])
        .is_err()
    );
    assert!(
        CallbackAnswer {
            text: Some("x".repeat(201)),
            ..Default::default()
        }
        .request(1)
        .is_err()
    );
}
#[test]
fn typed_inline_kind_is_independent_from_sent_contact_content() {
    let result:InlineResult=serde_json::from_value(json!({"type":"typed","result":{"id":"x","kind":"article","title":"Contact card","content":{"type":"contact","phone_number":"+1","first_name":"Test"}}})).unwrap();
    let tl::enums::InputBotInlineResult::Result(result) = result.to_tl().unwrap() else {
        panic!();
    };
    assert_eq!(result.r#type, "article");
    assert!(matches!(
        result.send_message,
        tl::enums::InputBotInlineMessage::MediaContact(_)
    ));
}
#[test]
fn copy_is_server_side_drop_author_and_uses_target_thread() {
    let request = bot_requests::forward(
        peer(),
        peer(),
        42,
        true,
        &SendOptions {
            protect_content: true,
            message_thread_id: Some(30),
            ..Default::default()
        },
        17,
    )
    .unwrap();
    assert!(request.drop_author && request.noforwards);
    assert_eq!(request.top_msg_id, Some(30));
    assert_eq!(request.random_id, vec![17]);
    assert!(
        !bot_requests::forward(peer(), peer(), 42, false, &SendOptions::default(), 17)
            .unwrap()
            .drop_author
    );
}
#[test]
fn response_random_ids_select_correct_peer_and_album_order() {
    let result = updates(vec![
        new(43, 9),
        ack(42, 17),
        new(42, 9),
        ack(43, 18),
        new(99, 10),
    ]);
    assert_eq!(
        bot_response::sent(&result, peer(), 17, "", None)
            .unwrap()
            .message_id,
        42
    );
    let album = bot_response::album(&result, peer(), &[18, 17]).unwrap();
    assert_eq!(
        album.iter().map(|m| m.message_id).collect::<Vec<_>>(),
        vec![43, 42]
    );
    assert!(
        bot_response::sent(&updates(vec![new(42, 9), new(43, 9)]), peer(), 17, "", None).is_err()
    );
    assert!(bot_response::sent(&updates(vec![ack(42, 17)]), peer(), 17, "", None).is_err());
    assert!(bot_response::album(&result, peer(), &[17, 17]).is_err());
}
#[test]
fn short_sent_response_and_edit_absence_do_not_fabricate_success() {
    let result = tl::types::UpdateShortSentMessage {
        out: true,
        id: 42,
        pts: 1,
        pts_count: 1,
        date: 123,
        media: None,
        entities: None,
        ttl_period: None,
    }
    .into();
    let message = bot_response::sent(&result, peer(), 17, "hello", None).unwrap();
    assert_eq!(message.data["text"], "hello");
    assert_eq!(message.message_id, 42);
    assert!(bot_response::edited(&tl::enums::Updates::TooLong, peer(), 42).is_err());
    assert!(bot_response::sent(&tl::enums::Updates::TooLong, peer(), 17, "", None).is_err());
}
#[test]
fn business_wrapper_and_error_mapping_match_bot_api_contract() {
    let request = CallbackAnswer::default().request(9).unwrap();
    let wrapped = tl::functions::InvokeWithBusinessConnection {
        connection_id: "business".into(),
        query: request.clone(),
    };
    assert!(wrapped.to_bytes().ends_with(&request.to_bytes()));
    for (code, name, kind) in [
        (400, "USER_IS_BLOCKED", ErrorKind::Forbidden),
        (406, "MESSAGE_EMPTY", ErrorKind::BadRequest),
        (404, "NOT_FOUND", ErrorKind::NotFound),
        (420, "FLOOD_WAIT", ErrorKind::RetryAfter),
    ] {
        let error = InvocationError::Rpc(grammers_client::sender::RpcError {
            code,
            name: name.into(),
            value: None,
            caused_by: None,
        });
        let mapped = ApiError::from_rpc("test", &error);
        assert_eq!(mapped.kind, kind);
        if kind == ErrorKind::RetryAfter {
            assert_eq!(mapped.retry_after, Some(1));
        }
    }
}
#[test]
fn albums_validate_every_item_before_upload_and_keep_options() {
    let media = atarustia::media::Media::from_url(
        atarustia::file_id::FileKind::Photo,
        "https://example.com/photo.jpg",
    );
    let items = vec![
        bot_requests::AlbumItem {
            media: media.clone(),
            caption: text(),
        },
        bot_requests::AlbumItem {
            media,
            caption: Text::default(),
        },
    ];
    let options = SendOptions {
        disable_notification: true,
        protect_content: true,
        message_thread_id: Some(30),
        ..Default::default()
    };
    assert!(bot_requests::validate_album(&items, &options).is_ok());
    let request = bot_requests::album(
        peer(),
        vec![
            tl::types::InputSingleMedia {
                media: tl::enums::InputMedia::Empty,
                random_id: 1,
                message: String::new(),
                entities: None
            }
            .into();
            2
        ],
        &options,
    )
    .unwrap();
    assert!(request.silent && request.noforwards);
    assert!(request.reply_to.is_some());
    let mut bad = items;
    bad[1].caption.text = "x".repeat(1025);
    assert!(bot_requests::validate_album(&bad, &options).is_err());
}

#[test]
fn original_python_send_preview_callback_copy_and_game_wire_vectors_match() {
    let fixtures: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("fixtures/python_bot_facade.json")).unwrap();
    let text = Text {
        text: "hello".into(),
        ..Default::default()
    };
    let options = SendOptions {
        disable_notification: true,
        protect_content: true,
        reply_to_message_id: Some(42),
        message_thread_id: Some(30),
        message_effect_id: Some(123),
        ..Default::default()
    };
    for fixture in fixtures {
        let bytes = match fixture["name"].as_str().unwrap() {
            "send" => {
                let SendRequest::Text(r) =
                    bot_requests::send(peer(), &text, None, &Preview::default(), &options, 17)
                        .unwrap()
                else {
                    panic!();
                };
                r.to_bytes()
            }
            "preview" => {
                let SendRequest::Media(r) = bot_requests::send(
                    peer(),
                    &text,
                    None,
                    &Preview {
                        url: Some("https://example.com".into()),
                        prefer_large_media: true,
                        show_above_text: true,
                        ..Default::default()
                    },
                    &options,
                    17,
                )
                .unwrap() else {
                    panic!();
                };
                r.to_bytes()
            }
            "callback" => CallbackAnswer {
                text: Some("Done".into()),
                show_alert: true,
                url: Some("https://example.com".into()),
                cache_time: 30,
            }
            .request(123)
            .unwrap()
            .to_bytes(),
            "copy" => bot_requests::forward(peer(), peer(), 42, true, &SendOptions::default(), 17)
                .unwrap()
                .to_bytes(),
            "game" => InlineResult::Game {
                id: "game".into(),
                short_name: "MyGame".into(),
                keyboard: vec![],
            }
            .to_tl()
            .unwrap()
            .to_bytes(),
            _ => panic!(),
        };
        assert_eq!(hex::encode(bytes), fixture["hex"], "{}", fixture["name"]);
    }
}

#[test]
fn formatted_inline_markdown_rich_flags_and_upload_thumbnails_validate() {
    let content = atarustia::inline_content::InputContent::FormattedText {
        text: Text {
            text: "**hello**".into(),
            parse_mode: ParseMode::Markdown,
            entities: None,
        },
        preview: Preview::default(),
    };
    let tl::enums::InputBotInlineMessage::Text(raw) = content.to_tl(&[]).unwrap() else {
        panic!();
    };
    assert_eq!(raw.message, "hello");
    assert!(raw.entities.is_some());
    let rich = RichText::HtmlOptions {
        source: "<b>hello</b>".into(),
        is_rtl: true,
        skip_entity_detection: true,
    };
    let tl::enums::InputRichMessage::Html(raw) = rich.to_tl().unwrap() else {
        panic!();
    };
    assert!(raw.rtl && raw.noautolink);
    let media = atarustia::media::Media {
        kind: atarustia::file_id::FileKind::Video,
        source: atarustia::media::Source::Bytes {
            name: "video.mp4".into(),
            data: vec![1],
        },
        upload_thumbnail: Some(atarustia::media::Thumbnail {
            name: "thumb.jpg".into(),
            data: vec![1; 200 * 1024],
        }),
        ..Default::default()
    };
    assert!(media.validate().is_ok());
    let mut invalid = media;
    invalid.upload_thumbnail.as_mut().unwrap().data.push(1);
    assert!(invalid.validate().is_err());
    invalid.upload_thumbnail.as_mut().unwrap().data = vec![1];
    invalid.kind = atarustia::file_id::FileKind::Photo;
    assert!(invalid.validate().is_err());
}

#[test]
fn unrelated_random_ack_does_not_allow_single_message_false_positive() {
    assert!(
        bot_response::sent(
            &updates(vec![ack(42, 99), new(42, 9)]),
            peer(),
            17,
            "",
            None
        )
        .is_err()
    );
}

#[test]
fn typed_media_results_cover_all_original_media_kinds_and_cached_sources() {
    use atarustia::{
        bot_inline::{Kind, ResultSpec},
        file_id::{FileKind, FileRef},
        inline_content::InputContent,
        media::{Media, Source},
    };
    for (kind, file_kind, wire_kind) in [
        (Kind::Photo, FileKind::Photo, "photo"),
        (Kind::Gif, FileKind::Animation, "gif"),
        (Kind::Mpeg4Gif, FileKind::Animation, "gif"),
        (Kind::Video, FileKind::Video, "video"),
        (Kind::Audio, FileKind::Audio, "audio"),
        (Kind::Voice, FileKind::Voice, "voice"),
        (Kind::Document, FileKind::Document, "file"),
        (Kind::Sticker, FileKind::Sticker, "sticker"),
    ] {
        for cached in [false, true] {
            let source = if cached {
                Source::FileId {
                    file_id: FileRef {
                        kind: file_kind,
                        dc: 2,
                        id: 123,
                        access_hash: 456,
                        reference: vec![],
                        thumb: if file_kind == FileKind::Photo {
                            "x".into()
                        } else {
                            String::new()
                        },
                    }
                    .encode()
                    .unwrap(),
                }
            } else {
                Source::Url {
                    url: "https://example.com/file".into(),
                }
            };
            let spec = ResultSpec {
                id: "one".into(),
                kind,
                title: "Test".into(),
                description: None,
                url: None,
                thumbnail_url: None,
                media: Some(Box::new(Media {
                    kind: file_kind,
                    source,
                    ..Default::default()
                })),
                content: InputContent::Text {
                    text: "override".into(),
                    html: false,
                    disable_web_page_preview: false,
                },
                keyboard: vec![],
            };
            let raw = spec.to_tl().unwrap();
            let received = match raw {
                tl::enums::InputBotInlineResult::Result(r) => r.r#type,
                tl::enums::InputBotInlineResult::Photo(r) => r.r#type,
                tl::enums::InputBotInlineResult::Document(r) => r.r#type,
                _ => panic!(),
            };
            assert_eq!(received, wire_kind);
        }
    }
}

#[tokio::test]
async fn numeric_chat_resolution_reads_shared_session_without_network() {
    use grammers_session::{
        Session,
        storages::MemorySession,
        types::{PeerAuth, PeerId, PeerInfo},
    };
    use std::sync::Arc;
    let session = Arc::new(MemorySession::default());
    session
        .cache_peer(PeerInfo::User {
            id: 9,
            auth: Some(PeerAuth::from_hash(456)),
            bot: None,
            is_self: None,
        })
        .await
        .unwrap();
    let pool = grammers_client::SenderPool::new(session, 123);
    let bot = atarustia::bot::Bot {
        client: grammers_client::Client::new(pool.handle.clone()),
        id: 1,
        username: "testbot".into(),
        session: Some(pool.handle.session.clone()),
    };
    let user = bot.resolve_chat("9").await.unwrap();
    assert_eq!(user.id, PeerId::user(9).unwrap());
    assert_eq!(user.auth.hash(), 456);
    assert!(bot.resolve_chat("10").await.is_err());
    assert!(bot.resolve_chat("0").await.is_err());
    assert!(bot.resolve_chat("invalid username").await.is_err());
    assert_eq!(
        bot.resolve_chat("-123").await.unwrap().id,
        PeerId::chat(123).unwrap()
    );
}

#[test]
fn inline_media_caption_and_short_sent_media_keep_caption_semantics() {
    let content = atarustia::inline_content::InputContent::Caption {
        caption: Text {
            text: "**caption**".into(),
            parse_mode: ParseMode::Markdown,
            entities: None,
        },
        show_above_media: true,
    };
    let tl::enums::InputBotInlineMessage::MediaAuto(raw) = content.to_tl(&[]).unwrap() else {
        panic!();
    };
    assert_eq!(raw.message, "caption");
    assert!(raw.invert_media);
    assert!(raw.entities.is_some());
    let media = tl::types::MessageMediaPhoto {
        spoiler: false,
        live_photo: false,
        photo: None,
        ttl_seconds: None,
        video: None,
    }
    .into();
    let result = tl::types::UpdateShortSentMessage {
        out: true,
        id: 42,
        pts: 1,
        pts_count: 1,
        date: 123,
        media: Some(media),
        entities: None,
        ttl_period: None,
    }
    .into();
    let sent = bot_response::sent(&result, peer(), 17, "caption", None).unwrap();
    assert_eq!(sent.data["caption"], "caption");
    assert!(sent.data.get("text").is_none());
}
