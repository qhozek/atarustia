//! Content-chat assets and bounded media identifiers; no token/session data in caches.
use crate::database::Database;
use anyhow::{Context, Result, ensure};
use grammers_client::{
    Client,
    media::Media,
    message::{InputMessage, Message},
};

pub async fn store_asset(client: &Client, db: &Database, message: &Message) -> Result<i32> {
    let (peer, topic) = crate::content::topic_destination(client, db, "Assets")
        .await?
        .context("Assets topic unavailable; service chat setup will be retried on restart")?;
    let media = message.media();
    let mut input = InputMessage::new()
        .text(message.text())
        .reply_to(Some(topic));
    if let Some(media) = media {
        input = input.copy_media(&media);
    }
    let sent = client.send_message(peer, input).await?;
    Ok(sent.id())
}
pub async fn fetch_asset(client: &Client, db: &Database, id: i32) -> Result<Option<Message>> {
    ensure!(id > 0, "asset ID must be positive");
    let (peer, _) = crate::content::topic_destination(client, db, "Assets")
        .await?
        .context("Assets topic unavailable")?;
    Ok(client
        .get_messages_by_id(peer, &[id])
        .await?
        .into_iter()
        .next()
        .flatten())
}
pub fn cache_key(bot_id: i64, bytes: &[u8]) -> String {
    format!("{bot_id}:{}", crate::modules::digest(bytes))
}
pub fn cached(db: &Database, key: &str) -> Option<String> {
    db.get("RustMedia", "cache")
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
}
pub fn remember(db: &Database, key: &str, media: &Media) -> Result<()> {
    let file = crate::botapi::File::from_media(media)?;
    db.transaction(|data| {
        let ns = data
            .entry("RustMedia")
            .or_insert_with(|| serde_json::json!({}))
            .as_object_mut()
            .context("invalid media namespace")?;
        let cache = ns
            .entry("cache")
            .or_insert_with(|| serde_json::json!({}))
            .as_object_mut()
            .context("invalid media cache")?;
        if cache.len() >= 256
            && !cache.contains_key(key)
            && let Some(oldest) = cache.keys().next().cloned()
        {
            cache.remove(&oldest);
        }
        cache.insert(key.into(), serde_json::Value::String(file.file_id));
        Ok(())
    })
}
pub fn forget(db: &Database, key: &str) -> Result<()> {
    db.transaction(|data| {
        if let Some(cache) = data
            .get_mut("RustMedia")
            .and_then(|v| v.get_mut("cache"))
            .and_then(serde_json::Value::as_object_mut)
        {
            cache.remove(key);
        }
        Ok(())
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bot_cache_keys_are_namespaced() {
        assert_ne!(cache_key(1, b"photo"), cache_key(2, b"photo"));
        assert_ne!(cache_key(1, b"photo"), cache_key(1, b"changed"));
    }
}
