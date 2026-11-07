use atarustia::{
    forms::{Action, Key},
    markup::{Appearance, Style},
    rich_html,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[test]
fn renderer_matches_pure_upstream_vectors() {
    let fixture: Value = serde_json::from_str(include_str!("fixtures/rich_html.json")).unwrap();
    assert_eq!(fixture["revision"], atarustia::original::REVISION);
    for vector in fixture["vectors"].as_array().unwrap() {
        let source = vector["source"].as_str().unwrap();
        assert_eq!(
            rich_html::to_rich(source),
            vector["rich"],
            "rich {}",
            vector["name"]
        );
        let result = rich_html::to_plain(source, &BTreeMap::new());
        // Upstream preserves raw KEEP start tags; html5ever canonicalizes attributes.
        // Compare spelling-only cases by Telegram text/entities, not HTML bytes.
        if matches!(
            vector["name"].as_str(),
            Some("quoted-attribute" | "uppercase")
        ) {
            let parse = |source: &str| {
                let article = grammers_client::update::Article::new(
                    "",
                    grammers_client::message::InputMessage::new().html(source),
                );
                let grammers_client::tl::enums::InputBotInlineMessage::Text(text) =
                    article.raw.send_message
                else {
                    panic!()
                };
                (text.message, text.entities)
            };
            assert_eq!(
                parse(&result.text),
                parse(vector["plain"].as_str().unwrap())
            );
        } else {
            assert_eq!(result.text, vector["plain"], "plain {}", vector["name"]);
        }
        // Raw callback data is deliberately not executed: without a declared action it is disabled.
        let rows: Vec<Vec<Value>> = result
            .rows
            .into_iter()
            .map(|r| {
                r.into_iter()
                    .map(|k| {
                        let mut object = json!({"text":k.key.text});
                        match k.key.action {
                            Some(Action::Copy { text }) => object["copy"] = text.into(),
                            Some(Action::Disabled) => object["disabled"] = true.into(),
                            _ => {}
                        }
                        if let Some(url) = k.key.url {
                            object["url"] = url.into();
                        }
                        object
                    })
                    .collect()
            })
            .collect();
        let mut expected = vector["rows"].clone();
        for row in expected.as_array_mut().unwrap() {
            for button in row.as_array_mut().unwrap() {
                if button.get("data").is_some() {
                    button.as_object_mut().unwrap().remove("data");
                    button["disabled"] = true.into();
                }
            }
        }
        assert_eq!(json!(rows), expected, "rows {}", vector["name"]);
    }
}

#[test]
fn fallback_binds_exact_declared_actions_not_duplicate_labels() {
    let first = Key {
        text: "Same".into(),
        action: Some(Action::Page { page: 1 }),
        url: None,
    };
    let second = Key {
        text: "Same".into(),
        action: Some(Action::Close),
        url: None,
    };
    let styles = [
        Appearance {
            style: Style::Success,
            emoji_id: None,
        },
        Appearance {
            style: Style::Danger,
            emoji_id: None,
        },
    ];
    let result = rich_html::fallback(
        &format!(
            "<tg-button-row>{}{}</tg-button-row>",
            atarustia::rich::marker(0),
            atarustia::rich::marker(1)
        ),
        &[first, second],
        &styles,
    );
    assert!(matches!(
        result.rows[0][0].key.action,
        Some(Action::Page { page: 1 })
    ));
    assert!(matches!(result.rows[0][1].key.action, Some(Action::Close)));
    assert!(matches!(result.rows[0][0].appearance.style, Style::Success));
    assert!(matches!(result.rows[0][1].appearance.style, Style::Danger));
    assert!(!result.text.contains("rich-fallback:"));
}

#[test]
fn non_callback_buttons_keep_identity_actions_and_appearance() {
    let keys = [
        Key {
            text: "Same".into(),
            action: Some(Action::Copy {
                text: "copy & value".into(),
            }),
            url: None,
        },
        Key {
            text: "Same".into(),
            action: Some(Action::SwitchInline {
                query: "input".into(),
                same_chat: true,
            }),
            url: None,
        },
        Key {
            text: "Same".into(),
            action: Some(Action::Disabled),
            url: None,
        },
        Key {
            text: "Same".into(),
            action: None,
            url: Some("https://example.com".into()),
        },
    ];
    let styles = vec![
        Appearance {
            style: Style::Success,
            emoji_id: Some(123)
        };
        keys.len()
    ];
    let source = format!(
        "<tg-button-row>{}</tg-button-row>",
        (0..keys.len())
            .map(atarustia::rich::marker)
            .collect::<String>()
    );
    let result = rich_html::fallback(&source, &keys, &styles);
    assert_eq!(result.rows[0].len(), 4);
    assert!(matches!(
        result.rows[0][0].key.action,
        Some(Action::Copy { .. })
    ));
    assert!(matches!(
        result.rows[0][1].key.action,
        Some(Action::SwitchInline {
            same_chat: true,
            ..
        })
    ));
    assert!(matches!(
        result.rows[0][2].key.action,
        Some(Action::Disabled)
    ));
    assert_eq!(
        result.rows[0][3].key.url.as_deref(),
        Some("https://example.com")
    );
    assert!(
        result.rows[0]
            .iter()
            .all(|key| matches!(key.appearance.style, Style::Success)
                && key.appearance.emoji_id == Some(123))
    );
}

#[test]
fn long_fallback_clips_entities_without_splitting_unicode_or_html() {
    use grammers_client::{tl, update::Article};
    let text = "🦀<&>\"\n".repeat(1600);
    let html = format!(
        "<pre><code class=\"language-rust\">{}</code></pre>",
        atarustia::output::escape(&text)
    );
    let pages = rich_html::plain_pages(&html).unwrap();
    assert!(pages.len() > 1);
    let mut joined = String::new();
    for page in pages {
        let tl::enums::InputBotInlineMessage::Text(content) =
            Article::new("", page).raw.send_message
        else {
            panic!()
        };
        assert!(content.message.encode_utf16().count() <= atarustia::output::MESSAGE_LIMIT);
        for entity in content.entities.unwrap_or_default() {
            let entity = atarustia::botapi::MessageEntity::from_tl(&entity).unwrap();
            assert_eq!(entity.offset, 0);
            assert_eq!(
                entity.length as usize,
                content.message.encode_utf16().count()
            );
            assert_eq!(entity.language.as_deref(), Some("rust"));
        }
        joined.push_str(&content.message);
    }
    assert_eq!(joined, text);
}

#[test]
fn fallback_only_retries_format_rejections_not_transport_or_authorization() {
    use atarustia::botapi::{ApiError, ErrorKind};
    for (kind, description, expected) in [
        (ErrorKind::BadRequest, "RICH_MESSAGE_NOT_ALLOWED", true),
        (
            ErrorKind::BadRequest,
            "Bad Request: can't parse entities",
            true,
        ),
        (ErrorKind::BadRequest, "PEER_ID_INVALID", false),
        (ErrorKind::RetryAfter, "RICH_MESSAGE_INVALID", false),
        (ErrorKind::Network, "rich message timeout", false),
        (ErrorKind::Forbidden, "rich message forbidden", false),
    ] {
        let error = ApiError {
            method: "editMessageText".into(),
            kind,
            description: description.into(),
            retry_after: None,
        };
        assert_eq!(rich_html::rejected_api(&error), expected);
        assert_eq!(
            rich_html::rejected(&anyhow::Error::new(error).context("edit failed")),
            expected
        );
    }
    assert!(!rich_html::rejected(&anyhow::anyhow!(
        "network RICH_MESSAGE_INVALID"
    )));
}
