//! The decision log: every routing decision persisted as JSONL tuning data.

use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::policy::Decision;

pub fn dir() -> PathBuf {
    directories::BaseDirs::new()
        .map(|b| b.home_dir().join(".config/maele"))
        .unwrap_or_else(|| PathBuf::from(".maele"))
}

pub fn decisions_path() -> PathBuf {
    dir().join("decisions.jsonl")
}

/// Append a decision. Failures are the caller's to ignore: logging must never
/// block a routing turn.
pub fn append(text: &str, decision: &Decision) -> std::io::Result<()> {
    let dir = dir();
    fs::create_dir_all(&dir)?;
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let record = serde_json::json!({
        "ts": ts,
        "text": text,
        "decision": decision.as_json(),
    });
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(decisions_path())?;
    writeln!(f, "{}", serde_json::to_string(&record)?)?;
    Ok(())
}

/// The most recent decision, if any.
pub fn last() -> Option<serde_json::Value> {
    let f = fs::File::open(decisions_path()).ok()?;
    let mut last = None;
    for line in BufReader::new(f).lines().map_while(Result::ok) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) {
            last = Some(v);
        }
    }
    last
}
