use super::*;
use crate::model::PROTOCOL_SCHEMA;
use serde_json::{Value, json};
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::Command;
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

fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .expect("git");
    assert!(output.status.success(), "git {args:?}");
}

fn serve_one(root: &Path, id: usize, method: &str, params: Value) -> Value {
    let mut params = params.as_object().cloned().unwrap_or_default();
    params.insert(
        "projectRoot".to_string(),
        Value::String(root.to_string_lossy().into_owned()),
    );
    let request = jsonl_request(id, method, Value::Object(params));
    let mut output = Vec::new();
    serve_jsonl(Cursor::new(format!("{request}\n")), &mut output).expect("serve request");
    serde_json::from_slice(&output).expect("response")
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
    assert_eq!(
        response["result"]["productIdentity"],
        "versioned-repository-context"
    );
    assert_eq!(response["result"]["graphRole"], "deterministic-substrate");
    assert_eq!(response["result"]["languageCountIsProductGoal"], false);
    assert_eq!(response["result"]["reviewGraphIsPrimaryProduct"], false);
    assert_eq!(
        response["result"]["graphIdentityBasis"],
        "typescript-context-structural-evidence"
    );
    assert_eq!(
        response["result"]["observationContinuity"],
        "immutable-scan-event-chain"
    );
    assert_eq!(
        response["result"]["automaticSupersession"],
        "disabled-without-lineage-proof"
    );
    assert_eq!(
        response["result"]["lastKnownGoodModel"],
        "immutable-candidate-append-only-event-reduced-state"
    );
    assert_eq!(
        response["result"]["lastKnownGoodLifecycle"],
        "protocol-1.0-deterministic-reducer"
    );
    assert_eq!(
        response["result"]["lastKnownGoodTrust"],
        "local-transition-boundary-caller-attributed"
    );
    assert_eq!(
        response["result"]["structuralChangeAttribution"],
        "adjacent-observation-compatible-evidence"
    );
    assert_eq!(
        response["result"]["repositoryIdentity"],
        "explicit-versioned-root-manifest"
    );
    assert_eq!(
        response["result"]["lastKnownGood"],
        "attributed-human-confirmation"
    );
    assert_eq!(
        response["result"]["lastKnownGoodLifecycle"],
        "protocol-1.0-deterministic-reducer"
    );
    assert_eq!(
        response["result"]["lastKnownGoodProvenance"],
        "revision-observation-graph-consistent"
    );
    assert_eq!(
        response["result"]["humanActorIdentity"],
        "caller-attributed-not-authenticated"
    );
}

#[test]
fn last_known_good_jsonl_methods_round_trip_with_human_confirmation() {
    let root = temp_root();
    fs::write(
        root.join(crate::identity::MANIFEST_PATH),
        r#"{"schemaVersion":"flopeek-repository-identity/v1","repositoryId":"repo_123e4567-e89b-12d3-a456-426614174000"}"#,
    )
    .expect("identity manifest");
    git(&root, &["init"]);
    git(
        &root,
        &["config", "user.email", "flopeek-test@example.invalid"],
    );
    git(&root, &["config", "user.name", "Flopeek Test"]);
    git(&root, &["add", "."]);
    git(&root, &["commit", "-m", "baseline"]);
    let revision = String::from_utf8_lossy(
        &Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(["rev-parse", "HEAD"])
            .output()
            .expect("revision")
            .stdout,
    )
    .trim()
    .to_string();
    let scan = serve_one(&root, 1, "scan", json!({}));
    assert_eq!(scan["ok"], true);
    let graph = scan["result"]["graph"].clone();
    let basis = json!({
        "projectId": graph["project_id"],
        "graphId": graph["graph_id"],
        "graphVersion": graph["graph_version"],
        "sourceRevision": graph["source_revision"],
        "observationId": graph["observation_id"]
    });
    let context = json!({
        "schemaVersion": "flopeek-diagnostic-context/v7",
        "id": "jsonl-lkg-context",
        "projectId": scan["result"]["project_id"],
        "contextDefinitionRevision": 0,
        "contextBasisFingerprint": "",
        "memoryRevision": 0,
        "intent": "diagnose",
        "symptom": "timeout",
        "expectedBehavior": "completes",
        "focusContextRefs": [scan["result"]["context_refs"][0]["uri"]],
        "focusFlowRefs": [],
        "currentGraphBasis": basis,
        "lastKnownGoodBasis": null,
        "lastKnownGoodBindingId": null,
        "lastKnownGoodCandidateId": null,
        "constraints": [],
        "acceptanceCriteria": [],
        "unresolvedQuestions": [],
        "actor": "jsonl-test",
        "createdAt": 0,
        "status": "open",
        "supersedes": null
    });
    let created = serve_one(
        &root,
        2,
        "createDiagnosticContext",
        json!({"context": context}),
    );
    assert_eq!(created["ok"], true);
    let blocked = serve_one(
        &root,
        3,
        "createLastKnownGoodBinding",
        json!({"binding": {}}),
    );
    assert_eq!(blocked["ok"], false);
    assert_eq!(blocked["error"]["message"], "legacy-lkg-write-disabled");
    let proposed = serve_one(
        &root,
        4,
        "proposeLastKnownGood",
        json!({
            "contextId": "jsonl-lkg-context",
            "gitRevision": revision,
            "actor": "agent",
            "reason": "fixture proposal",
            "evidence": [],
            "expectedTipEventId": null,
            "idempotencyKey": "jsonl-proposal-1"
        }),
    );
    assert_eq!(proposed["ok"], true);
    assert_eq!(proposed["result"]["integrity"]["status"], "complete");
    let current = serve_one(
        &root,
        5,
        "getLastKnownGoodProtocol",
        json!({"contextId": "jsonl-lkg-context"}),
    );
    assert_eq!(current["result"]["lifecycleStatus"], "pending");
    let canonical_get = serve_one(
        &root,
        50,
        "getLastKnownGood",
        json!({"contextId": "jsonl-lkg-context"}),
    );
    assert_eq!(canonical_get["result"]["lifecycleStatus"], "pending");
    let canonical_history = serve_one(
        &root,
        51,
        "listLastKnownGoodHistory",
        json!({"contextId": "jsonl-lkg-context"}),
    );
    assert_eq!(
        canonical_history["result"]["events"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
    let canonical_validation = serve_one(
        &root,
        52,
        "validateLastKnownGood",
        json!({"contextId": "jsonl-lkg-context"}),
    );
    assert_eq!(canonical_validation["result"]["lifecycleStatus"], "pending");
    let review = serve_one(
        &root,
        6,
        "getLastKnownGoodReviewPacket",
        json!({"contextId": "jsonl-lkg-context"}),
    );
    assert_eq!(review["ok"], true);
    assert_eq!(review["result"]["confirmable"], true);
    let transition = serve_one(
        &root,
        7,
        "confirmLastKnownGood",
        json!({"contextId": "jsonl-lkg-context", "actorKind": "human"}),
    );
    assert_eq!(transition["ok"], false);
    assert_eq!(
        transition["error"]["message"],
        "lkg-human-transition-requires-trusted-local-cli"
    );
    fs::remove_dir_all(root).expect("cleanup");
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
fn historical_continuity_jsonl_method_reports_dirty_history_explicitly() {
    let root = temp_root();
    let scan_line = format!(
        "{}\n",
        jsonl_request(1, "scan", json!({"projectRoot": root.to_string_lossy()}),)
    );
    let mut scan_output = Vec::new();
    serve_jsonl(Cursor::new(scan_line), &mut scan_output).expect("scan serve");
    let scan_response: Value = serde_json::from_slice(&scan_output).expect("scan json");
    let uri = scan_response["result"]["context_refs"][0]["uri"]
        .as_str()
        .expect("context uri");
    let request = jsonl_request(
        2,
        "getHistoricalContextContinuity",
        json!({"projectRoot": root.to_string_lossy(), "uri": uri}),
    );
    let mut output = Vec::new();
    serve_jsonl(Cursor::new(format!("{request}\n")), &mut output).expect("continuity serve");
    let response: Value = serde_json::from_slice(&output).expect("continuity json");
    assert_eq!(response["ok"], true);
    assert_eq!(response["result"]["status"], "unavailable");
    assert_eq!(
        response["result"]["reason"],
        "historical-continuity-unavailable-for-dirty-source"
    );
    fs::remove_dir_all(root).expect("cleanup");
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
        "schemaVersion": "flopeek-diagnostic-context/v7",
        "id": "jsonl-flow-context",
        "projectId": scan["project_id"],
        "contextDefinitionRevision": 0,
        "contextBasisFingerprint": "",
        "memoryRevision": 0,
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
        jsonl_request(
            4,
            "getFlow",
            json!({"projectRoot": root.to_string_lossy(), "flowId": flow_id}),
        ),
        jsonl_request(
            5,
            "getRelatedTests",
            json!({"projectRoot": root.to_string_lossy(), "flowId": graph["flows"][0]["flowId"]}),
        ),
        jsonl_request(
            6,
            "resolveFlowRef",
            json!({"projectRoot": root.to_string_lossy(), "uri": flow_uri}),
        ),
        jsonl_request(
            7,
            "resolveContextRef",
            json!({"projectRoot": root.to_string_lossy(), "uri": node_uri}),
        ),
        jsonl_request(
            8,
            "getObservationContinuity",
            json!({"projectRoot": root.to_string_lossy(), "maxEvents": 0}),
        ),
        jsonl_request(
            9,
            "reconcileContextRef",
            json!({"projectRoot": root.to_string_lossy(), "uri": node_uri}),
        ),
        jsonl_request(
            10,
            "getNode",
            json!({"projectRoot": root.to_string_lossy(), "nodeId": node_id}),
        ),
        jsonl_request(
            11,
            "createDiagnosticContext",
            json!({"projectRoot": root.to_string_lossy(), "context": context}),
        ),
        jsonl_request(
            12,
            "getObservationDelta",
            json!({"projectRoot": root.to_string_lossy(), "maxNodeChanges": 0}),
        ),
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
    assert_eq!(responses.len(), 11);
    assert!(responses.iter().all(|response| response["ok"] == true));
    assert_eq!(
        responses[2]["result"]["flowId"],
        graph["flows"][0]["flowId"]
    );
    assert_eq!(responses[4]["result"]["status"], "current");
    assert_eq!(responses[5]["result"]["status"], "current");
    assert_eq!(
        responses[6]["result"]["schemaVersion"],
        "flopeek-observation-continuity/v2"
    );
    assert_eq!(responses[6]["result"]["truncated"], true);
    assert_eq!(
        responses[7]["result"]["schemaVersion"],
        "flopeek-context-reconciliation/v2"
    );
    assert_eq!(responses[7]["result"]["status"], "current");
    assert_eq!(responses[8]["result"]["node"]["id"], node_id);
    assert_eq!(
        responses[10]["result"]["schemaVersion"],
        "flopeek-observation-delta/v2"
    );
    assert_eq!(responses[10]["result"]["status"], "unavailable");
    let context_id = responses[9]["result"]["id"]
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
    serve_jsonl(Cursor::new(diagnosis_requests), &mut diagnosis_output).expect("diagnosis methods");
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
    assert_eq!(
        diagnosis_responses[1]["result"]["contextReconciliation"]
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
