use atarustia::{
    botapi::{InlineResult, SentWebAppMessage, web_app_request},
    file_id,
};
use grammers_client::tl::{self, Deserializable, Serializable};

fn article() -> InlineResult {
    InlineResult::Article {
        id: "one".into(),
        title: "Test".into(),
        text: "<b>hello</b>".into(),
        html: true,
    }
}
#[test]
fn web_app_request_preserves_opaque_id_and_validates_result() {
    let request = web_app_request("opaque:query+/=", &article()).unwrap();
    let bytes = request.to_bytes();
    let mut cursor = tl::deserialize::Cursor::from_slice(&bytes[4..]);
    let round =
        tl::functions::messages::SendWebViewResultMessage::deserialize(&mut cursor).unwrap();
    assert_eq!(round.bot_query_id, "opaque:query+/=");
    let tl::enums::InputBotInlineResult::Result(result) = round.result else {
        panic!("expected article");
    };
    assert_eq!(result.id, "one");
    let tl::enums::InputBotInlineMessage::Text(text) = result.send_message else {
        panic!();
    };
    assert_eq!(text.message, "hello");
    assert!(matches!(
        &text.entities.as_ref().unwrap()[0],
        tl::enums::MessageEntity::Bold(_)
    ));
    for id in [String::new(), "bad\nquery".into(), "x".repeat(4097)] {
        assert!(web_app_request(&id, &article()).is_err());
    }
    let invalid = InlineResult::Article {
        id: String::new(),
        title: "Test".into(),
        text: "x".into(),
        html: false,
    };
    assert!(web_app_request("query", &invalid).is_err());
}
#[test]
fn optional_web_app_inline_ids_round_trip_both_wire_variants() {
    let absent = tl::types::WebViewMessageSent { msg_id: None }.into();
    assert!(
        SentWebAppMessage::from_tl(&absent)
            .inline_message_id
            .is_none()
    );
    let ids: [tl::enums::InputBotInlineMessageId; 2] = [
        tl::types::InputBotInlineMessageId {
            dc_id: 2,
            id: 42,
            access_hash: -55,
        }
        .into(),
        tl::types::InputBotInlineMessageId64 {
            dc_id: 2,
            owner_id: 123,
            id: 42,
            access_hash: -55,
        }
        .into(),
    ];
    for id in ids {
        let sent = tl::types::WebViewMessageSent {
            msg_id: Some(id.clone()),
        }
        .into();
        let converted = SentWebAppMessage::from_tl(&sent);
        let decoded =
            file_id::decode_inline_id(converted.inline_message_id.as_ref().unwrap()).unwrap();
        assert_eq!(decoded.to_bytes(), id.to_bytes());
        let round: SentWebAppMessage =
            serde_json::from_str(&serde_json::to_string(&converted).unwrap()).unwrap();
        assert_eq!(round.inline_message_id, converted.inline_message_id);
    }
}
