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
    /// Sleep history kept on the phone, newer than what the laptop itself still remembers.
    #[serde(default)]
    pub history: Vec<Value>,
}
pub const MAX_HISTORY: usize = 200;
/// Merges the laptop's recent sleep records into the phone's longer history, keyed by the time
/// the sleep started. A record that gained its wake information replaces its earlier open form.
pub fn merge_history(existing: &mut Vec<Value>, incoming: &[Value], cap: usize) {
    for rec in incoming {
        let Some(at) = rec["at"].as_u64() else {
            continue;
        };
        match existing.iter().position(|r| r["at"].as_u64() == Some(at)) {
            Some(i) => {
                if !rec["woke_at"].is_null() || existing[i]["woke_at"].is_null() {
                    existing[i] = rec.clone();
                }
            }
            None => existing.push(rec.clone()),
        }
    }
    existing.sort_by_key(|r| r["at"].as_u64().unwrap_or(0));
    if existing.len() > cap {
        let extra = existing.len() - cap;
        existing.drain(..extra);
    }
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
            history: vec![json!({"at":5})],
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
    #[test]
    fn history_merge_updates_dedupes_and_caps() {
        let mut h = vec![];
        merge_history(&mut h, &[json!({"at":10,"woke_at":null}), json!({"at":20,"woke_at":null})], 3);
        assert_eq!(h.len(), 2);
        // The same sleep arrives again, now closed: replaces, does not duplicate.
        merge_history(&mut h, &[json!({"at":10,"woke_at":15,"wake":"alarm"})], 3);
        assert_eq!(h.len(), 2);
        assert_eq!(h[0]["wake"], "alarm");
        // An older open copy never overwrites a closed one.
        merge_history(&mut h, &[json!({"at":10,"woke_at":null})], 3);
        assert_eq!(h[0]["wake"], "alarm");
        // Cap keeps the newest; records without a start time are ignored.
        merge_history(&mut h, &[json!({"at":30}), json!({"at":40}), json!({"nope":1})], 3);
        assert_eq!(h.iter().map(|r| r["at"].as_u64().unwrap()).collect::<Vec<_>>(), vec![20, 30, 40]);
    }
}
