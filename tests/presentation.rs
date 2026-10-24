use atarustia::{original, presentation};
use grammers_client::{message::InputMessage, tl, update::Article};
use sha2::{Digest, Sha256};
fn content(message: InputMessage) -> tl::types::InputBotInlineMessageText {
    let tl::enums::InputBotInlineMessage::Text(text) =
        Article::new("fixture", message).raw.send_message
    else {
        panic!("text")
    };
    text
}
#[test]
fn original_telegram_artwork_is_byte_identical() {
    for (bytes, expected) in [
        (
            include_bytes!("../assets/ataraxia.png").as_slice(),
            "3607a27e80fb10544e9bdeba1b38b268dead2ecfb2c8671fe3d1ca119cdf22d9",
        ),
        (
            include_bytes!("../assets/bot_pfp.png").as_slice(),
            "7fffd74233a6343b48dba8bd13a7b92766c7c2b5a97d41744a13e53aee688102",
        ),
        (
            include_bytes!("../assets/ataraxia_backup.png").as_slice(),
            "0df32456d78cb5aa47b6f48235f47271252098410d3a9efd30fd1ed007963d66",
        ),
    ] {
        assert_eq!(hex::encode(Sha256::digest(bytes)), expected);
    }
}
#[test]
fn bot_and_forum_use_original_names_not_rust_branding() {
    assert_eq!(original::BOT_NAME, "⛈️ ataraxia");
    assert_eq!(original::INLINE_PLACEHOLDER, "user@ataraxia:~$");
    assert_eq!(original::FORUM_TITLE, "ataraxia-userbot");
    assert_eq!(
        original::FORUM_DESCRIPTION,
        "⛈️ Content related to ataraxia will be here"
    );
    let re = regex::Regex::new(r"^[A-Za-z]+_[A-Za-z0-9]{6}_bot$").unwrap();
    for _ in 0..100 {
        let name = original::bot_username();
        assert!(re.is_match(&name));
        assert!(!name.contains("atarustia"));
    }
}
#[test]
fn source_formatter_never_expands_inserted_user_text() {
    assert_eq!(
        original::format(
            "{} {name}",
            &["{name}".into()],
            &[("name", "<Alice>".into())]
        ),
        "{name} <Alice>"
    );
    assert_eq!(
        original::emoji_html("<emoji document_id=123>⚡</emoji>", false),
        "⚡"
    );
    assert_eq!(
        original::emoji_html("<emoji document_id=123>⚡</emoji>", true),
        "<tg-emoji emoji-id=\"123\">⚡</tg-emoji>"
    );
}
#[test]
fn quickstart_has_original_guide_languages_community_buttons_and_styles() {
    for english in [false, true] {
        let body = content(presentation::quickstart(english, false).unwrap());
        assert!(!body.message.contains("atarustia"));
        assert!(body.message.contains(".help"));
        assert!(body.message.contains(".dlm"));
        let tl::enums::ReplyMarkup::ReplyInlineMarkup(markup) = body.reply_markup.unwrap() else {
            panic!("markup")
        };
        assert_eq!(markup.rows.len(), 2);
        let tl::enums::KeyboardInlineButtonRow::Row(languages) = &markup.rows[0];
        assert_eq!(languages.buttons.len(), 2);
        for (button, language) in languages.buttons.iter().zip(["en", "ru"]) {
            let tl::enums::KeyboardInlineButton::Button(button) = button;
            let tl::enums::InlineButtonType::Callback(callback) = &button.r#type else {
                panic!("callback")
            };
            assert_eq!(
                callback.data,
                format!("ataraxia/lang/{language}").as_bytes()
            );
        }
        let tl::enums::KeyboardInlineButtonRow::Row(community) = &markup.rows[1];
        assert_eq!(community.buttons.len(), 2);
        for button in &community.buttons {
            let tl::enums::KeyboardInlineButton::Button(button) = button;
            let Some(tl::enums::KeyboardButtonStyle::Style(style)) = &button.style else {
                panic!("style")
            };
            assert!(style.bg_primary);
            assert!(style.icon.is_some());
        }
    }
}
#[test]
fn start_card_is_original_banner_caption_not_help_catalogue() {
    for english in [true, false] {
        let body = content(presentation::start_card(english).unwrap());
        assert!(!body.message.contains("atarustia"));
        assert!(!body.message.contains("Native Rust"));
        assert!(body.no_webpage);
        let tl::enums::ReplyMarkup::ReplyInlineMarkup(markup) = body.reply_markup.unwrap() else {
            panic!("markup")
        };
        assert_eq!(markup.rows.len(), 2);
        let tl::enums::KeyboardInlineButtonRow::Row(row) = &markup.rows[1];
        let tl::enums::KeyboardInlineButton::Button(button) = &row.buttons[0];
        assert_eq!(
            button.text,
            original::text(english, "inline_stuff", "btn_source")
        );
        let tl::enums::InlineButtonType::Url(url) = &button.r#type else {
            panic!("url")
        };
        assert_eq!(url.url, "https://git.break.wtf/xenside/ataraxia");
    }
}
#[test]
fn ping_uses_original_template_and_real_measurement_placeholders() {
    let config = presentation::PingConfig::default();
    assert_eq!(config.ping_emoji, "⛈️");
    let plain = presentation::ping_text(&config, 12.345, 90061, false, false);
    assert_eq!(
        plain,
        "<blockquote>⚡ <b>Ping:</b> 12.345</blockquote>\n<blockquote>🕓 <b>Uptime:</b> 1 day(s), 1:01:01</blockquote>"
    );
    assert!(!plain.contains("Pong"));
    assert!(!plain.contains('🦀'));
    let premium = presentation::ping_text(&config, 1., 1, true, false);
    assert!(premium.contains("5920515922505765329"));
    assert!(premium.contains("5900104897885376843"));
}
#[test]
fn ping_template_values_are_substituted_once_and_unknown_fields_are_visible_errors() {
    let mut config = presentation::PingConfig {
        custom_message: "{me} {{literal}} {ping_hint}".into(),
        hint: Some("{ping}".into()),
        ..Default::default()
    };
    assert_eq!(
        presentation::ping_render(
            &config,
            1.,
            0,
            false,
            true,
            &[("me", "Alice {uptime}".into())]
        ),
        "Alice {uptime} {literal} {ping}"
    );
    config.custom_message = "{unknown}".into();
    assert_eq!(presentation::ping_text(&config, 1., 1, false, false), "🚫");
}
#[test]
fn info_and_updater_read_the_original_strings() {
    let ui = atarustia::update_ui::Ui { english: false };
    let screen = ui.restart();
    assert!(!screen.text.contains("atarustia"));
    assert!(screen.text.contains("<b>ataraxia</b>"));
    let screen = ui.completed(false, &"a".repeat(40), 1500, 14, &[], None);
    assert!(!screen.text.contains("{}"));
    assert!(!screen.text.contains("atarustia"));
    for english in [true, false] {
        let ui = atarustia::help::Ui {
            english,
            premium: false,
        };
        assert_eq!(
            ui.text("settings", &[]),
            original::text(english, "ataraxia_info", "settings_btn")
        );
        assert_eq!(
            ui.text("close", &[]),
            original::text(english, "$inline", "screen_close")
        );
    }
}
