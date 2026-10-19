use atarustia::{gift_payload, rich_payload, service_payload, story_payload};
use grammers_client::tl::{self, Deserializable, Serializable};
use serde_json::{Value, json};

fn plain(s: &str) -> tl::enums::RichText {
    tl::types::TextPlain { text: s.into() }.into()
}
fn caption() -> tl::enums::PageCaption {
    tl::types::PageCaption {
        text: plain("Caption"),
        credit: tl::enums::RichText::TextEmpty,
    }
    .into()
}
fn coordinates() -> tl::enums::MediaAreaCoordinates {
    tl::types::MediaAreaCoordinates {
        x: 1.,
        y: 2.,
        w: 3.,
        h: 4.,
        rotation: 5.,
        radius: Some(6.),
    }
    .into()
}
fn unique() -> tl::enums::StarGift {
    tl::types::StarGiftUnique {
        require_premium: true,
        resale_ton_only: false,
        theme_available: true,
        burned: false,
        crafted: true,
        id: 9007199254740993,
        gift_id: 123,
        title: "Unique".into(),
        slug: "unique-1".into(),
        num: 1,
        owner_id: Some(tl::types::PeerUser { user_id: 9 }.into()),
        owner_name: None,
        owner_address: Some("synthetic-wallet".into()),
        attributes: vec![
            tl::types::StarGiftAttributeBackdrop {
                name: "Blue".into(),
                backdrop_id: 1,
                center_color: 2,
                edge_color: 3,
                pattern_color: 4,
                text_color: 5,
                rarity: tl::enums::StarGiftAttributeRarity::Rare,
            }
            .into(),
            tl::types::StarGiftAttributeOriginalDetails {
                sender_id: None,
                recipient_id: tl::types::PeerUser { user_id: 9 }.into(),
                date: 123,
                message: None,
            }
            .into(),
        ],
        availability_issued: 1,
        availability_total: 100,
        gift_address: None,
        resell_amount: Some(vec![
            tl::types::StarsAmount {
                amount: 9007199254740993,
                nanos: 123,
            }
            .into(),
            tl::types::StarsTonAmount { amount: 17 }.into(),
        ]),
        released_by: None,
        value_amount: None,
        value_currency: None,
        value_usd_amount: None,
        theme_peer: None,
        peer_color: None,
        host_id: None,
        offer_min_stars: Some(5),
        craft_chance_permille: Some(10),
    }
    .into()
}
#[test]
fn original_python_extended_wire_vectors_decode_and_preserve_fields() {
    let fixtures: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/python_extended_payloads.json")).unwrap();
    for fixture in fixtures {
        let bytes = hex::decode(fixture["hex"].as_str().unwrap()).unwrap();
        let mut c = tl::deserialize::Cursor::from_slice(&bytes);
        match fixture["name"].as_str().unwrap() {
            "instant_view" => {
                let p = tl::enums::Page::deserialize(&mut c).unwrap();
                assert_eq!(p.to_bytes(), bytes);
                let p = rich_payload::page(&p);
                assert_eq!(p["views"], 7);
                assert_eq!(p["blocks"][0]["type"], "embed");
                assert_eq!(p["blocks"][1]["articles"][0]["title"], "Next");
                assert_eq!(p["partial"], false);
            }
            "story_area" => {
                let a = tl::enums::MediaArea::deserialize(&mut c).unwrap();
                assert_eq!(a.to_bytes(), bytes);
                let p = story_payload::area(&a);
                assert_eq!(p["type"], "suggested_reaction");
                assert_eq!(p["coordinates"]["rotation"], 5.);
                assert_eq!(p["dark"], true);
            }
            "privacy" => {
                let p = tl::enums::PrivacyRule::deserialize(&mut c).unwrap();
                assert_eq!(p.to_bytes(), bytes);
                assert_eq!(story_payload::privacy(&p)["user_ids"], json!([9, 10]));
            }
            "suggested_post" => {
                let a = tl::enums::MessageAction::deserialize(&mut c).unwrap();
                assert_eq!(a.to_bytes(), bytes);
                let p = service_payload::action(&a, None);
                assert_eq!(p["rejected"], true);
                assert_eq!(p["price"]["amount"], "9007199254740993");
                assert_eq!(p["price"]["nanos"], 123);
            }
            "gift" => {
                let g = tl::enums::StarGift::deserialize(&mut c).unwrap();
                assert_eq!(g.to_bytes(), bytes);
                let p = gift_payload::gift(&g);
                assert_eq!(p["title"], "Gift");
                assert_eq!(p["stars"], "50");
                assert_eq!(p["convert_stars"], "40");
            }
            _ => panic!(),
        }
    }
}
#[test]
fn unique_gift_attributes_prices_and_identity_keep_precision() {
    let g = gift_payload::gift(&unique());
    assert_eq!(g["id"], "9007199254740993");
    assert_eq!(g["attributes"][0]["rarity"]["type"], "rare");
    assert_eq!(g["attributes"][1]["recipient_id"], 9);
    assert_eq!(g["resell_amount"][0]["amount"], "9007199254740993");
    assert_eq!(g["resell_amount"][1]["currency"], "TON");
    assert_eq!(g["owner_address"], "synthetic-wallet");
    assert!(g.get("access_hash").is_none());
    let action = tl::types::MessageActionStarGiftUnique {
        upgrade: false,
        transferred: true,
        saved: true,
        refunded: false,
        prepaid_upgrade: false,
        assigned: false,
        from_offer: false,
        craft: true,
        name_hidden: false,
        gift: unique(),
        can_export_at: Some(123),
        transfer_stars: Some(50),
        from_id: None,
        peer: None,
        saved_id: Some(9007199254740993),
        resale_amount: None,
        can_transfer_at: None,
        can_resell_at: None,
        drop_original_details_stars: None,
        can_craft_at: None,
        message: None,
    }
    .into();
    let p = service_payload::action(&action, None);
    assert_eq!(p["type"], "unique_star_gift");
    assert_eq!(p["saved_id"], "9007199254740993");
    assert_eq!(p["gift"]["attributes"][0]["name"], "Blue");
}
#[test]
fn every_privacy_constructor_is_structured_and_list_bounds_mark_partial() {
    use tl::enums::PrivacyRule as P;
    for (rule, name) in [
        (P::PrivacyValueAllowContacts, "allow_contacts"),
        (P::PrivacyValueAllowAll, "allow_all"),
        (P::PrivacyValueDisallowContacts, "disallow_contacts"),
        (P::PrivacyValueDisallowAll, "disallow_all"),
        (P::PrivacyValueAllowCloseFriends, "allow_close_friends"),
        (P::PrivacyValueAllowPremium, "allow_premium"),
        (P::PrivacyValueAllowBots, "allow_bots"),
        (P::PrivacyValueDisallowBots, "disallow_bots"),
    ] {
        assert_eq!(story_payload::privacy(&rule)["type"], name);
    }
    let rule = tl::types::PrivacyValueAllowUsers {
        users: (1..=101).collect(),
    }
    .into();
    let p = story_payload::privacy(&rule);
    assert_eq!(p["user_ids"].as_array().unwrap().len(), 100);
    assert_eq!(p["partial"], true);
    let rule = tl::types::PrivacyValueDisallowChatParticipants { chats: vec![123] }.into();
    assert_eq!(story_payload::privacy(&rule)["chat_ids"], json!([123]));
}
#[test]
fn story_weather_url_post_and_reaction_statistics_are_preserved() {
    let weather = tl::types::MediaAreaWeather {
        coordinates: coordinates(),
        emoji: "sun".into(),
        temperature_c: 12.5,
        color: 123,
    }
    .into();
    assert_eq!(story_payload::area(&weather)["temperature_c"], 12.5);
    let url = tl::types::MediaAreaUrl {
        coordinates: coordinates(),
        url: "https://example.com".into(),
    }
    .into();
    assert_eq!(story_payload::area(&url)["url"], "https://example.com");
    let post = tl::types::MediaAreaChannelPost {
        coordinates: coordinates(),
        channel_id: 123,
        msg_id: 42,
    }
    .into();
    assert_eq!(story_payload::area(&post)["chat_id"], -1000000000123i64);
    let views = tl::types::StoryViews {
        has_viewers: true,
        views_count: 10,
        forwards_count: Some(2),
        reactions: Some(vec![
            tl::types::ReactionCount {
                chosen_order: Some(0),
                reaction: tl::types::ReactionCustomEmoji {
                    document_id: 9007199254740993,
                }
                .into(),
                count: 3,
            }
            .into(),
        ]),
        reactions_count: Some(3),
        recent_viewers: Some(vec![9]),
    }
    .into();
    let p = story_payload::views(&views);
    assert_eq!(
        p["reactions"][0]["reaction"]["custom_emoji_id"],
        "9007199254740993"
    );
    assert_eq!(p["partial"], false);
}
#[test]
fn remaining_page_blocks_are_not_generic_unsupported() {
    let rich = tl::types::RichMessage {
        rtl: false,
        part: false,
        photos: vec![],
        documents: vec![],
        blocks: vec![
            tl::types::PageBlockEmbed {
                full_width: true,
                allow_scrolling: true,
                url: Some("https://example.com".into()),
                html: Some("<b>raw</b>".into()),
                poster_photo_id: Some(123),
                w: Some(100),
                h: Some(200),
                caption: caption(),
            }
            .into(),
            tl::types::PageBlockEmbedPost {
                url: "https://example.com".into(),
                webpage_id: 123,
                author_photo_id: 456,
                author: "Author".into(),
                date: 1,
                blocks: vec![tl::enums::PageBlock::Divider],
                caption: caption(),
            }
            .into(),
            tl::types::PageBlockRelatedArticles {
                title: plain("Related"),
                articles: vec![
                    tl::types::PageRelatedArticle {
                        url: "https://example.com".into(),
                        webpage_id: 123,
                        title: Some("Next".into()),
                        description: None,
                        photo_id: None,
                        author: None,
                        published_date: None,
                    }
                    .into(),
                ],
            }
            .into(),
            tl::types::PageBlockChannel {
                channel: tl::types::ChatForbidden {
                    id: 123,
                    title: "Group".into(),
                }
                .into(),
            }
            .into(),
            tl::types::InputPageBlockMap {
                geo: tl::types::InputGeoPoint {
                    lat: 1.,
                    long: 2.,
                    accuracy_radius: None,
                }
                .into(),
                zoom: 3,
                w: 100,
                h: 200,
                caption: caption(),
            }
            .into(),
        ],
    }
    .into();
    let p = rich_payload::from_tl(&rich);
    assert_eq!(p["partial"], false);
    for (i, kind) in [
        "embed",
        "embed_post",
        "related_articles",
        "channel",
        "input_map",
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(p["blocks"][i]["type"], kind);
    }
    assert_eq!(p["blocks"][0]["html"], "<b>raw</b>");
}
#[test]
fn web_attributes_theme_wallpaper_and_gift_are_structured() {
    let wallpaper = tl::types::WallPaperNoFile {
        id: 123,
        default: true,
        dark: false,
        settings: Some(
            tl::types::WallPaperSettings {
                blur: true,
                motion: false,
                background_color: Some(123),
                second_background_color: None,
                third_background_color: None,
                fourth_background_color: None,
                intensity: Some(50),
                rotation: None,
                emoticon: None,
            }
            .into(),
        ),
    }
    .into();
    let theme = tl::types::ThemeSettings {
        message_colors_animated: false,
        base_theme: tl::enums::BaseTheme::Night,
        accent_color: 123,
        outbox_accent_color: None,
        message_colors: Some(vec![1, 2]),
        wallpaper: Some(wallpaper),
    }
    .into();
    let attr = tl::types::WebPageAttributeTheme {
        documents: None,
        settings: Some(theme),
    }
    .into();
    assert_eq!(
        gift_payload::web_attribute(&attr)["settings"]["wallpaper"]["settings"]["blur"],
        true
    );
    let attr = tl::types::WebPageAttributeUniqueStarGift { gift: unique() }.into();
    assert_eq!(
        gift_payload::web_attribute(&attr)["gift"]["id"],
        "9007199254740993"
    );
    let attr = tl::types::WebPageAttributeAiComposeTone { emoji_id: 123 }.into();
    assert_eq!(gift_payload::web_attribute(&attr)["emoji_id"], "123");
}

#[test]
fn request_peer_keyboard_preserves_false_constraints_admin_rights_and_shared_peers() {
    use atarustia::{
        markup::{Appearance, Keyboard, ReplyKey, ReplyKeyKind},
        request_peer::{AdminRights, Criteria},
    };
    let criteria = Criteria::Group {
        creator: false,
        bot_participant: true,
        has_username: Some(false),
        forum: Some(false),
        user_admin_rights: Some(AdminRights {
            pin_messages: true,
            ..Default::default()
        }),
        bot_admin_rights: None,
    };
    let keyboard = Keyboard::Reply {
        rows: vec![vec![ReplyKey {
            text: "Choose".into(),
            appearance: Appearance::default(),
            kind: ReplyKeyKind::RequestPeer {
                button_id: 7,
                criteria: Box::new(criteria),
                max_quantity: 2,
                name_requested: true,
                username_requested: false,
                photo_requested: true,
            },
        }]],
        resize: false,
        one_time: true,
        selective: false,
        persistent: false,
        placeholder: None,
    };
    let raw = keyboard.to_tl().unwrap();
    let bytes = raw.to_bytes();
    let mut c = tl::deserialize::Cursor::from_slice(&bytes);
    assert_eq!(
        tl::enums::ReplyMarkup::deserialize(&mut c)
            .unwrap()
            .to_bytes(),
        bytes
    );
    let p = atarustia::keyboard_payload::from_tl(&raw);
    assert_eq!(p["rows"][0][0]["kind"]["criteria"]["forum"], false);
    assert_eq!(
        p["rows"][0][0]["kind"]["criteria"]["user_admin_rights"]["can_pin_messages"],
        true
    );
    assert_eq!(p["partial"], false);
    for criteria in [
        Criteria::User {
            bot: Some(false),
            premium: Some(true),
        },
        Criteria::Channel {
            creator: true,
            has_username: None,
            user_admin_rights: None,
            bot_admin_rights: None,
        },
        Criteria::CreateBot {
            bot_managed: true,
            suggested_name: Some("Name".into()),
            suggested_username: Some("TestBot".into()),
        },
    ] {
        assert!(criteria.to_tl().is_ok());
    }
    let a = tl::types::MessageActionRequestedPeerSentMe {
        button_id: 7,
        peers: vec![
            tl::types::RequestedPeerUser {
                user_id: 9,
                first_name: Some("Name".into()),
                last_name: None,
                username: None,
                photo: None,
            }
            .into(),
        ],
    }
    .into();
    let p = service_payload::action(&a, None);
    assert_eq!(p["button_id"], 7);
    assert_eq!(p["peers"][0]["first_name"], "Name");
    assert!(
        Criteria::CreateBot {
            bot_managed: false,
            suggested_name: None,
            suggested_username: Some("!bad".into())
        }
        .to_tl()
        .is_err()
    );
}

#[test]
fn story_privacy_areas_and_reactions_no_longer_force_partial() {
    let story = tl::types::StoryItem {
        pinned: false,
        public: false,
        close_friends: true,
        min: false,
        noforwards: false,
        edited: false,
        contacts: false,
        selected_contacts: false,
        out: false,
        id: 1,
        date: 2,
        from_id: None,
        fwd_from: None,
        expire_date: 3,
        caption: None,
        entities: None,
        media: tl::enums::MessageMedia::Empty,
        media_areas: Some(vec![
            tl::types::MediaAreaUrl {
                coordinates: coordinates(),
                url: "https://example.com".into(),
            }
            .into(),
        ]),
        privacy: Some(vec![
            tl::types::PrivacyValueAllowUsers { users: vec![9] }.into(),
        ]),
        views: Some(
            tl::types::StoryViews {
                has_viewers: true,
                views_count: 1,
                forwards_count: None,
                reactions: None,
                reactions_count: None,
                recent_viewers: Some(vec![9]),
            }
            .into(),
        ),
        sent_reaction: Some(
            tl::types::ReactionEmoji {
                emoticon: "ok".into(),
            }
            .into(),
        ),
        albums: None,
        music: None,
    }
    .into();
    let p = atarustia::rare_payload::story(&story, atarustia::message_payload::media);
    assert_eq!(p["partial"], false);
    assert_eq!(p["privacy"][0]["user_ids"], json!([9]));
    assert_eq!(p["sent_reaction"]["emoji"], "ok");
    assert_eq!(p["media_areas"][0]["url"], "https://example.com");
}

#[test]
fn webpage_cached_page_and_attributes_retain_nested_structure() {
    let page = tl::types::WebPage {
        has_large_media: false,
        video_cover_photo: false,
        id: 1,
        url: "https://example.com".into(),
        display_url: "example.com".into(),
        hash: 1,
        r#type: None,
        site_name: None,
        title: None,
        description: None,
        photo: None,
        embed_url: None,
        embed_type: None,
        embed_width: None,
        embed_height: None,
        duration: None,
        author: None,
        document: None,
        cached_page: Some(
            tl::types::Page {
                part: false,
                rtl: false,
                v2: true,
                url: "https://example.com".into(),
                blocks: vec![
                    tl::types::PageBlockParagraph {
                        text: plain("Cached"),
                    }
                    .into(),
                ],
                photos: vec![],
                documents: vec![],
                views: Some(1),
            }
            .into(),
        ),
        attributes: Some(vec![
            tl::types::WebPageAttributeUniqueStarGift { gift: unique() }.into(),
        ]),
    }
    .into();
    let p = atarustia::rare_payload::webpage(&page);
    assert_eq!(p["partial"], false);
    assert_eq!(p["cached_page"]["blocks"][0]["text"]["text"], "Cached");
    assert_eq!(p["attributes"][0]["gift"]["slug"], "unique-1");
}

#[test]
fn theme_ownership_and_passport_metadata_are_explicit_without_secret_export() {
    let theme = tl::types::MessageActionSetChatTheme {
        theme: tl::types::ChatTheme {
            emoticon: "night".into(),
        }
        .into(),
    }
    .into();
    assert_eq!(
        service_payload::action(&theme, None)["theme"]["emoticon"],
        "night"
    );
    let owner = tl::types::MessageActionChangeCreator { new_creator_id: 9 }.into();
    assert_eq!(service_payload::action(&owner, None)["user_id"], 9);
    let protected = tl::types::MessageActionNoForwardsToggle {
        prev_value: true,
        new_value: false,
    }
    .into();
    assert_eq!(service_payload::action(&protected, None)["current"], false);
    let passport = tl::types::MessageActionSecureValuesSent {
        types: vec![
            tl::enums::SecureValueType::Passport,
            tl::enums::SecureValueType::Email,
        ],
    }
    .into();
    assert_eq!(
        service_payload::action(&passport, None)["value_types"],
        json!(["passport", "email"])
    );
    let offer = tl::types::MessageActionStarGiftPurchaseOffer {
        accepted: true,
        declined: false,
        gift: unique(),
        price: tl::types::StarsAmount {
            amount: 50,
            nanos: 0,
        }
        .into(),
        expires_at: 123,
    }
    .into();
    assert_eq!(service_payload::action(&offer, None)["accepted"], true);
    let success = tl::types::MessageActionSuggestedPostSuccess {
        price: tl::types::StarsTonAmount {
            amount: 9007199254740993,
        }
        .into(),
    }
    .into();
    assert_eq!(
        service_payload::action(&success, None)["price"]["amount"],
        "9007199254740993"
    );
}

fn story_with(media: tl::enums::MessageMedia) -> tl::enums::StoryItem {
    tl::types::StoryItem {
        pinned: false,
        public: true,
        close_friends: false,
        min: false,
        noforwards: false,
        edited: false,
        contacts: false,
        selected_contacts: false,
        out: false,
        id: 1,
        date: 2,
        from_id: None,
        fwd_from: None,
        expire_date: 3,
        caption: Some("Nested".into()),
        entities: None,
        media,
        media_areas: None,
        privacy: None,
        views: None,
        sent_reaction: None,
        albums: None,
        music: None,
    }
    .into()
}
fn web_with(story: tl::enums::StoryItem) -> tl::enums::MessageMedia {
    tl::types::MessageMediaWebPage {
        force_large_media: false,
        force_small_media: false,
        manual: false,
        safe: true,
        webpage: tl::types::WebPage {
            has_large_media: false,
            video_cover_photo: false,
            id: 1,
            url: "https://example.com".into(),
            display_url: "example.com".into(),
            hash: 1,
            r#type: None,
            site_name: None,
            title: None,
            description: None,
            photo: None,
            embed_url: None,
            embed_type: None,
            embed_width: None,
            embed_height: None,
            duration: None,
            author: None,
            document: None,
            cached_page: None,
            attributes: Some(vec![
                tl::types::WebPageAttributeStory {
                    peer: tl::types::PeerUser { user_id: 9 }.into(),
                    id: 1,
                    story: Some(story),
                }
                .into(),
            ]),
        }
        .into(),
    }
    .into()
}
#[test]
fn webpage_story_attribute_uses_shared_media_depth_budget() {
    let media = web_with(story_with(tl::enums::MessageMedia::Empty));
    let p = atarustia::message_payload::media(&media);
    assert_eq!(p["page"]["attributes"][0]["story"]["caption"], "Nested");
    assert_eq!(p["partial"], false);
    let mut nested = tl::enums::MessageMedia::Empty;
    for _ in 0..8 {
        nested = web_with(story_with(nested));
    }
    let p = atarustia::message_payload::media(&nested);
    assert!(rich_payload::has_partial(&p));
    assert!(
        serde_json::to_string(&p)
            .unwrap()
            .contains("media_depth_limit")
    );
}

#[test]
fn request_peer_quantity_and_optional_false_are_validated_before_sending() {
    use atarustia::{
        markup::{Appearance, Keyboard, ReplyKey, ReplyKeyKind},
        request_peer::Criteria,
    };
    for max_quantity in [0, 11] {
        let keyboard = Keyboard::Reply {
            rows: vec![vec![ReplyKey {
                text: "Select".into(),
                appearance: Appearance::default(),
                kind: ReplyKeyKind::RequestPeer {
                    button_id: 7,
                    criteria: Box::new(Criteria::User {
                        bot: Some(false),
                        premium: None,
                    }),
                    max_quantity,
                    name_requested: false,
                    username_requested: false,
                    photo_requested: false,
                },
            }]],
            resize: false,
            one_time: false,
            selective: false,
            persistent: false,
            placeholder: None,
        };
        assert!(keyboard.to_tl().is_err());
    }
    let criterion = Criteria::User {
        bot: Some(false),
        premium: None,
    }
    .to_tl()
    .unwrap();
    let value = atarustia::request_peer::from_tl(&criterion);
    assert_eq!(value["bot"], false);
    assert!(value["premium"].is_null());
}
