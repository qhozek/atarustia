use atarustia::{
    botapi::{ChatMember, MemberStatus, User},
    moderation::{self, ChatPermissions, LeaveRequest},
};
use grammers_client::tl::{self, Deserializable, Serializable};
use grammers_session::types::PeerRef;

fn channel() -> PeerRef {
    tl::types::InputPeerChannel {
        channel_id: 123,
        access_hash: 456,
    }
    .into()
}
fn user_ref() -> PeerRef {
    tl::types::InputPeerUser {
        user_id: 9,
        access_hash: 654,
    }
    .into()
}
fn user() -> User {
    User {
        id: 9,
        is_bot: false,
        first_name: "Test".into(),
        last_name: None,
        username: None,
        ..Default::default()
    }
}
fn admin() -> tl::enums::ChatAdminRights {
    tl::types::ChatAdminRights {
        change_info: false,
        post_messages: false,
        edit_messages: false,
        delete_messages: true,
        ban_users: false,
        invite_users: false,
        pin_messages: true,
        add_admins: false,
        anonymous: false,
        manage_call: false,
        other: true,
        manage_topics: false,
        post_stories: false,
        edit_stories: false,
        delete_stories: false,
        manage_direct_messages: false,
        manage_ranks: false,
        manage_linked_peers: false,
        manage_welcome_messages: false,
    }
    .into()
}
#[test]
fn ban_and_unban_wire_round_trip_preserves_targets_and_hashes() {
    for until in [None, Some(0), Some(1800000000)] {
        let request = moderation::edit_request(
            channel(),
            user_ref(),
            moderation::ban_rights(until).unwrap(),
        )
        .unwrap();
        let bytes = request.to_bytes();
        assert_eq!(
            &bytes[..4],
            &<tl::functions::channels::EditBanned as tl::Identifiable>::CONSTRUCTOR_ID
                .to_le_bytes()
        );
        // Generated request deserializers read the bare fields; to_bytes includes the constructor.
        let mut cursor = tl::deserialize::Cursor::from_slice(&bytes[4..]);
        let decoded = tl::functions::channels::EditBanned::deserialize(&mut cursor).unwrap();
        let tl::enums::InputChannel::Channel(chat) = decoded.channel else {
            panic!("wrong chat");
        };
        assert_eq!((chat.channel_id, chat.access_hash), (123, 456));
        let tl::enums::InputPeer::User(user) = decoded.participant else {
            panic!("wrong user");
        };
        assert_eq!((user.user_id, user.access_hash), (9, 654));
        let tl::enums::ChatBannedRights::Rights(rights) = decoded.banned_rights;
        assert!(rights.view_messages);
        assert!(!rights.send_messages);
        assert_eq!(i64::from(rights.until_date), until.unwrap_or(0));
    }
    let tl::enums::ChatBannedRights::Rights(rights) = moderation::unrestricted();
    assert!(
        !rights.view_messages && !rights.send_messages && !rights.send_media && !rights.send_photos
    );
    assert!(
        !rights.send_inline
            && !rights.embed_links
            && !rights.manage_topics
            && !rights.send_reactions
    );
    assert_eq!(rights.until_date, 0);
}
#[test]
fn absent_permissions_restrict_but_do_not_ban_viewing() {
    let permissions: ChatPermissions = serde_json::from_str("{}").unwrap();
    let tl::enums::ChatBannedRights::Rights(rights) = permissions.to_banned_rights(None).unwrap();
    assert!(!rights.view_messages);
    assert!(rights.send_messages && rights.send_media && rights.send_inline && rights.pin_messages);
    assert!(!rights.send_photos && !rights.send_docs && !rights.manage_topics);
    assert!(serde_json::from_str::<ChatPermissions>(r#"{"can_send_messages":"false"}"#).is_err());
    assert!(serde_json::from_str::<ChatPermissions>(r#"{"typo":true}"#).is_err());
}
#[test]
fn media_permissions_follow_original_or_semantics() {
    for key in [
        "can_send_media_messages",
        "can_send_audios",
        "can_send_documents",
        "can_send_photos",
        "can_send_videos",
        "can_send_video_notes",
        "can_send_voice_notes",
    ] {
        let permissions: ChatPermissions =
            serde_json::from_value(serde_json::json!({key:true})).unwrap();
        let tl::enums::ChatBannedRights::Rights(rights) =
            permissions.to_banned_rights(Some(1800000000)).unwrap();
        assert!(!rights.send_media, "{key}");
        assert!(rights.send_messages && rights.send_stickers);
        assert!(!rights.send_photos && !rights.send_videos);
        assert_eq!(rights.until_date, 1800000000);
    }
    let permissions = ChatPermissions {
        can_send_messages: true,
        can_send_other_messages: true,
        can_add_web_page_previews: true,
        can_send_polls: true,
        can_change_info: true,
        can_invite_users: true,
        can_pin_messages: true,
        ..Default::default()
    };
    let tl::enums::ChatBannedRights::Rights(rights) = permissions.to_banned_rights(None).unwrap();
    assert!(
        !rights.send_messages
            && !rights.send_stickers
            && !rights.send_gifs
            && !rights.send_games
            && !rights.send_inline
    );
    assert!(
        !rights.embed_links
            && !rights.send_polls
            && !rights.change_info
            && !rights.invite_users
            && !rights.pin_messages
    );
}
#[test]
fn invalid_targets_dates_and_private_leave_fail_before_rpc() {
    let group: PeerRef = tl::types::InputPeerChat { chat_id: 4 }.into();
    assert!(moderation::edit_request(group, user_ref(), moderation::unrestricted()).is_err());
    assert!(moderation::edit_request(user_ref(), user_ref(), moderation::unrestricted()).is_err());
    assert!(moderation::edit_request(channel(), channel(), moderation::unrestricted()).is_err());
    assert!(moderation::leave_request(user_ref()).is_err());
    for date in [-1, i64::from(i32::MAX) + 1, i64::MAX] {
        assert!(moderation::ban_rights(Some(date)).is_err());
        assert!(
            ChatPermissions::default()
                .to_banned_rights(Some(date))
                .is_err()
        );
    }
}
#[test]
fn leave_uses_channel_or_group_self_without_history_revocation() {
    let LeaveRequest::Channel(request) = moderation::leave_request(channel()).unwrap() else {
        panic!();
    };
    let tl::enums::InputChannel::Channel(chat) = request.channel else {
        panic!();
    };
    assert_eq!(chat.channel_id, 123);
    let group: PeerRef = tl::types::InputPeerChat { chat_id: 4 }.into();
    let LeaveRequest::Group(request) = moderation::leave_request(group).unwrap() else {
        panic!();
    };
    let bytes = request.to_bytes();
    assert_eq!(
        &bytes[..4],
        &<tl::functions::messages::DeleteChatUser as tl::Identifiable>::CONSTRUCTOR_ID
            .to_le_bytes()
    );
    let mut cursor = tl::deserialize::Cursor::from_slice(&bytes[4..]);
    let decoded = tl::functions::messages::DeleteChatUser::deserialize(&mut cursor).unwrap();
    assert_eq!(decoded.chat_id, 4);
    assert!(!decoded.revoke_history);
    assert!(matches!(decoded.user_id, tl::enums::InputUser::UserSelf));
}
#[test]
fn typed_channel_members_cover_all_statuses_and_preserve_rights() {
    let variants: Vec<(tl::enums::ChannelParticipant, MemberStatus)> = vec![
        (
            tl::types::ChannelParticipant {
                user_id: 9,
                date: 1,
                subscription_until_date: Some(5),
                rank: None,
            }
            .into(),
            MemberStatus::Member,
        ),
        (
            tl::types::ChannelParticipantSelf {
                via_request: true,
                user_id: 9,
                inviter_id: 1,
                date: 2,
                subscription_until_date: None,
                rank: None,
            }
            .into(),
            MemberStatus::Member,
        ),
        (
            tl::types::ChannelParticipantCreator {
                user_id: 9,
                admin_rights: admin(),
                rank: Some("Owner".into()),
            }
            .into(),
            MemberStatus::Creator,
        ),
        (
            tl::types::ChannelParticipantAdmin {
                can_edit: true,
                is_self: false,
                user_id: 9,
                inviter_id: None,
                promoted_by: 1,
                date: 2,
                admin_rights: admin(),
                rank: Some("Helper".into()),
            }
            .into(),
            MemberStatus::Administrator,
        ),
        (
            tl::types::ChannelParticipantBanned {
                left: false,
                peer: tl::types::PeerUser { user_id: 9 }.into(),
                kicked_by: 1,
                date: 2,
                banned_rights: ChatPermissions::default().to_banned_rights(None).unwrap(),
                rank: None,
            }
            .into(),
            MemberStatus::Restricted,
        ),
        (
            tl::types::ChannelParticipantBanned {
                left: true,
                peer: tl::types::PeerUser { user_id: 9 }.into(),
                kicked_by: 1,
                date: 2,
                banned_rights: moderation::ban_rights(None).unwrap(),
                rank: None,
            }
            .into(),
            MemberStatus::Kicked,
        ),
        (
            tl::types::ChannelParticipantLeft {
                peer: tl::types::PeerUser { user_id: 9 }.into(),
            }
            .into(),
            MemberStatus::Left,
        ),
    ];
    for (participant, status) in variants {
        let member = ChatMember::from_channel(user(), &participant).unwrap();
        assert_eq!(member.status, status);
        let round: ChatMember =
            serde_json::from_value(serde_json::to_value(&member).unwrap()).unwrap();
        assert_eq!(round.status, status);
        if status == MemberStatus::Administrator {
            assert_eq!(member.details["rights"]["can_pin_messages"], true);
            assert_eq!(member.details["rights"]["can_restrict_members"], false);
        }
        let mut wrong = user();
        wrong.id = 10;
        assert!(ChatMember::from_channel(wrong, &participant).is_err());
    }
}
#[test]
fn typed_basic_group_members_preserve_identity_and_status() {
    for (participant, status) in [
        (
            tl::types::ChatParticipant {
                user_id: 9,
                inviter_id: 1,
                date: 2,
                rank: None,
            }
            .into(),
            MemberStatus::Member,
        ),
        (
            tl::types::ChatParticipantAdmin {
                user_id: 9,
                inviter_id: 1,
                date: 2,
                rank: None,
            }
            .into(),
            MemberStatus::Administrator,
        ),
        (
            tl::types::ChatParticipantCreator {
                user_id: 9,
                rank: None,
            }
            .into(),
            MemberStatus::Creator,
        ),
    ] {
        assert_eq!(
            ChatMember::from_group(user(), &participant).unwrap().status,
            status
        );
        let mut wrong = user();
        wrong.id = 10;
        assert!(ChatMember::from_group(wrong, &participant).is_err());
    }
    let peer = tl::types::ChannelParticipantLeft {
        peer: tl::types::PeerChannel { channel_id: 9 }.into(),
    }
    .into();
    assert!(ChatMember::from_channel(user(), &peer).is_err());
}

#[test]
fn banned_rights_match_original_python_wire_vectors() {
    let vectors: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("fixtures/python_moderation.json")).unwrap();
    for vector in vectors {
        let name = vector["name"].as_str().unwrap();
        let until = vector["until_date"].as_i64();
        let rights = match name {
            "ban" => moderation::ban_rights(until).unwrap(),
            "unban" => moderation::unrestricted(),
            _ => serde_json::from_value::<ChatPermissions>(vector["permissions"].clone())
                .unwrap()
                .to_banned_rights(until)
                .unwrap(),
        };
        assert_eq!(
            hex::encode(rights.to_bytes()),
            vector["hex"].as_str().unwrap(),
            "{name}"
        );
    }
}
