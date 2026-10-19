use atarustia::{
    forms::{Action, Forms},
    update_ui::{self, Ui},
    updater::{Changes, Commit, Report},
};
use grammers_client::{message::InputMessage, tl, update::Article};
fn report() -> Report {
    Report {
        branch: "main<&>".into(),
        current: "a".repeat(40),
        incoming: "b".repeat(40),
        commits: String::new(),
        diff: String::new(),
        changes: Changes {
            commits: vec![Commit {
                hash: "c".repeat(40),
                subject: "fix <b> & {branch}".into(),
            }],
            total: 17,
            files: 3,
            plus: 15,
            minus: 7,
            date: 1700000000,
        },
    }
}
fn content(input: InputMessage) -> tl::types::InputBotInlineMessageText {
    let tl::enums::InputBotInlineMessage::Text(text) = Article::new("test", input).raw.send_message
    else {
        panic!()
    };
    text
}
#[test]
fn original_card_layout_styles_and_pinned_actions_are_preserved() {
    let ui = Ui { english: false };
    let card = ui.card(&report(), false);
    card.validate().unwrap();
    assert!(card.text.contains("Доступно обновление"));
    assert!(card.text.contains("main&lt;&amp;&gt;"));
    assert!(card.text.contains("+15 −7"));
    assert!(card.text.contains("<blockquote expandable>"));
    assert!(card.text.contains("fix &lt;b&gt; &amp; {branch}"));
    assert!(card.text.contains("…и ещё 16"));
    assert!(card.text.contains("14.11.2023 22:13 UTC"));
    let parsed = content(update_ui::user_body(&card, false).unwrap());
    assert!(parsed.entities.unwrap().iter().any(|e|matches!(e,tl::enums::MessageEntity::FormattedDate(date) if date.relative&&date.date==1700000000)));
    assert_eq!(card.buttons[0][0].text, "Обновить");
    assert_eq!(card.buttons[0][1].text, "Изменения");
    assert_eq!(card.buttons[1][0].text, "Отмена");
    assert!(!card.close_button);
    assert!(
        matches!(&card.buttons[0][0].action,Some(Action::Update{current,incoming,retry:false}) if current==&"a".repeat(40)&&incoming==&"b".repeat(40))
    );
    assert!(
        card.buttons[0][1]
            .url
            .as_ref()
            .unwrap()
            .contains("/compare/")
    );
    let mut forms = Forms::default();
    let (_, body) = forms.create(card, 123, vec![]).unwrap();
    let text = content(body);
    let tl::enums::ReplyMarkup::ReplyInlineMarkup(keyboard) = text.reply_markup.unwrap() else {
        panic!()
    };
    assert_eq!(keyboard.rows.len(), 2);
    let tl::enums::KeyboardInlineButtonRow::Row(row) = &keyboard.rows[0];
    let tl::enums::KeyboardInlineButton::Button(button) = &row.buttons[0];
    let tl::enums::KeyboardButtonStyle::Style(style) = button.style.as_ref().unwrap();
    assert!(style.bg_success);
    assert_eq!(style.icon, Some(5807601906603792598));
    let tl::enums::InlineButtonType::Callback(callback) = &button.r#type else {
        panic!()
    };
    let token = std::str::from_utf8(&callback.data).unwrap();
    assert!(token.starts_with("f:"));
    assert!(!token.contains(&"b".repeat(40)));
    assert!(forms.click(token, 123).is_ok());
    assert!(forms.click(token, 999).is_err());
    assert!(text.no_webpage);
}
#[test]
fn progress_retry_restart_and_locales_have_original_states() {
    for english in [false, true] {
        let ui = Ui { english };
        let progress = ui.progress(&[(0, 1250)], 1);
        progress.validate().unwrap();
        assert!(progress.text.contains("✅"));
        assert!(progress.text.contains("⌛️"));
        assert!(progress.text.contains("⚪️"));
        assert!(progress.text.contains("1.2"));
        assert!(progress.buttons.is_empty());
        let failed = ui.failed(
            "unsafe <file> & token",
            Some((&"a".repeat(40), &"b".repeat(40))),
        );
        failed.validate().unwrap();
        assert!(failed.text.contains("&lt;file&gt; &amp;"));
        assert!(matches!(
            failed.buttons[0][0].action,
            Some(Action::Update { retry: true, .. })
        ));
        assert_eq!(
            failed.buttons[0][0].text,
            if english { "Retry" } else { "Ещё раз" }
        );
        ui.restart().validate().unwrap();
        assert!(matches!(
            ui.restart().buttons[0][0].action,
            Some(Action::Restart)
        ));
        assert!(ui.restarting().buttons.is_empty());
        let finished = ui.completed(
            true,
            &"b".repeat(40),
            2500,
            3,
            &["bad<&>".into()],
            Some(&report().changes),
        );
        assert!(finished.text.contains("bad&lt;&amp;&gt;"));
        assert!(finished.text.contains("<blockquote expandable>"));
        assert!(finished.buttons.is_empty());
        finished.validate().unwrap();
    }
    let ui = Ui { english: false };
    assert_eq!(ui.plural(1, "commits"), "1 коммит");
    assert_eq!(ui.plural(2, "commits"), "2 коммита");
    assert_eq!(ui.plural(11, "commits"), "11 коммитов");
    assert_eq!(ui.plural(21, "commits"), "21 коммит");
    assert_eq!(ui.plural(114, "commits"), "114 коммитов");
}
#[test]
fn module_forms_cannot_request_core_update_and_replaced_tokens_expire() {
    let mut forms = Forms::default();
    let card = Ui { english: true }.card(&report(), false);
    assert!(card.restrict_module("evil").is_err());
    let (id, body) = forms.create(card, 123, vec![]).unwrap();
    let text = content(body);
    let tl::enums::ReplyMarkup::ReplyInlineMarkup(k) = text.reply_markup.unwrap() else {
        panic!()
    };
    let tl::enums::KeyboardInlineButtonRow::Row(row) = &k.rows[0];
    let tl::enums::KeyboardInlineButton::Button(b) = &row.buttons[0];
    let tl::enums::InlineButtonType::Callback(c) = &b.r#type else {
        panic!()
    };
    let token = String::from_utf8(c.data.clone()).unwrap();
    forms
        .replace(&id, Ui { english: true }.progress(&[], 0))
        .unwrap();
    assert!(forms.click(&token, 123).is_err());
    let notification = Ui { english: true }.card(&report(), true);
    assert_eq!(notification.buttons[1][0].text, "Later");
    assert_eq!(notification.ttl_seconds, 86400);
}
#[test]
fn long_commit_subjects_do_not_split_html_or_exceed_entity_limit() {
    let mut r = report();
    r.changes.commits = (0..15)
        .map(|_| Commit {
            hash: "c".repeat(40),
            subject: "🦀<&>".repeat(100),
        })
        .collect();
    let card = Ui { english: true }.card(&r, false);
    assert_eq!(card.pages.len(), 1);
    card.validate().unwrap();
    let mut forms = Forms::default();
    let (_, input) = forms.create(card, 123, vec![]).unwrap();
    let body = content(input);
    assert!(body.message.encode_utf16().count() <= 4096);
    assert_eq!(body.message.matches("ccccccc").count(), 15);
}
#[test]
fn updater_settings_are_strict_and_default_notifications_on() {
    assert!(update_ui::Config::default().update_notifications);
    assert!(
        serde_json::from_value::<update_ui::Config>(
            serde_json::json!({"update_notifications":"maybe"})
        )
        .is_err()
    );
}
#[test]
fn premium_logo_and_emojis_are_native_entities_with_plain_alt_fallback() {
    let ui = Ui { english: true };
    let screen = ui.restarting();
    let premium = content(update_ui::user_body(&screen, true).unwrap());
    let plain = content(update_ui::user_body(&screen, false).unwrap());
    assert!(plain.message.contains("atarustia"));
    assert!(!premium.message.contains("atarustia-emoji:"));
    let entities = premium.entities.unwrap();
    let ids = entities
        .iter()
        .filter_map(|e| {
            if let tl::enums::MessageEntity::CustomEmoji(e) = e {
                Some(e.document_id)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(
        ids,
        vec![
            5404651863054260920,
            5404474175962258931,
            5404590359122581779,
            5404537174542560251,
            5807900819147724161
        ]
    );
    assert!(
        plain
            .entities
            .unwrap()
            .iter()
            .all(|e| !matches!(e, tl::enums::MessageEntity::CustomEmoji(_)))
    );
    for e in entities {
        if let tl::enums::MessageEntity::CustomEmoji(e) = e {
            let units = premium.message.encode_utf16().collect::<Vec<_>>();
            assert!(
                !String::from_utf16(&units[e.offset as usize..(e.offset + e.length) as usize])
                    .unwrap()
                    .is_empty()
            );
        }
    }
}
#[test]
fn restart_reference_and_identity_survive_database_reopen_atomically() {
    use atarustia::{
        database::Database,
        update_flow::{self, Pending, Target},
    };
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open(dir.path(), 123).unwrap();
    let peer = grammers_session::types::PeerId::user(123)
        .unwrap()
        .to_ambient_ref();
    let inline = atarustia::file_id::encode_inline_id(
        &tl::types::InputBotInlineMessageId {
            dc_id: 2,
            id: 1,
            access_hash: 7,
        }
        .into(),
    );
    for target in [
        Target::User { peer, id: 5 },
        Target::Bot { peer, id: 6 },
        Target::Inline { id: inline },
    ] {
        let pending = Pending {
            target,
            update: true,
            started: 123,
            modules: vec!["counter".into()],
            from: Some("a".repeat(40)),
            expected: Some("b".repeat(40)),
            changes: Some(report().changes),
        };
        update_flow::remember(&db, &pending).unwrap();
        let reopened = Database::open(dir.path(), 123).unwrap();
        assert_eq!(reopened.get("RustRuntime", "restart"), true);
        assert_eq!(
            serde_json::to_value(update_flow::pending(&reopened).unwrap().unwrap()).unwrap(),
            serde_json::to_value(&pending).unwrap()
        );
    }
    let before = db.get("RustUpdater", "pending");
    let bad = Pending {
        target: Target::User { peer, id: 1 },
        update: true,
        started: 123,
        modules: vec![],
        from: Some("a".repeat(40)),
        expected: Some("bad".into()),
        changes: None,
    };
    assert!(update_flow::remember(&db, &bad).is_err());
    assert_eq!(before, db.get("RustUpdater", "pending"));
}
