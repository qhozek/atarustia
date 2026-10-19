//! Asynchronous permission checks with a short-lived, bounded participant cache.
use crate::{
    database::Database,
    security::{self, Permission},
};
use anyhow::Result;
use grammers_client::{Client, message::Message};
use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, Default)]
pub struct Rights {
    pub creator: bool,
    pub admin: bool,
    pub member: bool,
    pub add_admins: bool,
    pub change_info: bool,
    pub ban_users: bool,
    pub delete_messages: bool,
    pub pin_messages: bool,
    pub invite_users: bool,
}
impl Rights {
    pub fn permits(self, p: Permission) -> bool {
        if p.validate().is_err() {
            return false;
        }
        if let Permission::Mask { bits } = p {
            return [
                Permission::GroupOwner,
                Permission::GroupAdmin,
                Permission::GroupMember,
                Permission::GroupAdminAddAdmins,
                Permission::GroupAdminChangeInfo,
                Permission::GroupAdminBanUsers,
                Permission::GroupAdminDeleteMessages,
                Permission::GroupAdminPinMessages,
                Permission::GroupAdminInviteUsers,
            ]
            .into_iter()
            .any(|role| bits & role.bits() != 0 && self.permits(role));
        }
        if self.creator {
            return true;
        }
        match p {
            Permission::GroupOwner => false,
            Permission::GroupAdmin => self.admin,
            Permission::GroupMember => self.member,
            Permission::GroupAdminAddAdmins => self.admin && self.add_admins,
            Permission::GroupAdminChangeInfo => self.admin && self.change_info,
            Permission::GroupAdminBanUsers => self.admin && self.ban_users,
            Permission::GroupAdminDeleteMessages => self.admin && self.delete_messages,
            Permission::GroupAdminPinMessages => self.admin && self.pin_messages,
            Permission::GroupAdminInviteUsers => self.admin && self.invite_users,
            _ => false,
        }
    }
}
#[derive(Default)]
pub struct AccessCache(Mutex<HashMap<(i64, i64), (Instant, Rights)>>);
impl AccessCache {
    #[allow(clippy::too_many_arguments)] // Carries both transport and explicit security context, never inferred globals.
    pub async fn check(
        &self,
        client: &Client,
        db: &Database,
        owner: i64,
        message: &Message,
        command: &str,
        module: Option<&str>,
        permission: Permission,
    ) -> Result<bool> {
        let permission = security::effective(db, permission);
        let Some(user) = message
            .sender_id()
            .filter(|id| id.kind() == grammers_session::types::PeerKind::User)
        else {
            return Ok(false);
        };
        let user_id = user.bare_id_unchecked();
        let chat = message.peer_id().bot_api_dialog_id_unchecked();
        if security::allowed(db, owner, Some(user_id), chat, command, permission) {
            return Ok(true);
        }
        if security::owner_only(command) {
            return Ok(false);
        }
        if security::targeted(db, user_id, chat, command, module, false) {
            return Ok(true);
        }
        if chat >= 0 || !permission.needs_admin() {
            return Ok(false);
        }
        let cached = self
            .0
            .lock()
            .unwrap()
            .get(&(chat, user_id))
            .copied()
            .filter(|(at, _)| at.elapsed() < Duration::from_secs(30));
        let rights = if let Some((_, r)) = cached {
            r
        } else {
            let peer = crate::telegram::peer(message).await?;
            let participant = tokio::time::timeout(
                Duration::from_secs(10),
                client.get_permissions(peer, user.to_ambient_ref()),
            )
            .await;
            let Ok(Ok(participant)) = participant else {
                return Ok(false);
            };
            let rights = Rights {
                creator: participant.is_creator(),
                admin: participant.is_admin(),
                member: !participant.is_banned() && !participant.has_left(),
                add_admins: participant.can_add_admins(),
                ..Default::default()
            };
            let rights = if message.peer_id().kind() == grammers_session::types::PeerKind::Channel
                && rights.admin
            {
                use grammers_client::tl;
                let result = tokio::time::timeout(
                    Duration::from_secs(10),
                    client.invoke(&tl::functions::channels::GetParticipant {
                        channel: peer.into(),
                        participant: user.to_ambient_ref().into(),
                    }),
                )
                .await;
                match result {
                    Ok(Ok(tl::enums::channels::ChannelParticipant::Participant(result))) => {
                        match result.participant {
                            tl::enums::ChannelParticipant::Admin(p) => {
                                let tl::enums::ChatAdminRights::Rights(r) = p.admin_rights;
                                Rights {
                                    add_admins: r.add_admins,
                                    change_info: r.change_info,
                                    ban_users: r.ban_users,
                                    delete_messages: r.delete_messages,
                                    pin_messages: r.pin_messages,
                                    invite_users: r.invite_users,
                                    ..rights
                                }
                            }
                            _ => rights,
                        }
                    }
                    _ => rights,
                }
            } else {
                rights
            };
            let mut cache = self.0.lock().unwrap();
            cache.retain(|_, (at, _)| at.elapsed() < Duration::from_secs(30));
            if cache.len() >= 2048 {
                cache.clear();
            }
            cache.insert((chat, user_id), (Instant::now(), rights));
            rights
        };
        Ok(rights.permits(permission))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn admin_bits_do_not_imply_other_rights() {
        let r = Rights {
            admin: true,
            pin_messages: true,
            ..Default::default()
        };
        assert!(r.permits(Permission::GroupAdminPinMessages));
        assert!(!r.permits(Permission::GroupAdminBanUsers));
        assert!(!r.permits(Permission::GroupOwner));
        assert!(
            Rights {
                creator: true,
                ..Default::default()
            }
            .permits(Permission::GroupAdminBanUsers)
        );
    }
    #[test]
    fn bitmap_uses_union_without_granting_unrelated_rights() {
        let rights = Rights {
            admin: true,
            pin_messages: true,
            ..Default::default()
        };
        assert!(
            rights.permits(
                Permission::parse("group_admin_pin_messages|group_admin_ban_users").unwrap()
            )
        );
        assert!(!rights.permits(Permission::parse("group_owner|group_admin_ban_users").unwrap()));
        assert!(!rights.permits(Permission::Mask { bits: 1 << 25 }));
    }
}
