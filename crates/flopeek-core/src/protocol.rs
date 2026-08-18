//! Small JSONL boundary over the same Rust/SQLite authority used by the CLI.

use crate::diagnostic;
use crate::graph;
use crate::model::{
    DiagnosticAssertion, DiagnosticContext, DiagnosticLimits, PRODUCT_IDENTITY, PROTOCOL_SCHEMA,
    ScanResult,
};
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
            "diagnosticMetadataAuthority": "sqlite",
            "llmRequired": false,
            "graphIdentityBasis": "typescript-structural-evidence",
            "sourceBasis": "immutable-graph-observation",
            "contextFreshness": "node-ast-and-direct-edges",
            "flowEvidenceBasis": "root-package-manifest-and-static-call-projection",
            "flowFreshness": "entry-step-evidence-and-traversed-edges",
            "relatedTestEvidence": "direct-call-construct-or-import",
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
        "listFlows" => {
            let root = project_root(params)?;
            serde_json::to_value(store::list_flows(&root)?).map_err(|error| error.to_string())
        }
        "getFlow" => {
            let root = project_root(params)?;
            let flow_id = params
                .get("flowId")
                .and_then(Value::as_str)
                .ok_or_else(|| "getFlow requires params.flowId.".to_string())?;
            serde_json::to_value(store::get_flow(&root, flow_id)?)
                .map_err(|error| error.to_string())
        }
        "resolveFlowRef" => {
            let root = project_root(params)?;
            let uri = params
                .get("uri")
                .and_then(Value::as_str)
                .ok_or_else(|| "resolveFlowRef requires params.uri.".to_string())?;
            serde_json::to_value(store::resolve_flow(&root, uri)?)
                .map_err(|error| error.to_string())
        }
        "getRelatedTests" => {
            let root = project_root(params)?;
            let node_id = params.get("nodeId").and_then(Value::as_str);
            let flow_id = params.get("flowId").and_then(Value::as_str);
            serde_json::to_value(store::related_tests(&root, node_id, flow_id)?)
                .map_err(|error| error.to_string())
        }
        "createDiagnosticContext" => {
            let root = project_root(params)?;
            let value = payload_value(params, "context");
            let context = serde_json::from_value::<DiagnosticContext>(value)
                .map_err(|error| format!("Invalid Diagnostic Context: {error}"))?;
            serde_json::to_value(store::create_diagnostic_context(&root, context)?)
                .map_err(|error| error.to_string())
        }
        "getDiagnosticContext" => {
            let root = project_root(params)?;
            let id = params
                .get("contextId")
                .and_then(Value::as_str)
                .ok_or_else(|| "getDiagnosticContext requires params.contextId.".to_string())?;
            serde_json::to_value(store::get_diagnostic_context(&root, id)?)
                .map_err(|error| error.to_string())
        }
        "listDiagnosticAssertions" => {
            let root = project_root(params)?;
            let id = params
                .get("contextId")
                .and_then(Value::as_str)
                .ok_or_else(|| "listDiagnosticAssertions requires params.contextId.".to_string())?;
            serde_json::to_value(store::list_diagnostic_assertions(&root, id)?)
                .map_err(|error| error.to_string())
        }
        "appendDiagnosticAssertion" => {
            let root = project_root(params)?;
            let value = payload_value(params, "assertion");
            let assertion = serde_json::from_value::<DiagnosticAssertion>(value)
                .map_err(|error| format!("Invalid Diagnostic Assertion: {error}"))?;
            serde_json::to_value(store::append_diagnostic_assertion(&root, assertion)?)
                .map_err(|error| error.to_string())
        }
        "diagnoseHistory" => {
            let root = project_root(params)?;
            let context_id = params
                .get("contextId")
                .and_then(Value::as_str)
                .ok_or_else(|| "diagnoseHistory requires params.contextId.".to_string())?;
            serde_json::to_value(diagnostic::diagnose_history(
                &root,
                context_id,
                limits_from_params(params),
            )?)
            .map_err(|error| error.to_string())
        }
        "getDiagnosticPacket" => {
            let root = project_root(params)?;
            let context_id = params
                .get("contextId")
                .and_then(Value::as_str)
                .ok_or_else(|| "getDiagnosticPacket requires params.contextId.".to_string())?;
            serde_json::to_value(diagnostic::build_packet(
                &root,
                context_id,
                limits_from_params(params),
            )?)
            .map_err(|error| error.to_string())
        }
        _ => Err(format!("Unsupported protocol method: {method}")),
    }
}

fn limits_from_params(params: &Value) -> DiagnosticLimits {
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

fn payload_value(params: &Value, key: &str) -> Value {
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
    use std::fs;
    use std::io::Cursor;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root() -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("flopeek-protocol-{suffix}"));
        fs::create_dir_all(root.join("src")).expect("src");
        fs::create_dir_all(root.join("tests")).expect("tests");
        fs::write(
            root.join("package.json"),
            r#"{"scripts":{"start":"tsx src/main.ts","unsupported":"tsx src/main.ts && echo credential-sentinel"}}"#,
        )
        .expect("package");
        fs::write(
            root.join("src/main.ts"),
            "export function main() { return 'source-body-sentinel'; }\n",
        )
        .expect("main");
        fs::write(
            root.join("tests/main.test.ts"),
            "import { main } from '../src/main'; main();\n",
        )
        .expect("test");
        root
    }

    fn jsonl_request(id: usize, method: &str, params: Value) -> String {
        serde_json::to_string(&json!({
            "id": id,
            "method": method,
            "params": params,
        }))
        .expect("request")
    }

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
        assert_eq!(response["schemaVersion"], PROTOCOL_SCHEMA);
        assert_eq!(response["result"]["core"], "rust");
        assert_eq!(response["result"]["analyzedLanguages"][0], "typescript");
        assert_eq!(response["result"]["diagnosticMetadataAuthority"], "sqlite");
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

    #[test]
    fn flow_and_diagnostic_jsonl_methods_are_end_to_end_and_body_free() {
        let root = temp_root();
        let scan_line = format!(
            "{}\n",
            jsonl_request(1, "scan", json!({"projectRoot": root.to_string_lossy()}),)
        );
        let mut scan_output = Vec::new();
        serve_jsonl(Cursor::new(scan_line), &mut scan_output).expect("scan serve");
        let scan_response: Value = serde_json::from_slice(&scan_output).expect("scan json");
        assert_eq!(scan_response["ok"], true);
        let scan = &scan_response["result"];
        let graph = &scan["graph"];
        let flow_id = graph["flows"][0]["flowId"]
            .as_str()
            .expect("flow id")
            .to_string();
        let flow_uri = scan["flow_refs"][0]["uri"]
            .as_str()
            .expect("flow uri")
            .to_string();
        let node_uri = scan["context_refs"][0]["uri"]
            .as_str()
            .expect("node uri")
            .to_string();
        let node_id = graph["nodes"]
            .as_array()
            .and_then(|nodes| nodes.first())
            .and_then(|node| node["id"].as_str())
            .expect("node id")
            .to_string();
        let basis = json!({
            "projectId": graph["project_id"],
            "graphId": graph["graph_id"],
            "graphVersion": graph["graph_version"],
            "sourceRevision": graph["source_revision"],
            "observationId": graph["observation_id"],
        });
        let context = json!({
            "schemaVersion": "flopeek-diagnostic-context/v3",
            "id": "jsonl-flow-context",
            "projectId": scan["project_id"],
            "revision": 0,
            "intent": "diagnose",
            "symptom": "static flow changed",
            "expectedBehavior": "entry remains explicit",
            "focusContextRefs": [node_uri],
            "focusFlowRefs": [flow_uri.clone()],
            "currentGraphBasis": basis,
            "lastKnownGoodBasis": Value::Null,
            "constraints": ["Static evidence only"],
            "acceptanceCriteria": ["No runtime claim"],
            "unresolvedQuestions": ["Was the entry invoked?"],
            "actor": "jsonl-test",
            "createdAt": 0,
            "status": "open",
            "supersedes": Value::Null,
        });
        let root_param = json!({"projectRoot": root.to_string_lossy()});
        let requests = [
            jsonl_request(2, "getGraph", root_param.clone()),
            jsonl_request(3, "listFlows", root_param.clone()),
            jsonl_request(4, "getFlow", json!({"projectRoot": root.to_string_lossy(), "flowId": flow_id})),
            jsonl_request(5, "getRelatedTests", json!({"projectRoot": root.to_string_lossy(), "flowId": graph["flows"][0]["flowId"]})),
            jsonl_request(6, "resolveFlowRef", json!({"projectRoot": root.to_string_lossy(), "uri": flow_uri})),
            jsonl_request(7, "resolveContextRef", json!({"projectRoot": root.to_string_lossy(), "uri": node_uri})),
            jsonl_request(8, "getNode", json!({"projectRoot": root.to_string_lossy(), "nodeId": node_id})),
            jsonl_request(9, "createDiagnosticContext", json!({"projectRoot": root.to_string_lossy(), "context": context})),
        ]
        .join("\n")
            + "\n";
        let mut output = Vec::new();
        serve_jsonl(Cursor::new(requests), &mut output).expect("surface methods");
        let responses = String::from_utf8(output.clone())
            .expect("response bytes")
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).expect("response json"))
            .collect::<Vec<_>>();
        assert_eq!(responses.len(), 8);
        assert!(responses.iter().all(|response| response["ok"] == true));
        assert_eq!(
            responses[2]["result"]["flowId"],
            graph["flows"][0]["flowId"]
        );
        assert_eq!(responses[4]["result"]["status"], "current");
        assert_eq!(responses[5]["result"]["status"], "current");
        assert_eq!(responses[6]["result"]["node"]["id"], node_id);
        let context_id = responses[7]["result"]["id"]
            .as_str()
            .expect("context id")
            .to_string();
        let diagnosis_requests = [
            jsonl_request(
                10,
                "diagnoseHistory",
                json!({"projectRoot": root.to_string_lossy(), "contextId": context_id}),
            ),
            jsonl_request(
                11,
                "getDiagnosticPacket",
                json!({"projectRoot": root.to_string_lossy(), "contextId": "jsonl-flow-context"}),
            ),
        ]
        .join("\n")
            + "\n";
        let mut diagnosis_output = Vec::new();
        serve_jsonl(Cursor::new(diagnosis_requests), &mut diagnosis_output)
            .expect("diagnosis methods");
        let diagnosis_responses = String::from_utf8(diagnosis_output.clone())
            .expect("diagnosis response bytes")
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).expect("diagnosis json"))
            .collect::<Vec<_>>();
        assert!(
            diagnosis_responses
                .iter()
                .all(|response| response["ok"] == true)
        );
        assert_eq!(
            diagnosis_responses[1]["result"]["focusFlowRefs"]
                .as_array()
                .map(Vec::len),
            Some(1)
        );
        let all_output = format!(
            "{}{}",
            String::from_utf8_lossy(&output),
            String::from_utf8_lossy(&diagnosis_output)
        );
        assert!(!all_output.contains("source-body-sentinel"));
        assert!(!all_output.contains("credential-sentinel"));
        assert!(!all_output.contains(root.to_string_lossy().as_ref()));
        fs::remove_dir_all(root).expect("cleanup");
    }
}
