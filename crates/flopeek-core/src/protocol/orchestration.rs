//! Protocol orchestration over graph construction and SQLite persistence.

use crate::graph;
use crate::model::ScanResult;
use crate::store;
use serde_json::Value;
use std::path::Path;

pub fn scan_project(root: &Path) -> Result<ScanResult, String> {
    let root = root
        .canonicalize()
        .map_err(|error| format!("Unable to resolve project root {}: {error}", root.display()))?;
    if !root.is_dir() {
        return Err(format!(
            "Project root is not a directory: {}",
            root.display()
        ));
    }
    let (snapshot, facts) = graph::build(&root)?;
    store::persist_scan(&root, snapshot, &facts)
}

pub fn status_project(root: &Path) -> Result<Value, String> {
    serde_json::to_value(store::status(root)?).map_err(|error| error.to_string())
}
