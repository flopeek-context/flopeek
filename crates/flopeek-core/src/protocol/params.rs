//! Protocol parameter normalization and project-root validation.

use crate::model::DiagnosticLimits;
use serde_json::Value;
use std::path::PathBuf;

pub(super) fn limits_from_params(params: &Value) -> DiagnosticLimits {
    let mut limits = DiagnosticLimits::default();
    let Some(value) = params.get("limits") else {
        return limits;
    };
    if let Some(number) = value.get("maxCommits").and_then(Value::as_u64) {
        limits.max_commits = number as usize;
    }
    if let Some(number) = value.get("maxCandidates").and_then(Value::as_u64) {
        limits.max_candidates = number as usize;
    }
    if let Some(number) = value.get("maxPaths").and_then(Value::as_u64) {
        limits.max_paths = number as usize;
    }
    if let Some(number) = value.get("maxContextRefs").and_then(Value::as_u64) {
        limits.max_context_refs = number as usize;
    }
    if let Some(number) = value.get("maxAssertions").and_then(Value::as_u64) {
        limits.max_assertions = number as usize;
    }
    if let Some(number) = value.get("maxSnapshotBytes").and_then(Value::as_u64) {
        limits.max_snapshot_bytes = number as usize;
    }
    if let Some(number) = value.get("maxPacketBytes").and_then(Value::as_u64) {
        limits.max_packet_bytes = number as usize;
    }
    limits
}

pub(super) fn payload_value(params: &Value, key: &str) -> Value {
    if let Some(value) = params.get(key) {
        return value.clone();
    }
    let Some(object) = params.as_object() else {
        return params.clone();
    };
    let mut payload = object.clone();
    payload.remove("projectRoot");
    payload.remove("limits");
    Value::Object(payload)
}

pub(super) fn project_root(params: &Value) -> Result<PathBuf, String> {
    let value = params
        .get("projectRoot")
        .and_then(Value::as_str)
        .unwrap_or(".");
    let root = PathBuf::from(value);
    root.canonicalize()
        .map_err(|error| format!("Unable to resolve project root {}: {error}", root.display()))
}
