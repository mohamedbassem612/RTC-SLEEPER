//! Phone-side copy of the last schedule form, laptop status and logs.
//!
//! Tailscale only carries traffic; it stores nothing. The laptop remains the source of truth for
//! the schedule. This cache lets the phone show the last confirmed data (and keep the form
//! fields) while the laptop is asleep or the app was restarted. It holds no credentials.
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

pub const MAX_LOG_BYTES: usize = 64 * 1024;
const MAX_FILE_BYTES: u64 = 256 * 1024;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct PhoneCache {
    /// Unix time of the last live update stored here.
    pub saved: u64,
    pub form: Value,
    pub state: Value,
    pub logs: String,
}
impl PhoneCache {
    /// Keeps only the newest part of the logs so the file stays small.
    pub fn capped(mut self) -> Self {
        if self.logs.len() > MAX_LOG_BYTES {
            let mut cut = self.logs.len() - MAX_LOG_BYTES;
            while !self.logs.is_char_boundary(cut) {
                cut += 1;
            }
            self.logs = self.logs[cut..].to_string();
        }
        self
    }
    pub fn write(&self, path: &Path) -> Result<()> {
        let bytes = serde_json::to_vec(&self.clone().capped())?;
        let temp = path.with_extension("tmp");
        std::fs::write(&temp, bytes)?;
        std::fs::rename(&temp, path)?;
        Ok(())
    }
    /// A missing, oversized or damaged file is simply "no cache".
    pub fn read(path: &Path) -> Result<Option<Self>> {
        let meta = match std::fs::metadata(path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e).context("Cannot read saved data"),
        };
        ensure!(meta.len() <= MAX_FILE_BYTES, "Saved data is too large");
        Ok(serde_json::from_slice::<Self>(&std::fs::read(path)?).ok())
    }
    pub fn clear(path: &Path) {
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn dir() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("rtc-cache-{}", crate::random_hex(6)));
        std::fs::create_dir_all(&d).unwrap();
        d
    }
    #[test]
    fn roundtrip_and_clear() {
        let path = dir().join("phone-cache.json");
        assert_eq!(PhoneCache::read(&path).unwrap(), None);
        let c = PhoneCache {
            saved: 42,
            form: json!({"sh":11,"sm":30}),
            state: json!({"status":"scheduled"}),
            logs: "line".into(),
        };
        c.write(&path).unwrap();
        assert_eq!(PhoneCache::read(&path).unwrap(), Some(c));
        PhoneCache::clear(&path);
        assert_eq!(PhoneCache::read(&path).unwrap(), None);
    }
    #[test]
    fn damaged_file_is_no_cache() {
        let path = dir().join("phone-cache.json");
        std::fs::write(&path, b"{oops").unwrap();
        assert_eq!(PhoneCache::read(&path).unwrap(), None);
    }
    #[test]
    fn logs_are_capped_on_a_char_boundary() {
        let c = PhoneCache {
            logs: "ع".repeat(MAX_LOG_BYTES),
            ..Default::default()
        }
        .capped();
        assert!(c.logs.len() <= MAX_LOG_BYTES);
        assert!(c.logs.chars().all(|ch| ch == 'ع'));
    }
}
