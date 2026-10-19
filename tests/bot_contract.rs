//! Public-facade contract tests. No Telegram runner is started in this fixture.
use atarustia::{
    bot::Bot,
    bot_requests::{Edit, Preview, SendOptions, Text},
    bot_response,
    botapi::{
        ApiError, ApiResult, EditResult, ErrorKind, MessageId, SentGuestMessage, SentMessage,
    },
};
use grammers_client::{message::InputMessage, tl};
use grammers_session::{storages::MemorySession, types::PeerRef};
use std::{future::Future, sync::Arc};

fn bot() -> Bot {
    let pool = grammers_client::SenderPool::new(Arc::new(MemorySession::default()), 123);
    Bot {
        client: grammers_client::Client::new(pool.handle.clone()),
        id: 1,
        username: "fixturebot".into(),
        session: Some(pool.handle.session.clone()),
    }
}
fn peer() -> PeerRef {
    tl::types::InputPeerUser {
        user_id: 9,
        access_hash: 456,
    }
    .into()
}
fn source() -> atarustia::media::Source {
    atarustia::media::Source::Bytes {
        name: "bad.bin".into(),
        data: vec![],
    }
}
fn assert_method(error: ApiError, method: &str, kind: ErrorKind) {
    assert_eq!(error.method, method);
    assert_eq!(error.kind, kind);
    let round: ApiError = serde_json::from_str(&serde_json::to_string(&error).unwrap()).unwrap();
    assert_eq!(round.method, method);
    assert_eq!(round.kind, kind);
}
fn output<T>(_: impl Future<Output = ApiResult<T>>) {}

#[test]
fn every_send_family_and_native_escape_hatch_have_explicit_return_types() {
    let bot = bot();
    let options = SendOptions::default();
    let text = Text::default();
    let preview = Preview::default();
    let media = atarustia::media::Media {
        source: source(),
        ..Default::default()
    };
    output::<SentMessage>(bot.send_message(peer(), InputMessage::new()));
    output::<SentMessage>(bot.send_media(peer(), &media, InputMessage::new()));
    output::<SentMessage>(bot.send_photo(peer(), vec![], ""));
    output::<SentMessage>(bot.send_document(peer(), "file", vec![], ""));
    output::<SentMessage>(bot.send_video(peer(), source(), "", 0., 0, 0));
    output::<SentMessage>(bot.send_audio(peer(), source(), "", 0., None, None));
    output::<SentMessage>(bot.send_voice(peer(), source(), "", 0.));
    output::<SentMessage>(bot.send_animation(peer(), source(), ""));
    output::<SentMessage>(bot.send_sticker(peer(), source()));
    output::<SentMessage>(bot.send_video_note(peer(), source(), 0., 0));
    output::<Vec<SentMessage>>(bot.send_media_group(peer(), &[], None));
    output::<SentMessage>(bot.send_message_with_options(peer(), &text, None, &preview, &options));
    output::<SentMessage>(bot.send_media_with_options(peer(), &media, &text, false, &options));
    output::<Vec<SentMessage>>(bot.send_media_group_with_options(peer(), &[], &options));
    output::<EditResult>(bot.edit_message_text(peer(), 1, InputMessage::new()));
    output::<EditResult>(bot.edit_message_caption(peer(), 1, "", false, None));
    output::<EditResult>(bot.edit_message_reply_markup(peer(), 1, &[]));
    output::<EditResult>(bot.edit_with_options(peer(), 1, &Edit::Keyboard, None, None));
    output::<atarustia::botapi::ChatMember>(bot.get_chat_member(peer(), peer()));
    output::<atarustia::botapi::ChatFullInfo>(bot.get_chat(peer()));
    output::<atarustia::botapi::File>(bot.get_file("bad"));
    output::<Vec<atarustia::botapi::BotCommand>>(
        bot.get_my_commands(tl::enums::BotCommandScope::Default, ""),
    );
    let native: atarustia::native_bot::NativeBot = bot.native();
    let _: PinNative = Box::pin(native.send_message(peer(), InputMessage::new()));
    type PinNative<'a> = std::pin::Pin<
        Box<dyn Future<Output = anyhow::Result<grammers_client::message::Message>> + 'a>,
    >;
}

#[tokio::test]
async fn legacy_media_helpers_report_the_outer_method_on_validation_failure() {
    let bot = bot();
    for (method, error) in [
        (
            "send_photo",
            bot.send_photo(peer(), vec![], "").await.unwrap_err(),
        ),
        (
            "send_photo_url",
            bot.send_photo_url(peer(), "file:///secret", "")
                .await
                .unwrap_err(),
        ),
        (
            "send_document",
            bot.send_document(peer(), "../file", vec![1], "")
                .await
                .unwrap_err(),
        ),
        (
            "send_video",
            bot.send_video(peer(), source(), "", f64::NAN, 1, 1)
                .await
                .unwrap_err(),
        ),
        (
            "send_audio",
            bot.send_audio(peer(), source(), "", 0., None, None)
                .await
                .unwrap_err(),
        ),
        (
            "send_voice",
            bot.send_voice(peer(), source(), "", 0.).await.unwrap_err(),
        ),
        (
            "send_animation",
            bot.send_animation(peer(), source(), "").await.unwrap_err(),
        ),
        (
            "send_sticker",
            bot.send_sticker(peer(), source()).await.unwrap_err(),
        ),
        (
            "send_video_note",
            bot.send_video_note(peer(), source(), 0., 0)
                .await
                .unwrap_err(),
        ),
        (
            "send_media_group",
            bot.send_media_group(peer(), &[], None).await.unwrap_err(),
        ),
    ] {
        assert_method(error, method, ErrorKind::BadRequest);
    }
}

#[tokio::test]
async fn edit_options_and_operation_helpers_never_leak_anyhow_errors() {
    let bot = bot();
    assert_method(
        bot.edit_message_text(peer(), 0, InputMessage::new())
            .await
            .unwrap_err(),
        "edit_message_text",
        ErrorKind::BadRequest,
    );
    assert_method(
        bot.edit_message_reply_markup(peer(), 0, &[])
            .await
            .unwrap_err(),
        "edit_message_reply_markup",
        ErrorKind::BadRequest,
    );
    assert_method(
        bot.edit_message_caption(peer(), 42, &"x".repeat(1025), false, None)
            .await
            .unwrap_err(),
        "edit_message_caption",
        ErrorKind::BadRequest,
    );
    assert_method(
        bot.edit_inline_message("bad", InputMessage::new(), None, None)
            .await
            .unwrap_err(),
        "edit_inline_message",
        ErrorKind::BadRequest,
    );
    assert_method(
        bot.delete_message(peer(), 0).await.unwrap_err(),
        "delete_message",
        ErrorKind::BadRequest,
    );
    assert_method(
        bot.delete_messages(peer(), &[]).await.unwrap_err(),
        "delete_messages",
        ErrorKind::BadRequest,
    );
    assert_method(
        bot.pin_chat_message(peer(), 0, false).await.unwrap_err(),
        "pin_chat_message",
        ErrorKind::BadRequest,
    );
    assert_method(
        bot.answer_callback_query(1, Some(&"x".repeat(201)), false)
            .await
            .unwrap_err(),
        "answer_callback_query",
        ErrorKind::BadRequest,
    );
    assert_method(
        bot.get_file("bad").await.unwrap_err(),
        "get_file",
        ErrorKind::BadRequest,
    );
    assert_method(
        bot.download_file("bad", 100).await.unwrap_err(),
        "download_file",
        ErrorKind::BadRequest,
    );
    let bad = SendOptions {
        business_connection_id: Some("bad\nID".into()),
        ..Default::default()
    };
    assert_method(
        bot.send_message_with_options(peer(), &Text::default(), None, &Preview::default(), &bad)
            .await
            .unwrap_err(),
        "send_message_with_options",
        ErrorKind::BadRequest,
    );
}

#[test]
fn contextual_error_mapping_retains_kind_retry_and_public_method() {
    let rpc = grammers_client::InvocationError::Rpc(grammers_client::sender::RpcError {
        code: 420,
        name: "FLOOD_WAIT".into(),
        value: Some(17),
        caused_by: None,
    });
    let error = ApiError::from_error("send_photo", anyhow::Error::new(rpc).context("upload step"));
    assert_eq!(error.kind, ErrorKind::RetryAfter);
    assert_eq!(error.retry_after, Some(17));
    assert_eq!(error.method, "send_photo");
    let child = ApiError {
        method: "invoke_business".into(),
        kind: ErrorKind::Forbidden,
        description: "Forbidden".into(),
        retry_after: None,
    };
    assert_method(
        ApiError::from_error(
            "send_message_with_options",
            anyhow::Error::new(child).context("context"),
        ),
        "send_message_with_options",
        ErrorKind::Forbidden,
    );
    let response =
        bot_response::sent(&tl::enums::Updates::TooLong, peer(), 1, "", None).unwrap_err();
    assert_method(
        ApiError::from_error("send_media_group", response.context("decoding")),
        "send_media_group",
        ErrorKind::InvalidResponse,
    );
    let disk = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "fixture");
    assert_method(
        ApiError::from_error("download_to_path", disk.into()),
        "download_to_path",
        ErrorKind::LocalIo,
    );
    let socket = std::io::Error::new(std::io::ErrorKind::ConnectionReset, "fixture");
    assert_method(
        ApiError::from_error("send_photo", socket.into()),
        "send_photo",
        ErrorKind::Network,
    );
}

#[tokio::test]
async fn dropped_runner_errors_are_uniform_across_rpc_helpers() {
    // Dropping the fixture pool closes its receiver. Valid requests fail locally, never use network.
    let bot = bot();
    assert_method(
        bot.get_me().await.unwrap_err(),
        "get_me",
        ErrorKind::Network,
    );
    assert_method(bot.me().await.unwrap_err(), "me", ErrorKind::Network);
    assert_method(
        bot.send_message(peer(), InputMessage::new().text("test"))
            .await
            .unwrap_err(),
        "send_message",
        ErrorKind::Network,
    );
    assert_method(
        bot.edit_message_text(peer(), 42, InputMessage::new().text("test"))
            .await
            .unwrap_err(),
        "edit_message_text",
        ErrorKind::Network,
    );
    assert_method(
        bot.get_my_commands(tl::enums::BotCommandScope::Default, "")
            .await
            .unwrap_err(),
        "get_my_commands",
        ErrorKind::Network,
    );
    assert_method(
        bot.delete_my_commands(tl::enums::BotCommandScope::Default, "")
            .await
            .unwrap_err(),
        "delete_my_commands",
        ErrorKind::Network,
    );
    assert_method(
        bot.send_message_with_options(
            peer(),
            &Text {
                text: "test".into(),
                ..Default::default()
            },
            None,
            &Preview::default(),
            &SendOptions::default(),
        )
        .await
        .unwrap_err(),
        "send_message_with_options",
        ErrorKind::Network,
    );
    assert_method(
        bot.send_message_with_options(
            peer(),
            &Text {
                text: "test".into(),
                ..Default::default()
            },
            None,
            &Preview::default(),
            &SendOptions {
                business_connection_id: Some("business".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap_err(),
        "send_message_with_options",
        ErrorKind::Network,
    );
}

#[test]
fn every_native_public_helper_has_facade_coverage() {
    let native = include_str!("../src/native_bot.rs");
    let facade = include_str!("../src/bot.rs");
    for method in native.split("pub async fn ").skip(1) {
        let name = method.split(['(', '<']).next().unwrap();
        assert!(
            facade.contains(&format!("pub async fn {name}")),
            "native helper lacks public boundary: {name}"
        );
    }
}

#[tokio::test]
async fn timeout_is_network_error_and_local_path_failure_is_local_io() {
    let elapsed = tokio::time::timeout(std::time::Duration::ZERO, std::future::pending::<()>())
        .await
        .unwrap_err();
    assert_method(
        ApiError::from_error("download", elapsed.into()),
        "download",
        ErrorKind::Network,
    );
    let file = atarustia::file_id::FileRef {
        kind: atarustia::file_id::FileKind::Document,
        dc: 2,
        id: 123,
        access_hash: 456,
        reference: vec![],
        thumb: String::new(),
    }
    .encode()
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let error = bot()
        .download_to_path(&file, &dir.path().join("missing/file"), 100)
        .await
        .unwrap_err();
    assert_method(error, "download_to_path", ErrorKind::LocalIo);
}

#[test]
fn successful_response_types_round_trip_without_native_or_optional_album_slots() {
    let message = SentMessage {
        message_id: 42,
        chat_id: 9,
        data: serde_json::json!({"message_id":42,"chat_id":9,"text":"sent"}),
    };
    message.validate().unwrap();
    for result in [
        EditResult::Message {
            message: message.clone(),
        },
        EditResult::Acknowledged {
            chat_id: 9,
            message_id: 42,
        },
        EditResult::Inline {
            inline_message_id: "opaque-id".into(),
        },
    ] {
        let round: EditResult =
            serde_json::from_str(&serde_json::to_string(&result).unwrap()).unwrap();
        assert_eq!(round, result);
    }
    let copy = MessageId { message_id: 42 };
    assert_eq!(
        serde_json::to_value(copy).unwrap(),
        serde_json::json!({"message_id":42})
    );
    let guest = SentGuestMessage {
        inline_message_id: "id".into(),
    };
    assert_eq!(
        serde_json::to_value(guest).unwrap(),
        serde_json::json!({"inline_message_id":"id"})
    );
    let mut broken = message;
    broken.data["message_id"] = serde_json::json!(43);
    assert!(broken.validate().is_err());
}

#[test]
fn public_bot_surface_uses_apiresult_without_implicit_native_deref() {
    let source = include_str!("../src/bot.rs");
    for method in source.split("pub async fn ").skip(1) {
        let signature = method.split('{').next().unwrap();
        assert!(
            signature.contains("ApiResult<"),
            "public helper lacks uniform error contract: {signature}"
        );
        assert!(
            !signature.contains("Option<Message>"),
            "optional native message escaped facade"
        );
    }
    assert!(!source.contains("impl std::ops::Deref"));
}
