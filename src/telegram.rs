use crate::{
    config::Credentials,
    output,
    session::{EncryptedSession, MasterKey},
};
use anyhow::{Context, Result};
use grammers_client::{
    Client, SenderPool,
    message::{InputMessage, Message},
};
use grammers_session::types::PeerRef;
use std::{path::Path, sync::Arc, time::Duration};

pub struct Connection {
    pub client: Client,
    pub session: Arc<EncryptedSession>,
    pub updates: Option<tokio::sync::mpsc::Receiver<grammers_session::updates::UpdatesLike>>,
    pub handle: grammers_client::sender::SenderPoolFatHandle,
    task: Option<tokio::task::JoinHandle<()>>,
}
pub fn validate_proxy(proxy: &str) -> Result<Option<grammers_client::sender::mtproxy::Config>> {
    let url = reqwest::Url::parse(proxy).map_err(|_| anyhow::anyhow!("invalid proxy URL"))?;
    if url.scheme() == "socks5" {
        anyhow::ensure!(
            url.host_str().is_some()
                && url.port().is_some_and(|p| p > 0)
                && matches!(url.path(), "" | "/")
                && url.query().is_none()
                && url.fragment().is_none(),
            "proxy must be socks5://host:port or a valid MTProxy URL"
        );
        Ok(None)
    } else {
        Ok(Some(grammers_client::sender::mtproxy::Config::parse(
            proxy,
        )?))
    }
}
impl Connection {
    pub fn open(
        root: &Path,
        name: &str,
        credentials: &Credentials,
        key: Arc<MasterKey>,
    ) -> Result<Self> {
        Self::open_with_proxy(root, name, credentials, key, None)
    }
    pub fn open_with_proxy(
        root: &Path,
        name: &str,
        credentials: &Credentials,
        key: Arc<MasterKey>,
        proxy: Option<&str>,
    ) -> Result<Self> {
        let mtproxy = proxy.map(validate_proxy).transpose()?.flatten();
        let session = Arc::new(EncryptedSession::load(
            root.join(format!("atarustia-{name}.atsession")),
            key,
        )?);
        let params = grammers_client::sender::ConnectionParams {
            device_model: format!("atarustia Rust ({})", std::env::consts::ARCH),
            app_version: env!("CARGO_PKG_VERSION").into(),
            retry_policy: Box::new(grammers_client::sender::NoRetries),
            updates_channel_capacity: std::num::NonZeroUsize::new(1024).unwrap(),
            proxy_url: if mtproxy.is_none() {
                proxy.map(str::to_owned)
            } else {
                None
            },
            mtproxy,
            ..Default::default()
        };
        let SenderPool {
            runner,
            updates,
            handle,
        } = SenderPool::with_configuration(session.clone(), credentials.api_id, params);
        let client = Client::new(handle.clone());
        let task = tokio::spawn(async move {
            runner.run().await;
        });
        Ok(Self {
            client,
            session,
            updates: Some(updates),
            handle,
            task: Some(task),
        })
    }
    pub async fn close(mut self) -> Result<()> {
        self.handle.quit();
        if let Some(mut task) = self.task.take()
            && tokio::time::timeout(Duration::from_secs(5), &mut task)
                .await
                .is_err()
        {
            task.abort();
        }
        self.session.flush()
    }
}
impl Drop for Connection {
    fn drop(&mut self) {
        self.handle.quit();
        if let Some(task) = self.task.take() {
            task.abort();
        }
        if let Err(e) = self.session.flush() {
            tracing::error!(error=%e,"session flush failed");
        }
    }
}
pub async fn answer(message: &Message, text: &str) -> Result<()> {
    for (index, page) in output::pages(text).into_iter().enumerate() {
        if index == 0 && message.outgoing() {
            message.edit(page).await?;
        } else {
            message.reply(page).await?;
        }
    }
    Ok(())
}
pub async fn send_file(
    client: &Client,
    peer: PeerRef,
    name: &str,
    bytes: Vec<u8>,
    caption: &str,
) -> Result<()> {
    let upload = client
        .upload_stream(&mut bytes.as_slice(), bytes.len(), name.to_owned())
        .await?;
    client
        .send_message(peer, InputMessage::new().text(caption).document(upload))
        .await?;
    Ok(())
}
pub async fn send_backup(
    client: &Client,
    db: &crate::database::Database,
    bytes: Vec<u8>,
    caption: &str,
) -> Result<()> {
    let destination = tokio::time::timeout(
        Duration::from_secs(20),
        crate::content::destination(client, db),
    )
    .await;
    let (peer, topic) = match destination {
        Ok(Ok(Some((peer, topic)))) => (peer, Some(topic)),
        _ => (
            grammers_session::types::PeerId::self_user().to_ambient_ref(),
            None,
        ),
    };
    let upload = client
        .upload_stream(
            &mut bytes.as_slice(),
            bytes.len(),
            "atarustia.backup.json".into(),
        )
        .await?;
    client
        .send_message(
            peer,
            InputMessage::new()
                .text(caption)
                .document(upload)
                .reply_to(topic),
        )
        .await?;
    Ok(())
}
pub async fn peer(message: &Message) -> Result<PeerRef> {
    message
        .peer_ref()
        .await
        .map_err(|e| anyhow::anyhow!(e.to_string()))?
        .context("peer not cached")
}
pub fn topic(message: &Message) -> Option<i32> {
    match message.reply_header() {
        Some(grammers_client::tl::enums::MessageReplyHeader::Header(h)) if h.forum_topic => {
            h.reply_to_top_id.or(h.reply_to_msg_id)
        }
        _ => None,
    }
}
