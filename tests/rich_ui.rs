use atarustia::{
    bot_edits::{self, RichText},
    forms::{Action, Forms, Key, Screen},
    markup::{Appearance, Style},
    rich::RichScreen,
};
use grammers_client::{
    message::InputMessage,
    tl::{self, Deserializable, Serializable},
};
use grammers_session::types::PeerRef;

fn peer() -> PeerRef {
    tl::types::InputPeerUser {
        user_id: 9,
        access_hash: 456,
    }
    .into()
}
fn inline_id() -> tl::enums::InputBotInlineMessageId {
    tl::types::InputBotInlineMessageId64 {
        dc_id: 2,
        owner_id: 123,
        id: 42,
        access_hash: 456,
    }
    .into()
}
fn key() -> Key {
    Key {
        text: "Close".into(),
        action: Some(Action::Close),
        url: None,
    }
}

#[test]
fn rich_edits_never_set_text_entities_or_media_in_parallel() {
    for rich in [
        RichText::Html {
            source: "<b>hello</b>".into(),
        },
        RichText::Markdown {
            source: "**hello**".into(),
        },
    ] {
        let request = bot_edits::inline(
            inline_id(),
            InputMessage::new().html("fallback <b>text</b>"),
            Some(&rich),
            None,
        )
        .unwrap();
        assert!(request.message.is_none() && request.entities.is_none() && request.media.is_none());
        assert!(request.rich_message.is_some());
        let bytes = request.to_bytes();
        let mut cursor = tl::deserialize::Cursor::from_slice(&bytes[4..]);
        let round =
            tl::functions::messages::EditInlineBotMessage::deserialize(&mut cursor).unwrap();
        assert!(round.message.is_none() && round.rich_message.is_some());
        let ordinary = bot_edits::rich(peer(), 42, &rich, None).unwrap();
        assert!(
            ordinary.message.is_none()
                && ordinary.entities.is_none()
                && ordinary.rich_message.is_some()
        );
    }
    assert!(
        bot_edits::inline(
            inline_id(),
            InputMessage::new(),
            Some(&RichText::Html { source: "x".into() }),
            Some(tl::enums::InputMedia::Empty)
        )
        .is_err()
    );
    assert!(
        RichText::Html {
            source: String::new()
        }
        .to_tl()
        .is_err()
    );
}
#[test]
fn caption_clear_and_keyboard_only_edit_have_distinct_wire_flags() {
    let caption = bot_edits::caption(peer(), 42, "", false, None).unwrap();
    assert_eq!(caption.message.as_deref(), Some(""));
    assert_eq!(caption.entities, Some(vec![]));
    let keyboard = bot_edits::keyboard(peer(), 42, &[]).unwrap();
    assert!(keyboard.message.is_none() && keyboard.entities.is_none());
    assert!(keyboard.reply_markup.is_some());
    let inline = bot_edits::inline_keyboard(inline_id(), &[]).unwrap();
    assert!(inline.message.is_none() && inline.entities.is_none() && inline.reply_markup.is_some());
    let inline = bot_edits::inline_caption(inline_id(), "", false, None).unwrap();
    assert_eq!(inline.message.as_deref(), Some(""));
    assert_eq!(inline.entities, Some(vec![]));
    let html = bot_edits::caption(peer(), 42, "<b>🦀hello</b>", true, None).unwrap();
    assert_eq!(html.message.as_deref(), Some("🦀hello"));
    assert!(matches!(
        html.entities.unwrap()[0],
        tl::enums::MessageEntity::Bold(_)
    ));
    assert!(bot_edits::caption(peer(), 0, "x", false, None).is_err());
    assert!(bot_edits::caption(peer(), 42, &"🦀".repeat(513), false, None).is_err());
}
#[test]
fn styled_rich_callbacks_keep_appearance_and_security_after_fallback() {
    let mut builder = RichScreen::new();
    builder.emoji(123, "★").unwrap();
    let marker = builder
        .styled_tap(
            key(),
            Appearance {
                style: Style::Danger,
                emoji_id: Some(456),
            },
        )
        .unwrap();
    builder.html(&marker, "Close");
    let mut forms = Forms::default();
    let (id, _) = forms.create(builder.finish(), 9, vec![]).unwrap();
    let rich = forms.rich_source(&id).unwrap();
    assert!(rich.contains("style=\"danger\""));
    assert!(rich.contains("emoji-id=\"456\""));
    let token = rich
        .split("data=\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap()
        .to_owned();
    assert!(forms.click(&token, 10).is_err());
    assert!(forms.click(&token, 9).is_ok());
    assert!(forms.plain_fallback(&id));
    let fallback = forms.render(&id, 0).unwrap();
    assert!(forms.click(&token, 9).is_err());
    let article = grammers_client::update::Article::new("fallback", fallback);
    let tl::enums::InputBotInlineMessage::Text(message) = article.raw.send_message else {
        panic!();
    };
    assert!(message.message.contains('★'));
    let Some(tl::enums::ReplyMarkup::ReplyInlineMarkup(markup)) = message.reply_markup else {
        panic!();
    };
    let tl::enums::KeyboardInlineButtonRow::Row(row) = &markup.rows[0];
    let tl::enums::KeyboardInlineButton::Button(button) = &row.buttons[0];
    let Some(tl::enums::KeyboardButtonStyle::Style(style)) = &button.style else {
        panic!();
    };
    assert!(style.bg_danger);
    assert_eq!(style.icon, Some(456));
}
#[test]
fn invalid_marker_and_appearance_shapes_are_rejected_before_registration() {
    for source in [
        "{{atarustia:tap:99}}",
        "{{atarustia:tap:no}}",
        "{{atarustia:tap:0",
    ] {
        assert!(
            Screen {
                rich_html: Some(source.into()),
                ..Default::default()
            }
            .validate()
            .is_err()
        );
    }
    assert!(
        Screen {
            rich_html: Some("<b>x</b>".into()),
            rich_buttons: vec![key()],
            ..Default::default()
        }
        .validate()
        .is_err()
    );
    assert!(
        Screen {
            buttons: vec![vec![key()]],
            button_appearance: vec![vec![]],
            ..Default::default()
        }
        .validate()
        .is_err()
    );
    assert!(
        Screen {
            buttons: vec![vec![key()]],
            button_appearance: vec![vec![Appearance {
                style: Style::Success,
                emoji_id: Some(-1)
            }]],
            ..Default::default()
        }
        .validate()
        .is_err()
    );
}

#[test]
fn rich_tree_buttons_tables_and_lists_preserve_style_and_structure() {
    let plain = || {
        tl::types::TextPlain {
            text: "hello".into(),
        }
        .into()
    };
    let button = tl::types::TextButton {
        text: plain(),
        r#type: tl::types::InlineButtonTypeCallback {
            data: b"callback".to_vec(),
            requires_password: false,
        }
        .into(),
        style: Some(
            tl::types::RichButtonStyle {
                bg_primary: false,
                bg_danger: true,
                bg_success: false,
                link: false,
            }
            .into(),
        ),
    }
    .into();
    let rich = tl::types::RichMessage {
        rtl: true,
        part: false,
        photos: vec![],
        documents: vec![],
        blocks: vec![
            tl::types::PageBlockParagraph { text: button }.into(),
            tl::types::PageBlockDetails {
                open: true,
                title: plain(),
                blocks: vec![
                    tl::types::PageBlockList {
                        items: vec![
                            tl::types::PageListItemText {
                                checkbox: true,
                                checked: false,
                                text: plain(),
                            }
                            .into(),
                        ],
                    }
                    .into(),
                ],
            }
            .into(),
            tl::types::PageBlockTable {
                bordered: true,
                striped: false,
                compact: true,
                title: plain(),
                rows: vec![
                    tl::types::PageTableRow {
                        cells: vec![
                            tl::types::PageTableCell {
                                header: true,
                                align_center: true,
                                align_right: false,
                                valign_middle: false,
                                valign_bottom: false,
                                text: Some(plain()),
                                colspan: Some(2),
                                rowspan: None,
                            }
                            .into(),
                        ],
                    }
                    .into(),
                ],
            }
            .into(),
        ],
    }
    .into();
    let payload = atarustia::rich_payload::from_tl(&rich);
    assert_eq!(payload["rtl"], true);
    assert_eq!(payload["partial"], false);
    assert_eq!(payload["blocks"][0]["text"]["style"]["danger"], true);
    assert_eq!(payload["blocks"][0]["text"]["kind"]["data"], "callback");
    assert_eq!(
        payload["blocks"][1]["blocks"][0]["items"][0]["checked"],
        false
    );
    assert_eq!(payload["blocks"][2]["rows"][0]["cells"][0]["colspan"], 2);
}

#[test]
fn builder_lists_links_code_and_custom_emoji_escape_untrusted_labels() {
    let mut builder = RichScreen::new();
    builder.list(&["<unsafe>".into()], true);
    builder
        .link("<label>", "https://example.com/?a=1&b=2")
        .unwrap()
        .code("<script>", "rust")
        .unwrap()
        .emoji(123, "<x>")
        .unwrap();
    let screen = builder.finish();
    let html = screen.rich_html.as_ref().unwrap();
    assert!(html.contains("&lt;unsafe&gt;"));
    assert!(html.contains("a=1&amp;b=2"));
    assert!(html.contains("&lt;script&gt;"));
    assert!(html.contains("&lt;x&gt;"));
    assert!(screen.text.contains("1. <unsafe>"));
    assert!(builder_bad_link().is_err());
    fn builder_bad_link() -> anyhow::Result<()> {
        RichScreen::new().link("x", "javascript:alert(1)")?;
        Ok(())
    }
}

#[test]
fn normal_keyboard_appearance_is_preserved_and_replaced_callbacks_expire() {
    let mut forms = Forms::default();
    let screen = Screen {
        text: "Styled".into(),
        buttons: vec![vec![key()]],
        button_appearance: vec![vec![Appearance {
            style: Style::Primary,
            emoji_id: Some(123),
        }]],
        ..Default::default()
    };
    let (id, message) = forms.create(screen, 9, vec![]).unwrap();
    let article = grammers_client::update::Article::new("styled", message);
    let tl::enums::InputBotInlineMessage::Text(content) = article.raw.send_message else {
        panic!();
    };
    let Some(tl::enums::ReplyMarkup::ReplyInlineMarkup(markup)) = content.reply_markup else {
        panic!();
    };
    let tl::enums::KeyboardInlineButtonRow::Row(row) = &markup.rows[0];
    let tl::enums::KeyboardInlineButton::Button(button) = &row.buttons[0];
    let Some(tl::enums::KeyboardButtonStyle::Style(style)) = &button.style else {
        panic!();
    };
    assert!(style.bg_primary);
    assert_eq!(style.icon, Some(123));
    let tl::enums::InlineButtonType::Callback(callback) = &button.r#type else {
        panic!();
    };
    let token = std::str::from_utf8(&callback.data).unwrap();
    assert!(forms.click(token, 9).is_ok());
    forms
        .replace(
            &id,
            Screen {
                text: "Replaced".into(),
                ..Default::default()
            },
        )
        .unwrap();
    assert!(forms.click(token, 9).is_err());
}
