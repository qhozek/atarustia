//! Rust session format is deliberately distinct from Python's ATBINC1 container.
//! Authentication data never goes through a plaintext temporary file.
use crate::storage::atomic_write;
use aes_gcm::{
    Aes256Gcm, KeyInit, Nonce,
    aead::{Aead, Payload},
};
use anyhow::{Context, Result, bail, ensure};
use grammers_session::{
    BoxFuture, Session, SessionData,
    types::{ChannelState, DcOption, PeerId, PeerInfo, UpdateState, UpdatesState},
};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::{
    fs::OpenOptions,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::Mutex,
};
use zeroize::Zeroizing;

const MAGIC: &[u8; 8] = b"ATRUST1\0";
const MAX_SESSION: u64 = 32 * 1024 * 1024;
pub struct MasterKey(Zeroizing<[u8; 32]>);
impl MasterKey {
    pub fn load(root: &Path) -> Result<Self> {
        let path = root.join("atarustia.key");
        if path.exists() {
            let data = Zeroizing::new(std::fs::read_to_string(path)?);
            let bytes = Zeroizing::new(hex::decode(data.trim()).context("invalid session key")?);
            ensure!(bytes.len() == 32, "session key must contain 32 bytes");
            let mut key = Zeroizing::new([0; 32]);
            key.copy_from_slice(&bytes);
            return Ok(Self(key));
        }
        // Never silently generate a replacement key for existing encrypted sessions.
        for entry in std::fs::read_dir(root)? {
            if entry?.path().extension().is_some_and(|x| x == "atsession") {
                bail!("session key missing; restore atarustia.key before starting");
            }
        }
        let mut key = Zeroizing::new([0; 32]);
        rand::rngs::OsRng.fill_bytes(key.as_mut());
        let mut options = OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(path)?;
        let encoded = Zeroizing::new(hex::encode(key.as_ref()));
        file.write_all(encoded.as_bytes())?;
        file.sync_all()?;
        Ok(Self(key))
    }
    pub fn seal(&self, bytes: &[u8]) -> Result<Vec<u8>> {
        let cipher = Aes256Gcm::new_from_slice(self.0.as_ref()).expect("32-byte key");
        let mut nonce = [0; 12];
        rand::rngs::OsRng.fill_bytes(&mut nonce);
        let encrypted = cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: bytes,
                    aad: MAGIC,
                },
            )
            .map_err(|_| anyhow::anyhow!("session encryption failed"))?;
        Ok([MAGIC.as_slice(), &nonce, &encrypted].concat())
    }
    pub fn open(&self, bytes: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
        ensure!(
            bytes.len() >= 36 && bytes.starts_with(MAGIC),
            "not a Rust atarustia session (Python sessions require a new login)"
        );
        let cipher = Aes256Gcm::new_from_slice(self.0.as_ref()).expect("32-byte key");
        let plain = cipher
            .decrypt(
                Nonce::from_slice(&bytes[8..20]),
                Payload {
                    msg: &bytes[20..],
                    aad: MAGIC,
                },
            )
            .map_err(|_| {
                anyhow::anyhow!("session authentication failed: wrong key or damaged file")
            })?;
        Ok(Zeroizing::new(plain))
    }
}

#[derive(Serialize, Deserialize)]
struct Snapshot {
    version: u32,
    home_dc: i32,
    dcs: Vec<DcOption>,
    peers: Vec<PeerInfo>,
    updates: UpdatesState,
}
impl Snapshot {
    fn from_data(d: &SessionData) -> Self {
        Self {
            version: 1,
            home_dc: d.home_dc,
            dcs: d.dc_options.values().cloned().collect(),
            peers: d.peer_infos.values().cloned().collect(),
            updates: d.updates_state.clone(),
        }
    }
    fn into_data(self) -> Result<SessionData> {
        ensure!(self.version == 1, "unsupported session version");
        Ok(SessionData {
            home_dc: self.home_dc,
            dc_options: self.dcs.into_iter().map(|v| (v.id, v)).collect(),
            peer_infos: self.peers.into_iter().map(|v| (v.id(), v)).collect(),
            updates_state: self.updates,
        })
    }
}
struct State {
    data: SessionData,
    dirty: bool,
}
pub struct EncryptedSession {
    path: PathBuf,
    key: std::sync::Arc<MasterKey>,
    state: Mutex<State>,
}
impl EncryptedSession {
    pub fn import(path: PathBuf, key: std::sync::Arc<MasterKey>, data: SessionData) -> Result<()> {
        ensure!(!path.exists(), "destination session already exists");
        let session = Self {
            path,
            key,
            state: Mutex::new(State { data, dirty: true }),
        };
        session.flush()
    }
    pub fn load(path: PathBuf, key: std::sync::Arc<MasterKey>) -> Result<Self> {
        let data = if path.exists() {
            ensure!(
                std::fs::metadata(&path)?.len() <= MAX_SESSION,
                "session too large"
            );
            let plain = key.open(&std::fs::read(&path)?)?;
            serde_json::from_slice::<Snapshot>(&plain)?.into_data()?
        } else {
            SessionData::default()
        };
        Ok(Self {
            path,
            key,
            state: Mutex::new(State { data, dirty: false }),
        })
    }
    pub fn flush(&self) -> Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("session lock poisoned"))?;
        if !state.dirty {
            return Ok(());
        }
        let plain = Zeroizing::new(serde_json::to_vec(&Snapshot::from_data(&state.data))?);
        atomic_write(&self.path, &self.key.seal(&plain)?)?;
        state.dirty = false;
        Ok(())
    }
    fn mutate(&self, f: impl FnOnce(&mut SessionData)) -> io::Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| io::Error::other("session lock poisoned"))?;
        f(&mut state.data);
        state.dirty = true;
        Ok(())
    }
}
impl Session for EncryptedSession {
    type Error = io::Error;
    fn home_dc_id(&self) -> io::Result<i32> {
        Ok(self
            .state
            .lock()
            .map_err(|_| io::Error::other("session poisoned"))?
            .data
            .home_dc)
    }
    fn set_home_dc_id(&self, id: i32) -> BoxFuture<'_, io::Result<()>> {
        Box::pin(async move {
            self.mutate(|d| d.home_dc = id)?;
            self.flush().map_err(io::Error::other)
        })
    }
    fn dc_option(&self, id: i32) -> io::Result<Option<DcOption>> {
        Ok(self
            .state
            .lock()
            .map_err(|_| io::Error::other("session poisoned"))?
            .data
            .dc_options
            .get(&id)
            .cloned())
    }
    fn set_dc_option(&self, dc: &DcOption) -> BoxFuture<'_, io::Result<()>> {
        let dc = dc.clone();
        Box::pin(async move {
            self.mutate(|d| {
                d.dc_options.insert(dc.id, dc);
            })?;
            self.flush().map_err(io::Error::other)
        })
    }
    fn peer(&self, id: PeerId) -> BoxFuture<'_, io::Result<Option<PeerInfo>>> {
        Box::pin(async move {
            Ok(self
                .state
                .lock()
                .map_err(|_| io::Error::other("session poisoned"))?
                .data
                .peer_infos
                .get(&id)
                .cloned())
        })
    }
    fn cache_peer(&self, peer: PeerInfo) -> BoxFuture<'_, io::Result<()>> {
        Box::pin(async move {
            self.mutate(|d| {
                d.peer_infos
                    .entry(peer.id())
                    .or_insert_with(|| peer.clone())
                    .extend_info(&peer);
            })
        })
    }
    fn updates_state(&self) -> BoxFuture<'_, io::Result<UpdatesState>> {
        Box::pin(async move {
            Ok(self
                .state
                .lock()
                .map_err(|_| io::Error::other("session poisoned"))?
                .data
                .updates_state
                .clone())
        })
    }
    fn set_update_state(&self, update: UpdateState) -> BoxFuture<'_, io::Result<()>> {
        Box::pin(async move {
            self.mutate(|d| match update {
                UpdateState::All(s) => d.updates_state = s,
                UpdateState::Primary { pts, date, seq } => {
                    d.updates_state.pts = pts;
                    d.updates_state.date = date;
                    d.updates_state.seq = seq;
                }
                UpdateState::Secondary { qts } => d.updates_state.qts = qts,
                UpdateState::Channel { id, pts } => {
                    d.updates_state.channels.retain(|s| s.id != id);
                    d.updates_state.channels.push(ChannelState { id, pts });
                }
            })
        })
    }
}
impl Drop for EncryptedSession {
    fn drop(&mut self) {
        if let Err(e) = self.flush() {
            tracing::error!(error=%e,"unable to flush session");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authenticated_encryption() {
        let dir = tempfile::tempdir().unwrap();
        let key = MasterKey::load(dir.path()).unwrap();
        let sealed = key.seal(b"auth key").unwrap();
        assert!(!sealed.windows(8).any(|w| w == b"auth key"));
        assert_eq!(&*key.open(&sealed).unwrap(), b"auth key");
        let mut tampered = sealed.clone();
        tampered[21] ^= 1;
        assert!(key.open(&tampered).is_err());
        let other = MasterKey(Zeroizing::new([5; 32]));
        assert!(other.open(&sealed).is_err());
        assert_ne!(key.seal(b"auth key").unwrap(), sealed);
    }
    #[test]
    fn missing_key_does_not_destroy_sessions() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.atsession"), b"encrypted").unwrap();
        assert!(MasterKey::load(dir.path()).is_err());
        assert!(!dir.path().join("atarustia.key").exists());
    }
    #[tokio::test]
    async fn persistence() {
        let dir = tempfile::tempdir().unwrap();
        let key = std::sync::Arc::new(MasterKey::load(dir.path()).unwrap());
        let p = dir.path().join("a.atsession");
        let s = EncryptedSession::load(p.clone(), key.clone()).unwrap();
        s.set_home_dc_id(4).await.unwrap();
        s.cache_peer(PeerInfo::User {
            id: 123,
            auth: None,
            bot: Some(false),
            is_self: Some(true),
        })
        .await
        .unwrap();
        s.flush().unwrap();
        let restored = EncryptedSession::load(p, key).unwrap();
        assert_eq!(restored.home_dc_id().unwrap(), 4);
        assert!(
            restored
                .peer(PeerId::user(123).unwrap())
                .await
                .unwrap()
                .is_some()
        );
    }
    #[test]
    fn corrupt_session_is_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let key = std::sync::Arc::new(MasterKey::load(dir.path()).unwrap());
        let path = dir.path().join("a.atsession");
        let bytes = b"ATBINC1\0not a Rust session";
        std::fs::write(&path, bytes).unwrap();
        assert!(EncryptedSession::load(path.clone(), key).is_err());
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }
}
