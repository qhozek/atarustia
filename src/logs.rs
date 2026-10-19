//! A bounded in-memory tracing writer. No log-file dependency or unbounded queue.
use std::{
    collections::VecDeque,
    io::{self, Write},
    sync::{Arc, Mutex},
};
use tracing_subscriber::fmt::MakeWriter;

const CAPACITY: usize = 256 * 1024;
#[derive(Clone, Default)]
pub struct LogBuffer {
    ring: Arc<Mutex<VecDeque<u8>>>,
    secrets: Arc<Mutex<Vec<String>>>,
}
impl LogBuffer {
    pub fn snapshot(&self) -> Vec<u8> {
        self.ring
            .lock()
            .expect("log lock")
            .iter()
            .copied()
            .collect()
    }
    pub fn clear(&self) {
        self.ring.lock().expect("log lock").clear();
    }
    pub fn add_secret(&self, secret: String) {
        if secret.len() >= 6 {
            let mut secrets = self.secrets.lock().unwrap();
            if !secrets.contains(&secret) {
                secrets.push(secret);
            }
        }
    }
    pub fn redact(&self, text: String) -> String {
        crate::output::censor(text, &self.secrets.lock().unwrap())
    }
}
impl Write for LogBuffer {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut ring = self
            .ring
            .lock()
            .map_err(|_| io::Error::other("log lock poisoned"))?;
        for byte in buf {
            if ring.len() == CAPACITY {
                ring.pop_front();
            }
            ring.push_back(*byte);
        }
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
impl<'a> MakeWriter<'a> for LogBuffer {
    type Writer = Self;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_and_clear() {
        let mut b = LogBuffer::default();
        b.write_all(&vec![b'x'; CAPACITY + 50]).unwrap();
        assert_eq!(b.snapshot().len(), CAPACITY);
        b.clear();
        assert!(b.snapshot().is_empty());
    }
    #[test]
    fn secrets_from_all_accounts_are_removed() {
        let b = LogBuffer::default();
        b.add_secret("account-one-secret".into());
        b.add_secret("account-two-secret".into());
        assert_eq!(
            b.redact("account-one-secret account-two-secret".into()),
            "<redacted> <redacted>"
        );
    }
}
