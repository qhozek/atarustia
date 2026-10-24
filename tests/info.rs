use atarustia::info::{self, Config, Snapshot};
use grammers_client::{message::InputMessage, tl, update::Article};
use std::collections::BTreeMap;

fn snapshot() -> Snapshot {
    Snapshot {
        owner: 123,
        name: "Alice <&> {prefix}".into(),
        prefix: "<&".into(),
        version: "0.1.0".into(),
        uptime: 90061,
        branch: "main<&>".into(),
        commit: Some("a".repeat(40)),
        up_to_date: Some(false),
        cpu: Some(12.345),
        memory: Some(1572864),
        kernel: "6.1 <kernel>".into(),
        platform: "Linux & Rust".into(),
    }
}
fn text(input: InputMessage) -> tl::types::InputBotInlineMessageText {
    let tl::enums::InputBotInlineMessage::Text(value) =
        Article::new("Info", input).raw.send_message
    else {
        panic!("expected text");
    };
    value
}
#[test]
fn default_card_matches_original_layout_and_escapes_dynamic_fields() {
    let card = snapshot().render(&Config::default(), true, false).unwrap();
    assert_eq!(card.matches("<blockquote>").count(), 4);
    assert!(card.contains("Alice &lt;&amp;&gt; {prefix}"));
    assert!(card.contains("tg://user?id=123"));
    assert!(card.contains("<i>0.1.0</i>"));
    assert!(!card.contains("atarustia ·"));
    assert!(card.contains("main&lt;&amp;&gt;"));
    assert!(card.contains("https://github.com/qhozek/atarustia/commit/"));
    assert!(card.contains("1 day(s), 1:01:01"));
    assert!(card.contains("12.35"));
    assert!(card.contains("1.5 MB"));
    assert!(card.contains("Update required"));
    assert!(card.contains("&lt;&amp;update"));
    let value =
        text(info::input(&snapshot(), &Config::default(), true, false, false, None).unwrap());
    assert!(value.message.contains("Alice <&> {prefix}"));
    assert_eq!(
        value
            .entities
            .unwrap()
            .iter()
            .filter(|e| matches!(e, tl::enums::MessageEntity::Blockquote(_)))
            .count(),
        4
    );
}
#[test]
fn standard_card_is_the_original_langpack_template_without_rebranding() {
    let snapshot = snapshot();
    let placeholders = regex::Regex::new(r"\{\w+\}").unwrap();
    for english in [false, true] {
        for rich in [false, true] {
            let rendered = snapshot.render(&Config::default(), english, rich).unwrap();
            assert!(!rendered.contains("atarustia ·"));
            assert!(!rendered.contains("<emoji"));
            let source = atarustia::original::emoji_html(
                &atarustia::original::text(
                    english,
                    "ataraxia_info",
                    if rich { "info_rich" } else { "info_message" },
                ),
                rich,
            );
            // Static labels and exact block boundaries are taken from the source, not translations of our own.
            let mut rest = rendered.as_str();
            for fragment in placeholders.split(&source) {
                if fragment.is_empty() {
                    continue;
                }
                let at = rest.find(fragment).expect("original template fragment");
                rest = &rest[at + fragment.len()..];
            }
        }
    }
}
#[test]
fn unknown_measurements_and_updates_are_not_reported_as_success_or_zero() {
    let mut s = snapshot();
    s.up_to_date = None;
    s.cpu = None;
    s.memory = None;
    s.commit = Some("not-a-sha".into());
    let card = s.render(&Config::default(), false, false).unwrap();
    assert!(card.contains("Статус обновлений недоступен"));
    assert!(!card.contains("Up-to-date"));
    assert!(!card.contains("0.00"));
    assert!(!card.contains("not-a-sha"));
    s.up_to_date = Some(true);
    assert!(
        s.render(&Config::default(), false, false)
            .unwrap()
            .contains("Актуальная версия")
    );
}
#[test]
fn custom_template_is_nonrecursive_and_rejects_unknown_python_expressions() {
    let values = BTreeMap::from([("me", "{prefix}".into()), ("prefix", ".".into())]);
    assert_eq!(
        info::substitute("{{hello}} {me} {prefix}", &values).unwrap(),
        "{hello} {prefix} ."
    );
    for template in ["{", "}", "{no_such_key}", "{me.name}", "{me[0]}", "{me!r}"] {
        assert!(
            Config {
                custom_message: Some(template.into()),
                ..Default::default()
            }
            .validate()
            .is_err()
        );
    }
    let config = Config {
        custom_message: Some("<b>{me}</b> {ram_usage}".into()),
        ..Default::default()
    };
    assert!(
        snapshot()
            .render(&config, true, false)
            .unwrap()
            .starts_with("<b><b><a")
    );
}
#[test]
fn rich_template_line_breaks_remain_line_breaks_in_plain_fallback() {
    let config = Config {
        custom_message: Some("First<br>Second<br/>Third<br />Fourth".into()),
        ..Default::default()
    };
    let value = text(info::input(&snapshot(), &config, true, false, false, None).unwrap());
    assert_eq!(value.message, "First\nSecond\nThird\nFourth");
}
#[test]
fn preview_inversion_and_rich_requests_keep_wire_fields_separate() {
    let plain = info::input(&snapshot(), &Config::default(), true, false, false, Some(5)).unwrap();
    let edit = atarustia::bot_edits::inline(
        tl::types::InputBotInlineMessageId {
            dc_id: 2,
            id: 1,
            access_hash: 3,
        }
        .into(),
        plain,
        None,
        None,
    )
    .unwrap();
    assert!(edit.invert_media);
    assert!(!edit.no_webpage);
    assert!(edit.rich_message.is_none());
    let rich = Config {
        rich_mode: true,
        ..Default::default()
    };
    let html = snapshot().render(&rich, true, true).unwrap();
    assert!(html.contains("<br>"));
    assert!(html.contains("emoji-id=\"5879770735999717115\""));
    let peer = grammers_session::types::PeerId::user(123)
        .unwrap()
        .to_ambient_ref();
    let request = atarustia::bot_edits::rich(
        peer,
        7,
        &atarustia::bot_edits::RichText::Html { source: html },
        None,
    )
    .unwrap();
    assert!(request.message.is_none());
    assert!(request.entities.is_none());
    assert!(request.media.is_none());
    assert!(request.rich_message.is_some());
}
#[test]
fn premium_entities_preserve_utf16_offsets_and_plain_fallback() {
    let premium =
        text(info::input(&snapshot(), &Config::default(), true, true, false, None).unwrap());
    let plain =
        text(info::input(&snapshot(), &Config::default(), true, false, false, None).unwrap());
    assert_eq!(premium.message, plain.message);
    let utf16: Vec<_> = premium.message.encode_utf16().collect();
    let icons = premium
        .entities
        .unwrap()
        .into_iter()
        .filter_map(|e| {
            if let tl::enums::MessageEntity::CustomEmoji(e) = e {
                Some(e)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(icons.len(), 10);
    assert_eq!(
        String::from_utf16(
            &utf16[icons[0].offset as usize..(icons[0].offset + icons[0].length) as usize]
        )
        .unwrap(),
        "👤"
    );
    assert!(
        !plain
            .entities
            .unwrap()
            .iter()
            .any(|e| matches!(e, tl::enums::MessageEntity::CustomEmoji(_)))
    );
}
#[test]
fn banner_validation_and_length_limits_run_before_telegram() {
    for url in [
        "file:///etc/passwd",
        "http://example.com/a.png",
        "https://user:secret@example.com/a.png",
        "bad",
    ] {
        assert!(
            Config {
                banner_url: url.into(),
                ..Default::default()
            }
            .validate()
            .is_err()
        );
    }
    Config {
        banner_url: String::new(),
        ..Default::default()
    }
    .validate()
    .unwrap();
    let config = Config {
        custom_message: Some("a".repeat(1100)),
        quote_media: false,
        ..Default::default()
    };
    assert!(info::input(&snapshot(), &config, true, false, false, None).is_err());
    let config = Config {
        custom_message: Some("a".repeat(4100)),
        banner_url: String::new(),
        ..Default::default()
    };
    assert!(info::input(&snapshot(), &config, true, false, false, None).is_err());
    assert_eq!(info::uptime(0), "0:00:00");
    assert_eq!(info::uptime(86399), "23:59:59");
}
