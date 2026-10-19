//! Pure request construction for the bot facade. Telegram enforces administrator permissions.
use anyhow::{Result, ensure};
use grammers_client::tl;
use grammers_session::types::{PeerKind, PeerRef};
use serde::{Deserialize, Serialize};

/// Original facade semantics: absent/false fields revoke a capability. Media fields
/// are OR-combined into the legacy send_media flag, not independent per-media grants.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ChatPermissions {
    pub can_send_messages: bool,
    pub can_send_media_messages: bool,
    pub can_send_audios: bool,
    pub can_send_documents: bool,
    pub can_send_photos: bool,
    pub can_send_videos: bool,
    pub can_send_video_notes: bool,
    pub can_send_voice_notes: bool,
    pub can_send_other_messages: bool,
    pub can_add_web_page_previews: bool,
    pub can_send_polls: bool,
    pub can_change_info: bool,
    pub can_invite_users: bool,
    pub can_pin_messages: bool,
}
impl ChatPermissions {
    pub fn to_banned_rights(&self, until_date: Option<i64>) -> Result<tl::enums::ChatBannedRights> {
        let tl::enums::ChatBannedRights::Rights(mut rights) = unrestricted();
        rights.until_date = timestamp(until_date)?;
        rights.send_messages = !self.can_send_messages;
        rights.send_media = !(self.can_send_media_messages
            || self.can_send_audios
            || self.can_send_documents
            || self.can_send_photos
            || self.can_send_videos
            || self.can_send_video_notes
            || self.can_send_voice_notes);
        rights.send_stickers = !self.can_send_other_messages;
        rights.send_gifs = !self.can_send_other_messages;
        rights.send_games = !self.can_send_other_messages;
        rights.send_inline = !self.can_send_other_messages;
        rights.embed_links = !self.can_add_web_page_previews;
        rights.send_polls = !self.can_send_polls;
        rights.change_info = !self.can_change_info;
        rights.invite_users = !self.can_invite_users;
        rights.pin_messages = !self.can_pin_messages;
        Ok(rights.into())
    }
}
fn timestamp(value: Option<i64>) -> Result<i32> {
    let value = value.unwrap_or(0);
    ensure!(
        (0..=i64::from(i32::MAX)).contains(&value),
        "until_date must be a nonnegative Telegram Unix timestamp (i32)"
    );
    Ok(value as i32)
}
pub fn unrestricted() -> tl::enums::ChatBannedRights {
    tl::types::ChatBannedRights {
        view_messages: false,
        send_messages: false,
        send_media: false,
        send_stickers: false,
        send_gifs: false,
        send_games: false,
        send_inline: false,
        embed_links: false,
        send_polls: false,
        change_info: false,
        invite_users: false,
        pin_messages: false,
        manage_topics: false,
        send_photos: false,
        send_videos: false,
        send_roundvideos: false,
        send_audios: false,
        send_voices: false,
        send_docs: false,
        send_plain: false,
        edit_rank: false,
        send_reactions: false,
        manage_linked_peers: false,
        until_date: 0,
    }
    .into()
}
pub fn ban_rights(until_date: Option<i64>) -> Result<tl::enums::ChatBannedRights> {
    let tl::enums::ChatBannedRights::Rights(mut rights) = unrestricted();
    rights.view_messages = true;
    rights.until_date = timestamp(until_date)?;
    Ok(rights.into())
}
pub fn edit_request(
    chat: PeerRef,
    user: PeerRef,
    rights: tl::enums::ChatBannedRights,
) -> Result<tl::functions::channels::EditBanned> {
    ensure!(
        chat.id.kind() == PeerKind::Channel,
        "member restrictions require a channel or supergroup"
    );
    ensure!(
        user.id.kind() == PeerKind::User,
        "member target must be a user"
    );
    Ok(tl::functions::channels::EditBanned {
        channel: chat.into(),
        participant: user.into(),
        banned_rights: rights,
    })
}
#[derive(Debug)]
pub enum LeaveRequest {
    Channel(tl::functions::channels::LeaveChannel),
    Group(tl::functions::messages::DeleteChatUser),
}
pub fn leave_request(chat: PeerRef) -> Result<LeaveRequest> {
    match chat.id.kind() {
        PeerKind::Channel => Ok(LeaveRequest::Channel(
            tl::functions::channels::LeaveChannel {
                channel: chat.into(),
            },
        )),
        PeerKind::Chat => Ok(LeaveRequest::Group(
            tl::functions::messages::DeleteChatUser {
                revoke_history: false,
                chat_id: chat.id.bare_id_unchecked(),
                user_id: tl::enums::InputUser::UserSelf,
            },
        )),
        _ => anyhow::bail!(
            "leave_chat requires a group, supergroup or channel, not a private dialog"
        ),
    }
}
