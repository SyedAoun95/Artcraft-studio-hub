//! Append-only log of agent tool calls, shown in the hub window.

use crate::config::{activity_path, ensure_dir};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Entry {
    pub ts: u64,
    pub client: String,
    pub app: String,
    pub tool: String,
    pub ok: bool,
    pub ms: u64,
    #[serde(default)]
    pub note: String,
}

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub fn record(e: &Entry) {
    let path = activity_path();
    if let Some(dir) = path.parent() {
        let _ = ensure_dir(dir);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{}", serde_json::to_string(e).unwrap_or_default());
    }
}

/// The last `n` entries, newest first.
pub fn tail(n: usize) -> Vec<Entry> {
    let text = std::fs::read_to_string(activity_path()).unwrap_or_default();
    let mut v: Vec<Entry> = text.lines().rev().take(n).filter_map(|l| serde_json::from_str(l).ok()).collect();
    v.truncate(n);
    v
}

pub fn clear() {
    let _ = std::fs::write(activity_path(), b"");
}
