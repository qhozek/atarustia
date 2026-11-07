use atarustia::{
    bot_requests::{ParseMode, Text},
    emoji,
};
use grammers_client::{tl, update::Article};
#[test]
fn every_screen_icon_matches_pinned_upstream_id_alt_and_fallback() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/emoji.json")).unwrap();
    assert_eq!(fixture["revision"], atarustia::original::REVISION);
    assert_eq!(
        fixture["icons"].as_array().unwrap().len(),
        emoji::ICONS.len()
    );
    for expected in fixture["icons"].as_array().unwrap() {
        let icon = emoji::icon(expected["name"].as_str().unwrap()).unwrap();
        assert_eq!(icon.id.to_string(), expected["id"]);
        assert_eq!(icon.alt, expected["alt"]);
        assert_eq!(icon.fallback, expected["fallback"]);
        let (text, entities) = emoji::parse(&emoji::html(icon.name, true));
        assert_eq!(text, icon.alt);
        assert!(
            matches!(&entities[..],[tl::enums::MessageEntity::CustomEmoji(entity)] if entity.document_id==icon.id && entity.offset==0 && entity.length as usize==icon.alt.encode_utf16().count())
        );
        assert_eq!(emoji::html(icon.name, false), icon.fallback);
    }
}
#[test]
fn custom_emoji_survives_html_entity_offsets_and_all_quote_spellings() {
    let source = "🦀<b><emoji document_id=6021445064096486884>💾</emoji> &amp; <tg-emoji emoji-id='123'>★</tg-emoji></b>";
    let (text, entities) = Text {
        text: source.into(),
        parse_mode: ParseMode::Html,
        entities: None,
    }
    .parse(4096, false)
    .unwrap();
    assert_eq!(text, "🦀💾 & ★");
    let entities = entities.unwrap();
    assert!(entities.iter().any(|e|matches!(e,tl::enums::MessageEntity::CustomEmoji(e) if e.document_id==6021445064096486884 && e.offset==2 && e.length==2)));
    assert!(entities.iter().any(|e|matches!(e,tl::enums::MessageEntity::CustomEmoji(e) if e.document_id==123 && e.offset==7 && e.length==1)));
    assert!(
        entities
            .iter()
            .any(|e| matches!(e,tl::enums::MessageEntity::Bold(e) if e.offset==2 && e.length==6))
    );
    let (_, plain) = emoji::parse(&emoji::normalize(source, false));
    assert!(
        !plain
            .iter()
            .any(|e| matches!(e, tl::enums::MessageEntity::CustomEmoji(_)))
    );
}
#[test]
fn escaped_tags_and_user_links_do_not_fabricate_emoji_entities() {
    let source =
        "&lt;tg-emoji emoji-id=123&gt;★&lt;/tg-emoji&gt; <a href=\"atarustia-emoji:123\">link</a>";
    let (_, entities) = emoji::parse(source);
    assert!(
        !entities
            .iter()
            .any(|e| matches!(e, tl::enums::MessageEntity::CustomEmoji(_)))
    );
}
#[test]
fn emoji_inside_a_mention_preserves_both_ranges_without_link_sentinels() {
    let (text, entities) = emoji::parse(
        "<a href=\"tg://user?id=42\">Hi <tg-emoji emoji-id=123>👤</tg-emoji> Alice</a>",
    );
    assert_eq!(text, "Hi 👤 Alice");
    assert!(entities.iter().any(|entity|matches!(entity,tl::enums::MessageEntity::MentionName(entity) if entity.offset==0 && entity.length==11 && entity.user_id==42)));
    assert!(entities.iter().any(|entity|matches!(entity,tl::enums::MessageEntity::CustomEmoji(entity) if entity.offset==3 && entity.length==2 && entity.document_id==123)));
}
#[test]
fn quickstart_retains_four_letter_logo_entities_and_start_keeps_original_symbols() {
    for english in [true, false] {
        let tl::enums::InputBotInlineMessage::Text(content) = Article::new(
            "",
            atarustia::presentation::quickstart(english, true).unwrap(),
        )
        .raw
        .send_message
        else {
            panic!()
        };
        let entities = content.entities.unwrap();
        for id in [
            5404651863054260920,
            5404474175962258931,
            5404590359122581779,
            5404537174542560251,
        ] {
            assert!(entities.iter().any(
                |e| matches!(e,tl::enums::MessageEntity::CustomEmoji(e) if e.document_id==id)
            ));
        }
        let tl::enums::InputBotInlineMessage::Text(content) = Article::new(
            "",
            atarustia::presentation::quickstart(english, false).unwrap(),
        )
        .raw
        .send_message
        else {
            panic!()
        };
        assert!(
            !content
                .entities
                .unwrap_or_default()
                .iter()
                .any(|e| matches!(e, tl::enums::MessageEntity::CustomEmoji(_)))
        );
    }
}
#[test]
fn every_custom_emoji_tag_in_original_ru_en_langpacks_produces_its_original_document_id() {
    let tag=regex::Regex::new(r#"(?s)<(?:tg-emoji|emoji)\s+(?:emoji-id|document_id)=["']?(\d+)["']?>(.*?)</(?:tg-emoji|emoji)>"#).unwrap();
    let mut checked = std::collections::BTreeSet::new();
    for source in [
        include_str!("../locales/original/ru.yml"),
        include_str!("../locales/original/en.yml"),
    ] {
        let pack: serde_json::Value = serde_yaml::from_str(source).unwrap();
        for module in pack.as_object().unwrap().values() {
            for value in module
                .as_object()
                .unwrap()
                .values()
                .filter_map(serde_json::Value::as_str)
            {
                for capture in tag.captures_iter(value) {
                    let id = capture[1].parse::<i64>().unwrap();
                    let (text, entities) = emoji::parse(&capture[0]);
                    assert!(!text.is_empty());
                    assert!(entities.iter().any(|e|matches!(e,tl::enums::MessageEntity::CustomEmoji(e) if e.document_id==id)),"{}",&capture[0]);
                    checked.insert(id);
                }
            }
        }
    }
    assert!(
        checked.len() > 30,
        "must cover the real langpack emoji inventory"
    );
}
