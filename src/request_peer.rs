//! Optional false is distinct from an absent request-peer constraint.
use anyhow::{Result, ensure};
use grammers_client::tl;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AdminRights {
    pub change_info: bool,
    pub post_messages: bool,
    pub edit_messages: bool,
    pub delete_messages: bool,
    pub ban_users: bool,
    pub invite_users: bool,
    pub pin_messages: bool,
    pub add_admins: bool,
    pub anonymous: bool,
    pub manage_call: bool,
    pub other: bool,
    pub manage_topics: bool,
    pub post_stories: bool,
    pub edit_stories: bool,
    pub delete_stories: bool,
    pub manage_direct_messages: bool,
    pub manage_ranks: bool,
    pub manage_linked_peers: bool,
    pub manage_welcome_messages: bool,
}
impl AdminRights {
    pub fn to_tl(&self) -> tl::enums::ChatAdminRights {
        tl::types::ChatAdminRights {
            change_info: self.change_info,
            post_messages: self.post_messages,
            edit_messages: self.edit_messages,
            delete_messages: self.delete_messages,
            ban_users: self.ban_users,
            invite_users: self.invite_users,
            pin_messages: self.pin_messages,
            add_admins: self.add_admins,
            anonymous: self.anonymous,
            manage_call: self.manage_call,
            other: self.other,
            manage_topics: self.manage_topics,
            post_stories: self.post_stories,
            edit_stories: self.edit_stories,
            delete_stories: self.delete_stories,
            manage_direct_messages: self.manage_direct_messages,
            manage_ranks: self.manage_ranks,
            manage_linked_peers: self.manage_linked_peers,
            manage_welcome_messages: self.manage_welcome_messages,
        }
        .into()
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Criteria {
    User {
        #[serde(default)]
        bot: Option<bool>,
        #[serde(default)]
        premium: Option<bool>,
    },
    Group {
        #[serde(default)]
        creator: bool,
        #[serde(default)]
        bot_participant: bool,
        #[serde(default)]
        has_username: Option<bool>,
        #[serde(default)]
        forum: Option<bool>,
        #[serde(default)]
        user_admin_rights: Option<AdminRights>,
        #[serde(default)]
        bot_admin_rights: Option<AdminRights>,
    },
    Channel {
        #[serde(default)]
        creator: bool,
        #[serde(default)]
        has_username: Option<bool>,
        #[serde(default)]
        user_admin_rights: Option<AdminRights>,
        #[serde(default)]
        bot_admin_rights: Option<AdminRights>,
    },
    CreateBot {
        #[serde(default)]
        bot_managed: bool,
        #[serde(default)]
        suggested_name: Option<String>,
        #[serde(default)]
        suggested_username: Option<String>,
    },
}
impl Criteria {
    pub fn to_tl(&self) -> Result<tl::enums::RequestPeerType> {
        Ok(match self {
            Self::User { bot, premium } => tl::types::RequestPeerTypeUser {
                bot: *bot,
                premium: *premium,
            }
            .into(),
            Self::Group {
                creator,
                bot_participant,
                has_username,
                forum,
                user_admin_rights,
                bot_admin_rights,
            } => tl::types::RequestPeerTypeChat {
                creator: *creator,
                bot_participant: *bot_participant,
                has_username: *has_username,
                forum: *forum,
                user_admin_rights: user_admin_rights.as_ref().map(AdminRights::to_tl),
                bot_admin_rights: bot_admin_rights.as_ref().map(AdminRights::to_tl),
            }
            .into(),
            Self::Channel {
                creator,
                has_username,
                user_admin_rights,
                bot_admin_rights,
            } => tl::types::RequestPeerTypeBroadcast {
                creator: *creator,
                has_username: *has_username,
                user_admin_rights: user_admin_rights.as_ref().map(AdminRights::to_tl),
                bot_admin_rights: bot_admin_rights.as_ref().map(AdminRights::to_tl),
            }
            .into(),
            Self::CreateBot {
                bot_managed,
                suggested_name,
                suggested_username,
            } => {
                ensure!(
                    suggested_name
                        .as_ref()
                        .is_none_or(|v| !v.is_empty() && v.encode_utf16().count() <= 64),
                    "invalid suggested bot name"
                );
                ensure!(
                    suggested_username
                        .as_ref()
                        .is_none_or(|v| (5..=32).contains(&v.len())
                            && v.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')),
                    "invalid suggested bot username"
                );
                tl::types::RequestPeerTypeCreateBot {
                    bot_managed: *bot_managed,
                    suggested_name: suggested_name.clone(),
                    suggested_username: suggested_username.clone(),
                }
                .into()
            }
        })
    }
}
pub fn from_tl(v: &tl::enums::RequestPeerType) -> serde_json::Value {
    use serde_json::json;
    match v {
        tl::enums::RequestPeerType::User(v) => {
            json!({"type":"user","bot":v.bot,"premium":v.premium})
        }
        tl::enums::RequestPeerType::Chat(v) => {
            json!({"type":"group","creator":v.creator,"bot_participant":v.bot_participant,"has_username":v.has_username,"forum":v.forum,"user_admin_rights":v.user_admin_rights.as_ref().map(crate::bot_events::admin_rights),"bot_admin_rights":v.bot_admin_rights.as_ref().map(crate::bot_events::admin_rights)})
        }
        tl::enums::RequestPeerType::Broadcast(v) => {
            json!({"type":"channel","creator":v.creator,"has_username":v.has_username,"user_admin_rights":v.user_admin_rights.as_ref().map(crate::bot_events::admin_rights),"bot_admin_rights":v.bot_admin_rights.as_ref().map(crate::bot_events::admin_rights)})
        }
        tl::enums::RequestPeerType::CreateBot(v) => {
            json!({"type":"create_bot","bot_managed":v.bot_managed,"suggested_name":v.suggested_name,"suggested_username":v.suggested_username})
        }
    }
}
