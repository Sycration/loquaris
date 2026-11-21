//! Audio storage. Clips are immutable and content-addressed by the SHA-256
//! of their WAV bytes; the [`BlobStore`] trait keeps the file store
//! replaceable (by S3, say). Which projects may read a blob is recorded in
//! `project_blobs`.
use std::fs;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use sqlx::SqliteConnection;

use crate::util::now_ms;

/// Content-addressed blob storage. Methods block; async callers use
/// `spawn_blocking`.
pub trait BlobStore: Send + Sync + 'static {
    /// Stores `bytes` under `sha256` unless already present.
    fn put(&self, sha256: &str, bytes: &[u8]) -> io::Result<()>;
    fn get(&self, sha256: &str) -> io::Result<Vec<u8>>;
    /// `len` bytes from `start` (fewer at the end of the blob).
    fn get_range(&self, sha256: &str, start: u64, len: u64) -> io::Result<Vec<u8>>;
    /// The blob's size, or `None` if it is not stored.
    fn size(&self, sha256: &str) -> io::Result<Option<u64>>;
}

/// True for a lowercase hex SHA-256, the only form used as a blob key.
pub fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Blobs as files under `root/ab/cdef…`, split on the first byte so no
/// directory grows too large.
pub struct FsBlobStore {
    root: PathBuf,
}

impl FsBlobStore {
    pub fn new(root: impl Into<PathBuf>) -> io::Result<Self> {
        let root = root.into();
        fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    fn path(&self, sha256: &str) -> io::Result<PathBuf> {
        if !is_sha256(sha256) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid blob key"));
        }
        Ok(self.root.join(&sha256[..2]).join(&sha256[2..]))
    }
}

impl BlobStore for FsBlobStore {
    fn put(&self, sha256: &str, bytes: &[u8]) -> io::Result<()> {
        let path = self.path(sha256)?;
        if path.exists() {
            return Ok(());
        }
        let dir = path.parent().unwrap_or(Path::new("."));
        fs::create_dir_all(dir)?;
        // Write to a temporary name and rename, so a crash never leaves a
        // truncated file under the final name.
        let temp = dir.join(format!(".{}.{}.tmp", &sha256[2..], crate::util::new_id()));
        let mut file = fs::File::create(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp, &path).inspect_err(|_| {
            let _ = fs::remove_file(&temp);
        })
    }

    fn get(&self, sha256: &str) -> io::Result<Vec<u8>> {
        fs::read(self.path(sha256)?)
    }

    fn get_range(&self, sha256: &str, start: u64, len: u64) -> io::Result<Vec<u8>> {
        let mut file = fs::File::open(self.path(sha256)?)?;
        file.seek(SeekFrom::Start(start))?;
        let mut bytes = Vec::with_capacity(len.min(64 << 20) as usize);
        file.take(len).read_to_end(&mut bytes)?;
        Ok(bytes)
    }

    fn size(&self, sha256: &str) -> io::Result<Option<u64>> {
        match fs::metadata(self.path(sha256)?) {
            Ok(meta) => Ok(Some(meta.len())),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }
}

/// What the database records about a stored clip.
pub struct BlobInfo<'a> {
    pub sha256: &'a str,
    pub size: u64,
    pub sample_rate: u32,
    pub frames: u32,
    pub channels: u16,
}

/// Records a stored blob and lets `project_id` read it.
pub async fn register(
    conn: &mut SqliteConnection,
    info: &BlobInfo<'_>,
    project_id: &str,
    user_id: &str,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT OR IGNORE INTO blobs (sha256, size, mime, sample_rate, frames, channels, created_by, created_at)
         VALUES (?, ?, 'audio/wav', ?, ?, ?, ?, ?)",
    )
    .bind(info.sha256)
    .bind(info.size as i64)
    .bind(info.sample_rate)
    .bind(info.frames)
    .bind(info.channels)
    .bind(user_id)
    .bind(now_ms())
    .execute(&mut *conn)
    .await?;
    sqlx::query("INSERT OR IGNORE INTO project_blobs (project_id, sha256) VALUES (?, ?)")
        .bind(project_id)
        .bind(info.sha256)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_by_hash() {
        let dir = tempfile::tempdir().unwrap();
        let store = FsBlobStore::new(dir.path()).unwrap();
        let bytes = b"RIFF....WAVE".to_vec();
        let sha = crate::util::sha256_hex(&bytes);
        assert_eq!(store.size(&sha).unwrap(), None);
        store.put(&sha, &bytes).unwrap();
        store.put(&sha, &bytes).unwrap();
        assert!(dir.path().join(&sha[..2]).join(&sha[2..]).exists());
        assert_eq!(store.get(&sha).unwrap(), bytes);
        assert_eq!(store.get_range(&sha, 4, 4).unwrap(), b"....");
        assert_eq!(store.get_range(&sha, 10, 100).unwrap(), b"VE");
        assert_eq!(store.size(&sha).unwrap(), Some(12));
        assert!(store.get("../etc/passwd").is_err());
    }
}
