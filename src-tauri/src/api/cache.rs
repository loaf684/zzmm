use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

/// Small on-disk TTL cache for GameBanana responses. It keeps the app usable
/// offline for a while and avoids hammering the API while paging.
///
/// Entries are one JSON file each; `cached` is deliberately dumb (no eviction)
/// because the payloads are a few kilobytes and live in the app config folder.
#[derive(Serialize, Deserialize)]
struct Envelope<T> {
    saved_at: u64,
    payload: T,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn entry_path(dir: &Path, key: &str) -> PathBuf {
    // DefaultHasher is deterministic for a fixed input, which is all a cache
    // filename needs; keys include the full query so collisions are irrelevant.
    let mut hasher = DefaultHasher::new();
    key.hash(&mut hasher);
    dir.join(format!("{:016x}.json", hasher.finish()))
}

pub fn read<T: DeserializeOwned>(dir: &Path, key: &str, ttl: Duration) -> Option<T> {
    let path = entry_path(dir, key);
    let raw = fs::read_to_string(&path).ok()?;
    let envelope: Envelope<T> = serde_json::from_str(&raw).ok()?;

    let age_ms = now_ms().saturating_sub(envelope.saved_at);
    if age_ms > ttl.as_millis() as u64 {
        return None;
    }

    Some(envelope.payload)
}

/// Best-effort write: a failed cache write must never fail the request.
pub fn write<T: Serialize>(dir: &Path, key: &str, value: &T) {
    if let Err(error) = fs::create_dir_all(dir) {
        log::warn!("Could not create cache folder {}: {error}", dir.display());
        return;
    }

    let envelope = Envelope {
        saved_at: now_ms(),
        payload: value,
    };

    let path = entry_path(dir, key);
    let temp = path.with_extension("json.tmp");

    let result = serde_json::to_string(&envelope)
        .map_err(|error| error.to_string())
        .and_then(|json| fs::write(&temp, json).map_err(|error| error.to_string()))
        .and_then(|_| fs::rename(&temp, &path).map_err(|error| error.to_string()));

    if let Err(error) = result {
        log::warn!("Could not cache GameBanana response: {error}");
        let _ = fs::remove_file(&temp);
    }
}
