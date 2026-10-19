//! Bounded, serialized BotFather conversation using MTProto messages (no HTTP Bot API).
use anyhow::{Context, Result, ensure};
use grammers_client::{Client, message::Message};
use grammers_session::types::PeerRef;
use std::time::Duration;

pub fn extract_token(text: &str) -> Option<String> {
    text.split_whitespace().find_map(|part| {
        let part = part
            .trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != ':' && c != '_' && c != '-');
        crate::config::validate_bot_token(part)
            .ok()
            .map(|_| part.into())
    })
}
pub struct Conversation {
    client: Client,
    peer: PeerRef,
    trash: Vec<i32>,
}
impl Conversation {
    pub async fn new(client: &Client) -> Result<Self> {
        let peer = client
            .resolve_username("BotFather")
            .await?
            .context("BotFather not found")?
            .to_ref()
            .await
            .map_err(anyhow::Error::from_boxed)?
            .context("BotFather peer missing")?;
        Ok(Self {
            client: client.clone(),
            peer,
            trash: vec![],
        })
    }
    pub async fn ask(&mut self, text: &str) -> Result<Message> {
        tokio::time::sleep(Duration::from_millis(500)).await;
        let sent = tokio::time::timeout(
            Duration::from_secs(20),
            self.client.send_message(self.peer, text),
        )
        .await??;
        self.trash.push(sent.id());
        let response = tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                let mut messages = self.client.iter_messages(self.peer).limit(8);
                while let Some(message) = messages.next().await? {
                    if message.id() > sent.id() && !message.outgoing() {
                        return Ok::<_, anyhow::Error>(message);
                    }
                }
                tokio::time::sleep(Duration::from_millis(600)).await;
            }
        })
        .await
        .context("BotFather did not reply in 20 seconds")??;
        self.trash.push(response.id());
        Ok(response)
    }
    pub async fn cleanup(&self) {
        let _ = tokio::time::timeout(
            Duration::from_secs(10),
            self.client.delete_messages(self.peer, &self.trash),
        )
        .await;
    }
}
pub async fn provision(client: &Client, username: &str) -> Result<String> {
    ensure!(
        (5..=32).contains(&username.len())
            && username.to_lowercase().ends_with("bot")
            && username
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'_'),
        "invalid bot username"
    );
    let mut conversation = Conversation::new(client).await?;
    let result = async {
        let reply = conversation.ask("/newbot").await?;
        ensure!(
            !reply.text().contains("Too many") && !reply.text().contains("That I cannot do"),
            "BotFather refused creation; check your bot limit"
        );
        conversation.ask("atarustia Rust").await?;
        let reply = conversation.ask(username).await?;
        let token = extract_token(reply.text())
            .context("BotFather did not return a token; username may be occupied")?;
        let setup = async {
            conversation.ask("/setinline").await?;
            conversation.ask(&format!("@{username}")).await?;
            conversation.ask("atarustia · command").await?;
            conversation.ask("/setinlinefeedback").await?;
            conversation.ask(&format!("@{username}")).await?;
            conversation.ask("Enabled").await?;
            Ok::<_, anyhow::Error>(())
        }
        .await;
        if setup.is_err() {
            tracing::warn!(
                "bot created but inline setup incomplete; enable /setinline in BotFather manually"
            );
        }
        Ok(token)
    }
    .await;
    if result.is_err() {
        let _ = conversation.ask("/cancel").await;
    }
    conversation.cleanup().await;
    result
}
pub async fn owned_token(client: &Client, username: &str) -> Result<String> {
    ensure!(!username.is_empty(), "bot username missing");
    let mut c = Conversation::new(client).await?;
    let result = async {
        c.ask("/token").await?;
        let r = c
            .ask(&format!("@{}", username.trim_start_matches('@')))
            .await?;
        extract_token(r.text()).context("BotFather did not provide a token for this bot")
    }
    .await;
    c.cleanup().await;
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn token_extraction() {
        let token = format!("123456789:{}", "a".repeat(35));
        assert_eq!(
            extract_token(&format!("Use this token:\n{token}\nKeep safe")),
            Some(token)
        );
        assert!(extract_token("Too many bots").is_none());
    }
}
