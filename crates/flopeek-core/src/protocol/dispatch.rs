//! JSONL request decoding and method dispatch.

use crate::diagnostic;
use crate::model::{DiagnosticAssertion, DiagnosticContext, PRODUCT_IDENTITY, PROTOCOL_SCHEMA};
use crate::store;
use crate::temporal::DeltaLimits;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::orchestration::{scan_project, status_project};
use super::params::{limits_from_params, payload_value, project_root};

#[derive(Debug, Deserialize)]
pub(super) struct Request {
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Serialize)]
pub(super) struct ErrorBody {
    pub(super) code: String,
    pub(super) message: String,
}

pub(super) fn handle_request(request: Request) -> Value {
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
            "productIdentity": "versioned-repository-context",
            "graphRole": "deterministic-substrate",
            "languageCountIsProductGoal": false,
            "reviewGraphIsPrimaryProduct": false,
            "graphIdentityBasis": "typescript-context-structural-evidence",
            "sourceBasis": "immutable-graph-observation",
            "contextFreshness": "node-ast-and-direct-edges",
            "flowEvidenceBasis": "root-package-manifest-and-static-call-projection",
            "flowFreshness": "entry-step-evidence-and-traversed-edges",
            "relatedTestEvidence": "direct-call-construct-or-import",
            "observationContinuity": "immutable-scan-event-chain",
            "contextReconciliation": "exact-compatible-fingerprint-candidates",
            "automaticSupersession": "disabled-without-lineage-proof",
            "structuralChangeAttribution": "adjacent-observation-compatible-evidence",
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
        "getObservationContinuity" => {
            let root = project_root(params)?;
            let max_events = params
                .get("maxEvents")
                .and_then(Value::as_u64)
                .unwrap_or(128)
                .min(usize::MAX as u64) as usize;
            serde_json::to_value(store::get_observation_continuity(&root, max_events)?)
                .map_err(|error| error.to_string())
        }
        "getObservationDelta" => {
            let root = project_root(params)?;
            let event_id = params.get("eventId").and_then(Value::as_str);
            let defaults = DeltaLimits::default();
            let limits = DeltaLimits {
                max_source_changes: bounded_delta_limit(
                    params,
                    "maxSourceChanges",
                    defaults.max_source_changes,
                    1_000,
                ),
                max_node_changes: bounded_delta_limit(
                    params,
                    "maxNodeChanges",
                    defaults.max_node_changes,
                    2_000,
                ),
                max_edge_changes: bounded_delta_limit(
                    params,
                    "maxEdgeChanges",
                    defaults.max_edge_changes,
                    4_000,
                ),
                max_flow_changes: bounded_delta_limit(
                    params,
                    "maxFlowChanges",
                    defaults.max_flow_changes,
                    512,
                ),
            };
            serde_json::to_value(store::get_observation_delta(&root, event_id, limits)?)
                .map_err(|error| error.to_string())
        }
        "reconcileContextRef" => {
            let root = project_root(params)?;
            let uri = params
                .get("uri")
                .and_then(Value::as_str)
                .ok_or_else(|| "reconcileContextRef requires params.uri.".to_string())?;
            serde_json::to_value(store::reconcile_context(&root, uri)?)
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

fn bounded_delta_limit(params: &Value, key: &str, default: usize, maximum: usize) -> usize {
    params
        .get(key)
        .and_then(Value::as_u64)
        .map_or(default, |value| (value as usize).min(maximum))
}
