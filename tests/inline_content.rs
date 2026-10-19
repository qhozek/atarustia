use atarustia::{
    botapi::{self, GeoPoint, InlineResult, InputContent, VenueProvider},
    file_id::{FileKind, FileRef},
    inline_content,
    markup::{Key, Kind, Style},
    media::{Media, Source},
};
use grammers_client::tl::{self, Deserializable, Serializable};
use serde_json::json;

fn point() -> GeoPoint {
    GeoPoint {
        latitude: 55.75,
        longitude: 37.62,
        horizontal_accuracy: Some(12.9),
    }
}
fn contact() -> InputContent {
    InputContent::Contact {
        phone_number: "+10000000000".into(),
        first_name: "Test".into(),
        last_name: "Contact".into(),
        vcard: "VCARD".into(),
    }
}
fn content_result(content: InputContent) -> InlineResult {
    InlineResult::Content {
        id: "one".into(),
        title: "Test".into(),
        description: None,
        url: None,
        thumbnail_url: None,
        media: None,
        content,
        keyboard: vec![],
    }
}
fn button() -> Key {
    Key {
        text: "Go".into(),
        kind: Kind::Url {
            url: "https://example.com".into(),
        },
        style: Style::Success,
        emoji_id: Some(123),
    }
}

#[test]
fn original_python_content_and_result_wire_vectors_match() {
    let vectors: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("fixtures/python_inline_content.json")).unwrap();
    for vector in vectors {
        let content: InputContent = serde_json::from_value(vector["content"].clone()).unwrap();
        let message = content.to_tl(&[]).unwrap();
        assert_eq!(
            hex::encode(message.to_bytes()),
            vector["hex"],
            "{} content",
            vector["name"]
        );
        let result = content_result(content).to_tl().unwrap();
        assert_eq!(
            hex::encode(result.to_bytes()),
            vector["result_hex"],
            "{} result",
            vector["name"]
        );
        let bytes = result.to_bytes();
        let mut cursor = tl::deserialize::Cursor::from_slice(&bytes);
        assert_eq!(
            tl::enums::InputBotInlineResult::deserialize(&mut cursor)
                .unwrap()
                .to_bytes(),
            bytes
        );
    }
}
#[test]
fn keyboards_survive_all_input_content_constructors() {
    for content in [
        contact(),
        InputContent::Text {
            text: "<b>hello</b>".into(),
            html: true,
            disable_web_page_preview: true,
        },
        InputContent::Rich {
            html: "<h2>Heading</h2>".into(),
        },
        InputContent::Location {
            point: point(),
            live_period: Some(60),
            heading: Some(90),
            proximity_alert_radius: Some(20),
        },
        InputContent::Venue {
            point: point(),
            title: "Venue".into(),
            address: "Address".into(),
            provider: None,
        },
    ] {
        let raw = content.to_tl(&[vec![button()]]).unwrap();
        let markup = match raw {
            tl::enums::InputBotInlineMessage::Text(t) => {
                assert_eq!(t.message, "hello");
                assert!(t.no_webpage);
                assert!(t.entities.is_some());
                t.reply_markup
            }
            tl::enums::InputBotInlineMessage::RichMessage(t) => t.reply_markup,
            tl::enums::InputBotInlineMessage::MediaContact(t) => t.reply_markup,
            tl::enums::InputBotInlineMessage::MediaGeo(t) => t.reply_markup,
            tl::enums::InputBotInlineMessage::MediaVenue(t) => t.reply_markup,
            _ => panic!("wrong content constructor"),
        };
        let Some(tl::enums::ReplyMarkup::ReplyInlineMarkup(markup)) = markup else {
            panic!("keyboard missing");
        };
        let tl::enums::KeyboardInlineButtonRow::Row(row) = &markup.rows[0];
        assert_eq!(row.buttons.len(), 1);
        let tl::enums::KeyboardInlineButton::Button(button) = &row.buttons[0];
        let Some(tl::enums::KeyboardButtonStyle::Style(style)) = &button.style else {
            panic!();
        };
        assert!(style.bg_success);
        assert_eq!(style.icon, Some(123));
    }
}
#[test]
fn coordinates_accuracy_and_live_options_are_bounded() {
    for (lat, long, accuracy) in [
        (f64::NAN, 0.0, None),
        (0.0, f64::INFINITY, None),
        (90.1, 0.0, None),
        (0.0, -180.1, None),
        (0.0, 0.0, Some(-1.0)),
        (0.0, 0.0, Some(1501.0)),
        (0.0, 0.0, Some(f64::NAN)),
    ] {
        assert!(
            GeoPoint {
                latitude: lat,
                longitude: long,
                horizontal_accuracy: accuracy
            }
            .to_tl()
            .is_err()
        );
    }
    let tl::enums::InputGeoPoint::Point(p) = GeoPoint {
        latitude: -90.0,
        longitude: 180.0,
        horizontal_accuracy: Some(0.0),
    }
    .to_tl()
    .unwrap() else {
        panic!();
    };
    assert_eq!(p.accuracy_radius, None);
    for (period, heading, radius) in [
        (Some(59), None, None),
        (Some(86401), None, None),
        (None, Some(0), None),
        (None, Some(361), None),
        (None, None, Some(0)),
        (None, None, Some(100001)),
    ] {
        assert!(
            InputContent::Location {
                point: point(),
                live_period: period,
                heading,
                proximity_alert_radius: radius
            }
            .to_tl(&[])
            .is_err()
        );
    }
    assert!(
        InputContent::Location {
            point: point(),
            live_period: Some(i32::MAX),
            heading: Some(360),
            proximity_alert_radius: Some(100000)
        }
        .to_tl(&[])
        .is_ok()
    );
}
#[test]
fn invalid_text_contacts_venues_and_keyboards_fail_without_network() {
    for content in [
        InputContent::Text {
            text: String::new(),
            html: false,
            disable_web_page_preview: false,
        },
        InputContent::Text {
            text: "🦀".repeat(2049),
            html: false,
            disable_web_page_preview: false,
        },
        InputContent::Rich {
            html: String::new(),
        },
        InputContent::Contact {
            phone_number: "bad\nphone".into(),
            first_name: "Test".into(),
            last_name: String::new(),
            vcard: String::new(),
        },
        InputContent::Contact {
            phone_number: "+1".into(),
            first_name: String::new(),
            last_name: String::new(),
            vcard: String::new(),
        },
        InputContent::Contact {
            phone_number: "+1".into(),
            first_name: "Test".into(),
            last_name: String::new(),
            vcard: "x".repeat(2049),
        },
        InputContent::Venue {
            point: point(),
            title: "Venue".into(),
            address: String::new(),
            provider: None,
        },
        InputContent::Venue {
            point: point(),
            title: "Venue".into(),
            address: "Address".into(),
            provider: Some(VenueProvider::Foursquare {
                id: String::new(),
                category: String::new(),
            }),
        },
    ] {
        assert!(content.to_tl(&[]).is_err());
    }
    assert!(contact().to_tl(&[vec![button(); 9]]).is_err());
}
#[test]
fn result_metadata_duplicate_ids_and_json_round_trip() {
    let mut result = content_result(contact());
    if let InlineResult::Content {
        description,
        url,
        thumbnail_url,
        keyboard,
        ..
    } = &mut result
    {
        *description = Some("Description".into());
        *url = Some("https://example.com/contact".into());
        *thumbnail_url = Some("https://example.com/thumb.jpg".into());
        keyboard.push(vec![button()]);
    }
    let round: InlineResult =
        serde_json::from_str(&serde_json::to_string(&result).unwrap()).unwrap();
    let tl::enums::InputBotInlineResult::Result(raw) = round.to_tl().unwrap() else {
        panic!();
    };
    assert_eq!(raw.r#type, "contact");
    assert_eq!(raw.description.as_deref(), Some("Description"));
    assert!(raw.thumb.is_some());
    assert!(botapi::inline_results(&[round.clone(), round]).is_err());
    assert!(
        botapi::inline_results(&[
            result,
            InlineResult::Article {
                id: "one".into(),
                title: "Test".into(),
                text: "hello".into(),
                html: false
            }
        ])
        .is_err()
    );
    let mut value = json!({"type":"content","id":"x","title":"x","content":{"type":"contact","phone_number":"+1","first_name":"Test"}});
    assert!(
        serde_json::from_value::<InlineResult>(value.clone())
            .unwrap()
            .to_tl()
            .is_ok()
    );
    value["content"]["typo"] = json!(true);
    assert!(serde_json::from_value::<InlineResult>(value).is_err());
    assert!(
        inline_content::result(
            "one",
            "Test",
            None,
            Some("file:///secret"),
            None,
            None,
            &contact(),
            &[]
        )
        .is_err()
    );
}
#[test]
fn content_override_preserves_cached_and_url_media_reference() {
    for kind in [FileKind::Photo, FileKind::Voice] {
        let file = FileRef {
            kind,
            dc: 2,
            id: 123,
            access_hash: 456,
            reference: vec![],
            thumb: if kind == FileKind::Photo {
                "x".into()
            } else {
                String::new()
            },
        };
        let media = Media {
            kind,
            source: Source::FileId {
                file_id: file.encode().unwrap(),
            },
            ..Default::default()
        };
        let result = inline_content::result(
            "one",
            "Test",
            None,
            None,
            None,
            Some(&media),
            &contact(),
            &[vec![button()]],
        )
        .unwrap();
        let send_message = match result {
            tl::enums::InputBotInlineResult::Photo(p) => {
                let tl::enums::InputPhoto::Photo(photo) = p.photo else {
                    panic!();
                };
                assert_eq!(photo.id, 123);
                p.send_message
            }
            tl::enums::InputBotInlineResult::Document(d) => {
                let tl::enums::InputDocument::Document(p) = d.document else {
                    panic!();
                };
                assert_eq!(p.access_hash, 456);
                d.send_message
            }
            _ => panic!(),
        };
        assert!(matches!(
            send_message,
            tl::enums::InputBotInlineMessage::MediaContact(_)
        ));
        assert!(
            inline_content::result(
                "one",
                "Test",
                None,
                None,
                None,
                Some(&media),
                &InputContent::Rich {
                    html: "hello".into()
                },
                &[]
            )
            .is_err()
        );
    }
    let media = Media::from_url(FileKind::Photo, "https://example.com/photo.jpg");
    assert!(
        inline_content::result(
            "one",
            "Test",
            None,
            None,
            None,
            Some(&media),
            &contact(),
            &[]
        )
        .is_ok()
    );
    let bytes = Media {
        kind: FileKind::Photo,
        source: Source::Bytes {
            name: "photo.jpg".into(),
            data: vec![1, 2, 3],
        },
        ..Default::default()
    };
    assert!(
        inline_content::result(
            "one",
            "Test",
            None,
            None,
            None,
            Some(&bytes),
            &contact(),
            &[]
        )
        .is_err()
    );
}
#[test]
fn web_app_answers_accept_non_text_content_through_shared_pipeline() {
    let result = content_result(InputContent::Location {
        point: point(),
        live_period: None,
        heading: None,
        proximity_alert_radius: None,
    });
    let request = botapi::web_app_request("query", &result).unwrap();
    let tl::enums::InputBotInlineResult::Result(r) = request.result else {
        panic!();
    };
    assert_eq!(r.r#type, "geo");
    assert!(matches!(
        r.send_message,
        tl::enums::InputBotInlineMessage::MediaGeo(_)
    ));
}

#[test]
fn web_page_preview_flags_disable_path_and_keyboard_are_preserved() {
    for disabled in [false, true] {
        let content = InputContent::WebPage {
            text: "<b>hello</b>".into(),
            html: true,
            url: "https://example.com".into(),
            is_disabled: disabled,
            prefer_large_media: true,
            prefer_small_media: false,
            show_above_text: true,
        };
        match content.to_tl(&[vec![button()]]).unwrap() {
            tl::enums::InputBotInlineMessage::MediaWebPage(p) => {
                assert!(!disabled);
                assert!(
                    p.optional && p.invert_media && p.force_large_media && !p.force_small_media
                );
                assert!(p.reply_markup.is_some() && p.entities.is_some());
                assert_eq!(p.message, "hello");
            }
            tl::enums::InputBotInlineMessage::Text(p) => {
                assert!(disabled && p.no_webpage && p.invert_media);
                assert!(p.reply_markup.is_some() && p.entities.is_some());
            }
            _ => panic!(),
        }
    }
    let content = InputContent::WebPage {
        text: "hello".into(),
        html: false,
        url: "file:///secret".into(),
        is_disabled: false,
        prefer_large_media: false,
        prefer_small_media: false,
        show_above_text: false,
    };
    assert!(content.to_tl(&[]).is_err());
    let conflict = InputContent::WebPage {
        text: "hello".into(),
        html: false,
        url: "https://example.com".into(),
        is_disabled: false,
        prefer_large_media: true,
        prefer_small_media: true,
        show_above_text: false,
    };
    assert!(conflict.to_tl(&[]).is_err());
}

#[test]
fn examples_and_unsupported_metadata_are_validated() {
    let examples: Vec<InlineResult> =
        serde_json::from_str(include_str!("../examples/inline_content.json")).unwrap();
    assert_eq!(botapi::inline_results(&examples).unwrap().len(), 3);
    assert!(inline_content::result("one", "", None, None, None, None, &contact(), &[]).is_err());
    assert!(
        inline_content::result(
            "one",
            "Test",
            Some(&"x".repeat(1025)),
            None,
            None,
            None,
            &contact(),
            &[]
        )
        .is_err()
    );
    let photo = Media {
        kind: FileKind::Photo,
        source: Source::FileId {
            file_id: FileRef {
                kind: FileKind::Photo,
                dc: 2,
                id: 1,
                access_hash: 2,
                reference: vec![],
                thumb: "x".into(),
            }
            .encode()
            .unwrap(),
        },
        ..Default::default()
    };
    assert!(
        inline_content::result(
            "one",
            "Test",
            Some("not representable"),
            None,
            None,
            Some(&photo),
            &contact(),
            &[]
        )
        .is_err()
    );
    let document = Media {
        kind: FileKind::Document,
        source: Source::FileId {
            file_id: FileRef {
                kind: FileKind::Document,
                dc: 2,
                id: 1,
                access_hash: 2,
                reference: vec![],
                thumb: String::new(),
            }
            .encode()
            .unwrap(),
        },
        ..Default::default()
    };
    assert!(
        inline_content::result(
            "one",
            "Test",
            None,
            Some("https://example.com"),
            None,
            Some(&document),
            &contact(),
            &[]
        )
        .is_err()
    );
    assert!(
        inline_content::result(
            "one",
            "Test",
            Some("description"),
            None,
            None,
            Some(&document),
            &contact(),
            &[]
        )
        .is_ok()
    );
}
