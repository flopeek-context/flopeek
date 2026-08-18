//! Small JSONL boundary over the same Rust/SQLite authority used by the CLI.

use crate::graph;
use crate::model::{PRODUCT_IDENTITY, PROTOCOL_SCHEMA, ScanResult};
use crate::store;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
struct Request {
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Serialize)]
struct ErrorBody {
    code: String,
    message: String,
}

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

pub fn serve_jsonl<R: BufRead, W: Write>(reader: R, mut writer: W) -> Result<(), String> {
    for line in reader.lines() {
        let line = line.map_err(|error| format!("Unable to read JSONL request: {error}"))?;
        if line.trim().is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<Request>(&line) {
            Ok(request) => handle_request(request),
            Err(error) => json!({
                "schemaVersion": PROTOCOL_SCHEMA,
                "ok": false,
                "error": ErrorBody { code: "invalid-request".to_string(), message: error.to_string() },
            }),
        };
        serde_json::to_writer(&mut writer, &response)
            .map_err(|error| format!("Unable to write JSONL response: {error}"))?;
        writer
            .write_all(b"\n")
            .map_err(|error| format!("Unable to terminate JSONL response: {error}"))?;
        writer
            .flush()
            .map_err(|error| format!("Unable to flush JSONL response: {error}"))?;
    }
    Ok(())
}

fn handle_request(request: Request) -> Value {
    let id = request.id.unwrap_or(Value::Null);
    match handle_method(&request.method, &request.params) {
        Ok(result) => json!({
            "schemaVersion": PROTOCOL_SCHEMA,
            "id": id,
            "ok": true,
            "result": result,
        }),
        Err(message) => json!({
            "schemaVersion": PROTOCOL_SCHEMA,
            "id": id,
            "ok": false,
            "error": ErrorBody { code: "request-failed".to_string(), message },
        }),
    }
}

fn handle_method(method: &str, params: &Value) -> Result<Value, String> {
    match method {
        "health" => Ok(json!({
            "product": PRODUCT_IDENTITY,
            "core": "rust",
            "analyzedLanguages": ["typescript", "tsx"],
            "persistedAuthority": "sqlite",
            "llmRequired": false,
        })),
        "scan" => {
            let root = project_root(params)?;
            serde_json::to_value(scan_project(&root)?).map_err(|error| error.to_string())
        }
        "status" => {
            let root = project_root(params)?;
            status_project(&root)
        }
        "getGraph" => {
            let root = project_root(params)?;
            serde_json::to_value(store::current_graph(&root)?).map_err(|error| error.to_string())
        }
        "getNode" => {
            let root = project_root(params)?;
            let node_id = params
                .get("nodeId")
                .and_then(Value::as_str)
                .ok_or_else(|| "getNode requires params.nodeId.".to_string())?;
            store::node_details(&root, node_id)
        }
        "resolveContextRef" => {
            let root = project_root(params)?;
            let uri = params
                .get("uri")
                .and_then(Value::as_str)
                .ok_or_else(|| "resolveContextRef requires params.uri.".to_string())?;
            serde_json::to_value(store::resolve_context(&root, uri)?)
                .map_err(|error| error.to_string())
        }
        _ => Err(format!("Unsupported protocol method: {method}")),
    }
}

fn project_root(params: &Value) -> Result<PathBuf, String> {
    let value = params
        .get("projectRoot")
        .and_then(Value::as_str)
        .unwrap_or(".");
    let root = PathBuf::from(value);
    root.canonicalize()
        .map_err(|error| format!("Unable to resolve project root {}: {error}", root.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn jsonl_health_is_rust_only_and_deterministic() {
        let input = Cursor::new(
            br#"{"id":1,"method":"health","params":{}}
"#,
        );
        let mut output = Vec::new();
        serve_jsonl(input, &mut output).expect("serve");
        let response: Value = serde_json::from_slice(&output).expect("json");
        assert_eq!(response["ok"], true);
        assert_eq!(response["result"]["core"], "rust");
        assert_eq!(response["result"]["analyzedLanguages"][0], "typescript");
    }

    #[test]
    fn invalid_jsonl_is_an_explicit_error() {
        let input = Cursor::new(b"not-json\n");
        let mut output = Vec::new();
        serve_jsonl(input, &mut output).expect("serve");
        let response: Value = serde_json::from_slice(&output).expect("json");
        assert_eq!(response["ok"], false);
        assert_eq!(response["error"]["code"], "invalid-request");
    }
}
