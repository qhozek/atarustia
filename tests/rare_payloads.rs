use atarustia::{keyboard_payload, markup, message_payload, rare_payload};
use grammers_client::tl;
use serde_json::json;
fn text(value: &str) -> tl::enums::TextWithEntities {
    tl::types::TextWithEntities {
        text: value.into(),
        entities: vec![],
    }
    .into()
}

#[test]
fn webpage_states_and_loaded_metadata_are_structured() {
    for (page, state) in [
        (tl::types::WebPageEmpty { id: 9, url: None }.into(), "empty"),
        (
            tl::types::WebPagePending {
                id: 9,
                url: Some("https://example.com".into()),
                date: 10,
            }
            .into(),
            "pending",
        ),
        (
            tl::types::WebPageNotModified {
                cached_page_views: Some(5),
            }
            .into(),
            "not_modified",
        ),
    ] {
        assert_eq!(rare_payload::webpage(&page)["state"], state);
    }
    let page = tl::types::WebPage {
        has_large_media: true,
        video_cover_photo: false,
        id: 9007199254740993,
        url: "https://example.com".into(),
        display_url: "example.com".into(),
        hash: 1,
        r#type: Some("article".into()),
        site_name: Some("Site".into()),
        title: Some("Title".into()),
        description: Some("Body".into()),
        photo: None,
        embed_url: None,
        embed_type: None,
        embed_width: None,
        embed_height: None,
        duration: None,
        author: Some("Author".into()),
        document: None,
        cached_page: None,
        attributes: None,
    }
    .into();
    let media = tl::types::MessageMediaWebPage {
        force_large_media: true,
        force_small_media: false,
        manual: true,
        safe: true,
        webpage: page,
    }
    .into();
    let result = message_payload::media(&media);
    assert_eq!(result["page"]["id"], "9007199254740993");
    assert_eq!(result["page"]["title"], "Title");
    assert_eq!(result["partial"], false);
}
#[test]
fn giveaway_and_checklist_preserve_metadata_and_completions() {
    let giveaway = tl::types::MessageMediaGiveaway {
        only_new_subscribers: true,
        winners_are_visible: true,
        channels: vec![123],
        countries_iso2: Some(vec!["RU".into()]),
        prize_description: Some("Prize".into()),
        quantity: 5,
        months: Some(3),
        stars: None,
        until_date: 123,
    }
    .into();
    let payload = message_payload::media(&giveaway);
    assert_eq!(payload["chat_ids"], json!([-1000000000123i64]));
    assert_eq!(payload["winner_count"], 5);
    let results = tl::types::MessageMediaGiveawayResults {
        only_new_subscribers: false,
        refunded: true,
        channel_id: 123,
        additional_peers_count: Some(2),
        launch_msg_id: 42,
        winners_count: 2,
        unclaimed_count: 0,
        winners: vec![9, 10],
        months: None,
        stars: Some(50),
        prize_description: None,
        until_date: 123,
    }
    .into();
    assert_eq!(
        message_payload::media(&results)["winner_user_ids"],
        json!([9, 10])
    );
    let checklist = tl::types::MessageMediaToDo {
        todo: tl::types::TodoList {
            others_can_append: true,
            others_can_complete: false,
            title: text("Tasks"),
            list: vec![
                tl::types::TodoItem {
                    id: 1,
                    title: text("First"),
                }
                .into(),
            ],
        }
        .into(),
        completions: Some(vec![
            tl::types::TodoCompletion {
                id: 1,
                completed_by: tl::types::PeerUser { user_id: 9 }.into(),
                date: 123,
            }
            .into(),
        ]),
    }
    .into();
    let payload = message_payload::media(&checklist);
    assert_eq!(payload["tasks"][0]["title"]["text"], "First");
    assert_eq!(payload["completions"][0]["completed_by"], 9);
}
#[test]
fn stories_preserve_deleted_skipped_loaded_and_nested_depth_bounds() {
    for (story, state) in [
        (tl::types::StoryItemDeleted { id: 1 }.into(), "deleted"),
        (
            tl::types::StoryItemSkipped {
                close_friends: true,
                live: false,
                id: 1,
                date: 2,
                expire_date: 3,
            }
            .into(),
            "skipped",
        ),
    ] {
        assert_eq!(
            rare_payload::story(&story, |_| panic!("no media"))["state"],
            state
        );
    }
    let story = tl::types::StoryItem {
        pinned: true,
        public: true,
        close_friends: false,
        min: false,
        noforwards: true,
        edited: false,
        contacts: false,
        selected_contacts: false,
        out: false,
        id: 1,
        date: 2,
        from_id: None,
        fwd_from: None,
        expire_date: 3,
        caption: Some("Caption".into()),
        entities: None,
        media: tl::enums::MessageMedia::Empty,
        media_areas: None,
        privacy: None,
        views: None,
        sent_reaction: None,
        albums: Some(vec![7]),
        music: None,
    }
    .into();
    let media = tl::types::MessageMediaStory {
        via_mention: true,
        peer: tl::types::PeerUser { user_id: 9 }.into(),
        id: 1,
        story: Some(story),
    }
    .into();
    let result = message_payload::media(&media);
    assert_eq!(result["story"]["caption"], "Caption");
    assert_eq!(result["partial"], false);
    let missing = tl::types::MessageMediaStory {
        via_mention: false,
        peer: tl::types::PeerUser { user_id: 9 }.into(),
        id: 1,
        story: None,
    }
    .into();
    assert_eq!(message_payload::media(&missing)["partial"], true);
}
#[test]
fn inline_keyboard_binary_callbacks_and_styles_survive_read_conversion() {
    let markup: tl::enums::ReplyMarkup = tl::types::ReplyInlineMarkup {
        force_reply: false,
        rows: vec![
            tl::types::KeyboardInlineButtonRow {
                buttons: vec![
                    tl::types::KeyboardInlineButton {
                        text: "Binary".into(),
                        r#type: tl::types::InlineButtonTypeCallback {
                            data: vec![0, 255, 1],
                            requires_password: true,
                        }
                        .into(),
                        style: Some(
                            tl::types::KeyboardButtonStyle {
                                bg_primary: false,
                                bg_success: true,
                                bg_danger: false,
                                icon: Some(9007199254740993),
                            }
                            .into(),
                        ),
                    }
                    .into(),
                ],
            }
            .into(),
        ],
    }
    .into();
    let result = keyboard_payload::from_tl(&markup);
    assert!(result["rows"][0][0]["kind"]["data"].is_null());
    assert_eq!(result["rows"][0][0]["kind"]["data_base64"], "AP8B");
    assert_eq!(
        result["rows"][0][0]["appearance"]["icon_custom_emoji_id"],
        "9007199254740993"
    );
    for kind in [
        markup::Kind::Game,
        markup::Kind::Pay,
        markup::Kind::Disabled,
        markup::Kind::UserProfile { user_id: 9 },
        markup::Kind::LoginUrl {
            url: "https://example.com/login".into(),
            forward_text: None,
            request_write_access: true,
        },
    ] {
        assert!(
            markup::Key {
                text: "Button".into(),
                kind,
                style: markup::Style::Primary,
                emoji_id: Some(123)
            }
            .button()
            .is_ok()
        );
    }
}
#[test]
fn reply_keyboard_variants_and_row_limits_are_explicit() {
    let markup = tl::types::ReplyKeyboardMarkup {
        resize: true,
        single_use: false,
        selective: false,
        persistent: true,
        force_reply: false,
        placeholder: Some("Type".into()),
        rows: vec![
            tl::types::KeyboardButtonRow {
                buttons: vec![
                    tl::types::KeyboardButton {
                        style: None,
                        text: "Phone".into(),
                        r#type: tl::enums::ButtonType::RequestPhone,
                    }
                    .into(),
                ],
            }
            .into(),
        ],
    }
    .into();
    assert_eq!(
        keyboard_payload::from_tl(&markup)["rows"][0][0]["kind"]["type"],
        "request_contact"
    );
    let hide = tl::types::ReplyKeyboardHide { selective: true }.into();
    assert_eq!(keyboard_payload::from_tl(&hide)["type"], "remove");
    let force = tl::types::ReplyKeyboardForceReply {
        single_use: true,
        selective: false,
        placeholder: None,
    }
    .into();
    assert_eq!(keyboard_payload::from_tl(&force)["one_time"], true);
}

#[test]
fn original_python_rare_media_and_rich_wire_are_decoded() {
    use grammers_client::tl::{Deserializable, Serializable};
    let fixtures: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("fixtures/python_rare_rich.json")).unwrap();
    for fixture in fixtures {
        let bytes = hex::decode(fixture["hex"].as_str().unwrap()).unwrap();
        let mut cursor = tl::deserialize::Cursor::from_slice(&bytes);
        match fixture["name"].as_str().unwrap() {
            "rich" => {
                let rich = tl::enums::RichMessage::deserialize(&mut cursor).unwrap();
                assert_eq!(rich.to_bytes(), bytes);
                let payload = atarustia::rich_payload::from_tl(&rich);
                assert_eq!(payload["blocks"][0]["text"]["type"], "bold");
                assert_eq!(payload["blocks"][0]["text"]["text"]["text"], "hello");
                assert_eq!(payload["partial"], false);
            }
            "rich_edit" => {
                let rich = atarustia::bot_edits::RichText::Html {
                    source: "<b>hello</b>".into(),
                };
                assert_eq!(rich.to_tl().unwrap().to_bytes(), bytes);
            }
            name => {
                let media = tl::enums::MessageMedia::deserialize(&mut cursor).unwrap();
                assert_eq!(media.to_bytes(), bytes);
                let payload = message_payload::media(&media);
                match name {
                    "webpage" => assert_eq!(payload["page"]["state"], "pending"),
                    "giveaway" => assert_eq!(payload["winner_count"], 5),
                    "story_deleted" => assert_eq!(payload["story"]["state"], "deleted"),
                    "checklist" => assert_eq!(payload["tasks"][0]["title"]["text"], "First"),
                    _ => panic!(),
                }
            }
        }
    }
}

#[test]
fn rare_updates_include_chosen_results_binary_callbacks_and_bot_stop() {
    let chosen = tl::types::UpdateBotInlineSend {
        user_id: 9,
        query: "query".into(),
        geo: None,
        id: "result".into(),
        msg_id: None,
    }
    .into();
    assert_eq!(
        atarustia::bot_events::raw_event(&chosen, 1).unwrap().name,
        "chosen_inline_result"
    );
    let stopped = tl::types::UpdateBotStopped {
        user_id: 9,
        date: 10,
        stopped: true,
        qts: 1,
    }
    .into();
    assert_eq!(
        atarustia::bot_events::raw_event(&stopped, 1).unwrap().data["stopped"],
        true
    );
    let callback = tl::types::UpdateBotCallbackQuery {
        query_id: 9007199254740993,
        user_id: 9,
        peer: tl::types::PeerUser { user_id: 9 }.into(),
        msg_id: 42,
        chat_instance: 9007199254740993,
        data: Some(vec![0, 255, 1]),
        game_short_name: None,
    }
    .into();
    let event = atarustia::bot_events::raw_event(&callback, 1).unwrap();
    assert_eq!(event.data["data_base64"], "AP8B");
    assert!(event.data["data"].is_null());
    assert_eq!(event.data["chat_instance"], "9007199254740993");
}

#[test]
fn rich_tree_depth_and_collection_limits_propagate_partial() {
    let mut text: tl::enums::RichText = tl::types::TextPlain { text: "x".into() }.into();
    for _ in 0..20 {
        text = tl::types::TextBold { text }.into();
    }
    let rich = tl::types::RichMessage {
        rtl: false,
        part: false,
        blocks: vec![tl::types::PageBlockParagraph { text }.into()],
        photos: vec![],
        documents: vec![],
    }
    .into();
    let payload = atarustia::rich_payload::from_tl(&rich);
    assert_eq!(payload["partial"], true);
    assert!(
        serde_json::to_string(&payload)
            .unwrap()
            .contains("truncated")
    );
    let blocks = vec![tl::enums::PageBlock::Divider; 101];
    let rich = tl::types::RichMessage {
        rtl: false,
        part: false,
        blocks,
        photos: vec![],
        documents: vec![],
    }
    .into();
    let payload = atarustia::rich_payload::from_tl(&rich);
    assert_eq!(payload["blocks"].as_array().unwrap().len(), 100);
    assert_eq!(payload["partial"], true);
}

#[test]
fn outbound_reply_keyboards_round_trip_and_unknown_json_fields_fail() {
    use grammers_client::tl::{Deserializable, Serializable};
    use markup::{Appearance, Keyboard, ReplyKey, ReplyKeyKind, Style};
    let keyboard = Keyboard::Reply {
        rows: vec![vec![
            ReplyKey {
                text: "Phone".into(),
                kind: ReplyKeyKind::RequestContact,
                appearance: Appearance {
                    style: Style::Success,
                    emoji_id: Some(123),
                },
            },
            ReplyKey {
                text: "Poll".into(),
                kind: ReplyKeyKind::RequestPoll { quiz: Some(false) },
                appearance: Appearance::default(),
            },
        ]],
        resize: true,
        one_time: false,
        selective: true,
        persistent: true,
        placeholder: Some("Reply".into()),
    };
    let raw = keyboard.to_tl().unwrap();
    let bytes = raw.to_bytes();
    let mut cursor = tl::deserialize::Cursor::from_slice(&bytes);
    assert_eq!(
        tl::enums::ReplyMarkup::deserialize(&mut cursor)
            .unwrap()
            .to_bytes(),
        bytes
    );
    let payload = keyboard_payload::from_tl(&raw);
    assert_eq!(payload["rows"][0][0]["kind"]["type"], "request_contact");
    assert_eq!(payload["rows"][0][0]["appearance"]["success"], true);
    assert_eq!(payload["rows"][0][1]["kind"]["quiz"], false);
    let round: Keyboard = serde_json::from_value(serde_json::to_value(&keyboard).unwrap()).unwrap();
    assert!(round.markup().is_ok());
    assert!(
        Keyboard::ForceReply {
            selective: false,
            one_time: true,
            placeholder: Some("x".repeat(65))
        }
        .to_tl()
        .is_err()
    );
    assert!(serde_json::from_value::<Keyboard>(json!({"type":"remove","typo":true})).is_err());
    let pay = markup::Key {
        text: "Pay".into(),
        kind: markup::Kind::Pay,
        style: Style::Default,
        emoji_id: None,
    };
    let disabled = markup::Key {
        kind: markup::Kind::Disabled,
        ..pay.clone()
    };
    assert!(markup::keyboard(&[vec![disabled, pay]]).is_err());
}

#[test]
fn formatted_date_diff_and_ton_entities_are_not_dropped() {
    use atarustia::botapi::MessageEntity;
    let date = tl::types::MessageEntityFormattedDate {
        relative: true,
        short_time: false,
        long_time: false,
        short_date: true,
        long_date: false,
        day_of_week: false,
        offset: 0,
        length: 2,
        date: 123,
    }
    .into();
    let date = MessageEntity::from_tl(&date).unwrap();
    assert_eq!(date.unix_time, Some(123));
    assert_eq!(date.extra.as_ref().unwrap()["relative"], true);
    let diff = tl::types::MessageEntityDiffReplace {
        offset: 0,
        length: 2,
        old_text: "old".into(),
    }
    .into();
    let diff = MessageEntity::from_tl(&diff).unwrap();
    assert_eq!(diff.extra.unwrap()["old_text"], "old");
    let ton = tl::types::MessageEntityTonAddress {
        offset: 0,
        length: 2,
    }
    .into();
    assert_eq!(MessageEntity::from_tl(&ton).unwrap().kind, "ton_address");
}

#[test]
fn checklist_and_paid_price_service_events_are_structured() {
    let complete = tl::types::MessageActionTodoCompletions {
        completed: vec![1],
        incompleted: vec![2],
    }
    .into();
    let payload = atarustia::service_payload::action(&complete, None);
    assert_eq!(payload["completed_task_ids"], json!([1]));
    assert_eq!(payload["incomplete_task_ids"], json!([2]));
    let append = tl::types::MessageActionTodoAppendTasks {
        list: vec![
            tl::types::TodoItem {
                id: 3,
                title: text("Third"),
            }
            .into(),
        ],
    }
    .into();
    assert_eq!(
        atarustia::service_payload::action(&append, None)["tasks"][0]["title"],
        "Third"
    );
    let price = tl::types::MessageActionPaidMessagesPrice {
        broadcast_messages_allowed: true,
        stars: 50,
    }
    .into();
    assert_eq!(
        atarustia::service_payload::action(&price, None)["star_count"],
        50
    );
    let refund = tl::types::MessageActionPaidMessagesRefunded {
        count: 2,
        stars: 100,
    }
    .into();
    assert_eq!(
        atarustia::service_payload::action(&refund, None)["message_count"],
        2
    );
}
