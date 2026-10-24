//! Idempotent content forum setup; only reuse a chat owned by this account.
use crate::database::Database;
use anyhow::{Context as _, Result, ensure};
use grammers_client::{Client, peer::Peer, tl};
use grammers_session::types::PeerRef;

pub async fn setup(client: &Client, db: &Database) -> Result<i64> {
    let mut dialogs = client.iter_dialogs();
    let mut found = None;
    let saved = db.get("atarustia.forums", "channel_id").as_i64();
    while let Some(dialog) = dialogs.next().await? {
        if let Peer::Group(group) = dialog.peer()
            && let tl::enums::Chat::Channel(channel) = &group.raw
            && channel.creator
            && (saved == Some(channel.id) || channel.title == crate::original::FORUM_TITLE)
        {
            if found.is_none() || saved == Some(channel.id) {
                found = Some((
                    channel.id,
                    channel.access_hash.context("channel access hash missing")?,
                    channel.forum,
                    channel.title.clone(),
                ));
            }
            if saved == Some(channel.id) {
                break;
            }
        }
    }
    let created = found.is_none();
    let (id, hash, forum, title) = if let Some(found) = found {
        found
    } else {
        let updates = client
            .invoke(&tl::functions::channels::CreateChannel {
                broadcast: false,
                megagroup: true,
                for_import: false,
                forum: true,
                title: crate::original::FORUM_TITLE.into(),
                about: crate::original::FORUM_DESCRIPTION.into(),
                geo_point: None,
                address: None,
                ttl_period: None,
            })
            .await?;
        let chats = match updates {
            tl::enums::Updates::Updates(u) => u.chats,
            tl::enums::Updates::Combined(u) => u.chats,
            _ => Vec::new(),
        };
        chats
            .into_iter()
            .find_map(|c| {
                if let tl::enums::Chat::Channel(c) = c {
                    if c.creator {
                        Some((c.id, c.access_hash?, c.forum, c.title))
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .context("created channel not found in response")?
    };
    let channel = tl::enums::InputChannel::Channel(tl::types::InputChannel {
        channel_id: id,
        access_hash: hash,
    });
    let input = tl::enums::InputPeer::Channel(tl::types::InputPeerChannel {
        channel_id: id,
        access_hash: hash,
    });
    if title == "atarustia-userbot" {
        // Only the previously recorded, account-owned service chat is migrated.
        client
            .invoke(&tl::functions::channels::EditTitle {
                channel: channel.clone(),
                title: crate::original::FORUM_TITLE.into(),
            })
            .await?;
        client
            .invoke(&tl::functions::messages::EditChatAbout {
                peer: input.clone(),
                about: crate::original::FORUM_DESCRIPTION.into(),
            })
            .await?;
    }
    if created || title == "atarustia-userbot" {
        let bytes = include_bytes!("../assets/bot_pfp.png");
        let result = async {
            let upload = client
                .upload_stream(&mut bytes.as_slice(), bytes.len(), "bot_pfp.png".into())
                .await?;
            client
                .invoke(&tl::functions::channels::EditPhoto {
                    channel: channel.clone(),
                    photo: tl::types::InputChatUploadedPhoto {
                        file: Some(upload.raw),
                        video: None,
                        video_start_ts: None,
                        video_emoji_markup: None,
                    }
                    .into(),
                })
                .await?;
            Ok::<_, anyhow::Error>(())
        }
        .await;
        if let Err(error) = result {
            tracing::warn!(error=%error,"cannot set service chat avatar");
        }
    }
    if !forum {
        client
            .invoke(&tl::functions::channels::ToggleForum {
                channel: channel.clone(),
                enabled: true,
                tabs: false,
            })
            .await?;
    }
    db.set("atarustia.forums", "channel_id", id)?;
    let mut topics = serde_json::Map::new();
    for title in ["Assets", "Backups", "Logs"] {
        let tl::enums::messages::ForumTopics::Topics(existing) = client
            .invoke(&tl::functions::messages::GetForumTopics {
                peer: input.clone(),
                q: Some(title.into()),
                offset_date: 0,
                offset_id: 0,
                offset_topic: 0,
                limit: 100,
            })
            .await?;
        let mut topic = existing.topics.into_iter().find_map(|t| match t {
            tl::enums::ForumTopic::Topic(t) if t.title == title => Some(t.id),
            _ => None,
        });
        if topic.is_none() {
            client
                .invoke(&tl::functions::messages::CreateForumTopic {
                    title_missing: false,
                    peer: input.clone(),
                    title: title.into(),
                    icon_color: Some(0x6fb9f0),
                    icon_emoji_id: Some(5877307202888273539),
                    random_id: rand::random(),
                    send_as: None,
                })
                .await?;
            let tl::enums::messages::ForumTopics::Topics(existing) = client
                .invoke(&tl::functions::messages::GetForumTopics {
                    peer: input.clone(),
                    q: Some(title.into()),
                    offset_date: 0,
                    offset_id: 0,
                    offset_topic: 0,
                    limit: 100,
                })
                .await?;
            topic = existing.topics.into_iter().find_map(|t| match t {
                tl::enums::ForumTopic::Topic(t) if t.title == title => Some(t.id),
                _ => None,
            });
        }
        topics.insert(
            title.into(),
            serde_json::json!(topic.context("topic missing after creation")?),
        );
    }
    db.set(
        "atarustia.forums",
        "forums_cache",
        serde_json::json!({crate::original::FORUM_TITLE:topics}),
    )?;
    Ok(id)
}
pub async fn destination(client: &Client, db: &Database) -> Result<Option<(PeerRef, i32)>> {
    topic_destination(client, db, "Backups").await
}
pub async fn topic_destination(
    client: &Client,
    db: &Database,
    name: &str,
) -> Result<Option<(PeerRef, i32)>> {
    let Some(id) = db.get("atarustia.forums", "channel_id").as_i64() else {
        return Ok(None);
    };
    let value = db.get("atarustia.forums", "forums_cache");
    let Some(topic) = value
        .get(crate::original::FORUM_TITLE)
        .or_else(|| value.get("atarustia-userbot"))
        .and_then(|v| v.get(name))
        .and_then(serde_json::Value::as_i64)
    else {
        return Ok(None);
    };
    ensure!(
        topic > 0 && topic <= i32::MAX as i64,
        "invalid backup topic"
    );
    let peer = client
        .resolve_peer(
            grammers_session::types::PeerId::channel(id)
                .context("invalid content channel")?
                .to_ambient_ref(),
        )
        .await?;
    let peer = peer
        .to_ref()
        .await
        .map_err(anyhow::Error::from_boxed)?
        .context("content peer missing")?;
    Ok(Some((peer, topic as i32)))
}
