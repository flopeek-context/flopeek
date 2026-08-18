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
    load_with_key(root, revision, "legacy", "legacy", usize::MAX, usize::MAX)
}

pub fn load_with_key(
    root: &Path,
    revision: &str,
    derivation_id: &str,
    parser_id: &str,
    max_paths: usize,
    max_snapshot_bytes: usize,
) -> Result<Option<HistoricalSnapshot>, String> {
    let path = snapshot_path(
        root,
        revision,
        derivation_id,
        parser_id,
        max_paths,
        max_snapshot_bytes,
    )?;
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
    if snapshot.files.len() > max_paths || snapshot_json_size(&snapshot)? > max_snapshot_bytes {
        return Ok(None);
    }
    Ok(Some(snapshot))
}

pub fn save(root: &Path, snapshot: &HistoricalSnapshot) -> Result<(), String> {
    save_with_key(root, snapshot, "legacy", "legacy", usize::MAX, usize::MAX)
}

pub fn save_with_key(
    root: &Path,
    snapshot: &HistoricalSnapshot,
    derivation_id: &str,
    parser_id: &str,
    max_paths: usize,
    max_snapshot_bytes: usize,
) -> Result<(), String> {
    if snapshot.schema_version != HISTORICAL_SNAPSHOT_SCHEMA
        || snapshot.project_id != crate::graph::project_id(root)
    {
        return Err("Historical snapshot cannot be stored for this repository.".to_string());
    }
    let path = snapshot_path(
        root,
        &snapshot.source_revision,
        derivation_id,
        parser_id,
        max_paths,
        max_snapshot_bytes,
    )?;
    let directory = path
        .parent()
        .ok_or_else(|| "Historical snapshot path has no parent.".to_string())?;
    fs::create_dir_all(directory)
        .map_err(|error| format!("Unable to create historical snapshot directory: {error}"))?;
    let payload = serde_json::to_vec_pretty(snapshot)
        .map_err(|error| format!("Unable to encode historical snapshot: {error}"))?;
    if payload.len() > max_snapshot_bytes || payload.len() as u64 > MAX_SNAPSHOT_BYTES {
        return Err("Historical snapshot exceeds the metadata bound.".to_string());
    }
    atomic_write(&path, &payload)
}

fn snapshot_path(
    root: &Path,
    revision: &str,
    derivation_id: &str,
    parser_id: &str,
    max_paths: usize,
    max_snapshot_bytes: usize,
) -> Result<PathBuf, String> {
    if revision.is_empty()
        || revision.len() > 128
        || revision.bytes().any(|byte| !byte.is_ascii_hexdigit())
    {
        return Err("Historical snapshot revision must be a hexadecimal Git revision.".to_string());
    }
    let key = format!(
        "flopeek-history-cache-v2\\0{revision}\\0{derivation_id}\\0{parser_id}\\0{max_paths}\\0{max_snapshot_bytes}"
    );
    let cache_id = blake3::hash(key.as_bytes()).to_hex().to_string();
    Ok(root
        .join(crate::store::STORE_DIRECTORY)
        .join("diagnostics")
        .join(HISTORY_DIRECTORY)
        .join(format!("{revision}-{cache_id}.json")))
}

fn snapshot_json_size(snapshot: &HistoricalSnapshot) -> Result<usize, String> {
    serde_json::to_vec(snapshot)
        .map(|payload| payload.len())
        .map_err(|error| format!("Unable to measure historical snapshot: {error}"))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_parser_identity_is_part_of_the_history_cache_namespace() {
        let root =
            std::env::temp_dir().join(format!("flopeek-history-store-test-{}", std::process::id()));
        if root.exists() {
            fs::remove_dir_all(&root).expect("remove stale test root");
        }
        fs::create_dir_all(&root).expect("test root");
        let snapshot = HistoricalSnapshot {
            schema_version: HISTORICAL_SNAPSHOT_SCHEMA.to_string(),
            project_id: crate::graph::project_id(&root),
            source_revision: "a".repeat(40),
            files: Vec::new(),
            nodes: Vec::new(),
            edges: Vec::new(),
            resolution_evidence: crate::model::ResolutionEvidence::default(),
            module_resolution: crate::model::ModuleResolutionBasis::default(),
            truncated: false,
            omissions: Vec::new(),
        };
        let derivation = "typescript-historical-delta-v2";
        let old_parser = "tree-sitter-typescript-0.23.2";
        save_with_key(&root, &snapshot, derivation, old_parser, 10, 1024)
            .expect("save old parser namespace");
        assert!(
            load_with_key(
                &root,
                &snapshot.source_revision,
                derivation,
                crate::typescript::PARSER_IDENTITY,
                10,
                1024,
            )
            .expect("load exact parser namespace before migration")
            .is_none()
        );
        save_with_key(
            &root,
            &snapshot,
            derivation,
            crate::typescript::PARSER_IDENTITY,
            10,
            1024,
        )
        .expect("save exact parser namespace");
        assert!(
            load_with_key(
                &root,
                &snapshot.source_revision,
                derivation,
                crate::typescript::PARSER_IDENTITY,
                10,
                1024,
            )
            .expect("load exact parser namespace")
            .is_some()
        );
        assert!(
            load_with_key(
                &root,
                &snapshot.source_revision,
                crate::graph::GRAPH_DERIVATION_ID,
                crate::typescript::PARSER_IDENTITY,
                10,
                1024,
            )
            .expect("reject prior derivation namespace")
            .is_none()
        );
        fs::remove_dir_all(&root).expect("cleanup test root");
    }
}
