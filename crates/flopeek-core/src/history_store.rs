//! Bounded immutable Git-revision snapshot cache.
//!
//! This cache is derived evidence only.  Authoritative graph, Context, Assertion,
//! and Historical Candidate state remains in SQLite.  Snapshot JSON is allowed
//! because it is an immutable portable cache and contains no source bodies.

use crate::model::{HISTORICAL_SNAPSHOT_SCHEMA, HistoricalSnapshot};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const HISTORY_DIRECTORY: &str = "history";
const MAX_SNAPSHOT_BYTES: u64 = 8 * 1024 * 1024;

pub fn load(root: &Path, revision: &str) -> Result<Option<HistoricalSnapshot>, String> {
    let path = snapshot_path(root, revision)?;
    if !path.is_file() {
        return Ok(None);
    }
    let metadata = fs::metadata(&path)
        .map_err(|error| format!("Unable to inspect historical snapshot: {error}"))?;
    if metadata.len() > MAX_SNAPSHOT_BYTES {
        return Err(format!(
            "Historical snapshot exceeds the {} byte bound.",
            MAX_SNAPSHOT_BYTES
        ));
    }
    let payload = fs::read_to_string(&path)
        .map_err(|error| format!("Unable to read historical snapshot: {error}"))?;
    let snapshot = serde_json::from_str::<HistoricalSnapshot>(&payload)
        .map_err(|error| format!("Historical snapshot is corrupted: {error}"))?;
    let project_id = crate::graph::project_id(root);
    if snapshot.schema_version != HISTORICAL_SNAPSHOT_SCHEMA
        || snapshot.project_id != project_id
        || snapshot.source_revision != revision
    {
        return Err(
            "Historical snapshot schema, project identity, or revision is incompatible."
                .to_string(),
        );
    }
    Ok(Some(snapshot))
}

pub fn save(root: &Path, snapshot: &HistoricalSnapshot) -> Result<(), String> {
    if snapshot.schema_version != HISTORICAL_SNAPSHOT_SCHEMA
        || snapshot.project_id != crate::graph::project_id(root)
    {
        return Err("Historical snapshot cannot be stored for this repository.".to_string());
    }
    let path = snapshot_path(root, &snapshot.source_revision)?;
    let directory = path
        .parent()
        .ok_or_else(|| "Historical snapshot path has no parent.".to_string())?;
    fs::create_dir_all(directory)
        .map_err(|error| format!("Unable to create historical snapshot directory: {error}"))?;
    let payload = serde_json::to_vec_pretty(snapshot)
        .map_err(|error| format!("Unable to encode historical snapshot: {error}"))?;
    if payload.len() as u64 > MAX_SNAPSHOT_BYTES {
        return Err("Historical snapshot exceeds the metadata bound.".to_string());
    }
    atomic_write(&path, &payload)
}

fn snapshot_path(root: &Path, revision: &str) -> Result<PathBuf, String> {
    if revision.is_empty()
        || revision.len() > 128
        || revision.bytes().any(|byte| !byte.is_ascii_hexdigit())
    {
        return Err("Historical snapshot revision must be a hexadecimal Git revision.".to_string());
    }
    Ok(root
        .join(crate::store::STORE_DIRECTORY)
        .join("diagnostics")
        .join(HISTORY_DIRECTORY)
        .join(format!("{revision}.json")))
}

fn atomic_write(path: &Path, payload: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "Historical snapshot path has no parent.".to_string())?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let temporary = parent.join(format!(
        ".{}.tmp.{}-{}",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("snapshot"),
        std::process::id(),
        nonce
    ));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .map_err(|error| format!("Unable to create historical snapshot temporary file: {error}"))?;
    file.write_all(payload)
        .and_then(|_| file.sync_all())
        .map_err(|error| format!("Unable to flush historical snapshot: {error}"))?;
    drop(file);
    match fs::rename(&temporary, path) {
        Ok(()) => Ok(()),
        Err(rename_error) if path.exists() => {
            fs::remove_file(path).map_err(|error| {
                format!(
                    "Unable to replace historical snapshot after atomic rename failure ({rename_error}): {error}"
                )
            })?;
            fs::rename(&temporary, path).map_err(|error| {
                format!("Unable to finalize historical snapshot replacement: {error}")
            })
        }
        Err(error) => {
            let _ = fs::remove_file(&temporary);
            Err(format!(
                "Unable to atomically write historical snapshot: {error}"
            ))
        }
    }
}
