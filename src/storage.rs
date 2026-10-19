//! Crash-safe replacement and an exclusive lock for the entire data directory.
use anyhow::{Context, Result, bail};
use fs2::FileExt;
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::Path,
};

/// Size-check both metadata and the bytes actually read (a file can grow after stat).
pub fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let file = File::open(path)?;
    anyhow::ensure!(
        file.metadata()?.is_file() && file.metadata()?.len() <= limit,
        "input is not a regular file or exceeds the size limit"
    );
    let mut bytes = Vec::new();
    file.take(limit.checked_add(1).context("input limit overflow")?)
        .read_to_end(&mut bytes)?;
    anyhow::ensure!(bytes.len() as u64 <= limit, "input exceeds the size limit");
    Ok(bytes)
}

pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().context("file has no parent directory")?;
    fs::create_dir_all(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    file.write_all(bytes)?;
    file.as_file().sync_all()?;
    file.persist(path)
        .map_err(|e| e.error)
        .with_context(|| format!("save {}", path.display()))?;
    #[cfg(unix)]
    File::open(parent)?.sync_all()?;
    Ok(())
}

pub struct DataLock {
    _file: File,
}
impl DataLock {
    pub fn acquire(root: &Path) -> Result<Self> {
        fs::create_dir_all(root)?;
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(root.join("atarustia.lock"))?;
        if file.try_lock_exclusive().is_err() {
            bail!("another atarustia process is using {}", root.display());
        }
        Ok(Self { _file: file })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_input_rejects_oversize_and_accepts_exact_limit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("input");
        std::fs::write(&path, b"1234").unwrap();
        assert_eq!(read_bounded(&path, 4).unwrap(), b"1234");
        assert!(read_bounded(&path, 3).is_err());
        assert!(read_bounded(dir.path(), 4).is_err());
    }
    #[test]
    fn replacement_and_lock() {
        let root = tempfile::tempdir().unwrap();
        let p = root.path().join("db.json");
        atomic_write(&p, b"first").unwrap();
        atomic_write(&p, b"second").unwrap();
        assert_eq!(fs::read(p).unwrap(), b"second");
        let lock = DataLock::acquire(root.path()).unwrap();
        assert!(DataLock::acquire(root.path()).is_err());
        drop(lock);
        assert!(DataLock::acquire(root.path()).is_ok());
    }
}
