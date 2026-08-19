//! Temporal observation and exact Context Ref reconciliation contracts.

use super::ObservationBasisRelations;
use super::{ContextRef, GraphBasis, GraphEdge, GraphNode};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct ObservationContinuityEvent {
    pub event_id: String,
    pub project_id: String,
    pub observation_id: String,
    pub predecessor_event_id: Option<String>,
    pub relation: String,
    pub observed_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct ObservationContinuity {
    pub schema_version: String,
    pub project_id: String,
    pub current_observation_id: Option<String>,
    pub current_event_id: Option<String>,
    pub current_basis: Option<GraphBasis>,
    pub events: Vec<ObservationContinuityEvent>,
    pub graph_relation: String,
    pub truncated: bool,
    pub omissions: Vec<String>,
    pub limitations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct ContextReconciliation {
    pub schema_version: String,
    pub reference: ContextRef,
    pub evaluation_event_id: Option<String>,
    pub status: String,
    pub reason: String,
    pub successor: Option<String>,
    pub candidates: Vec<String>,
    pub truncated: bool,
    pub omissions: Vec<String>,
    pub limitations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct HistoricalPathChange {
    pub path: String,
    pub status: String,
    pub before_hash: Option<String>,
    pub after_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct HistoricalNodeChange {
    pub node_id: String,
    pub status: String,
    pub before: Option<GraphNode>,
    pub after: Option<GraphNode>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct HistoricalEdgeChange {
    pub status: String,
    pub edge: GraphEdge,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct HistoricalFlowChange {
    pub flow_id: String,
    pub status: String,
    pub before_fingerprint: Option<String>,
    pub after_fingerprint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct HistoricalContinuityCounts {
    pub path_changes: usize,
    pub node_changes: usize,
    pub edge_changes: usize,
    pub flow_changes: usize,
    pub lineage_candidates: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct HistoricalContextContinuity {
    pub schema_version: String,
    pub project_id: String,
    pub reference_uri: String,
    pub status: String,
    pub reason: String,
    pub relation: String,
    pub from_revision: Option<String>,
    pub to_revision: Option<String>,
    pub origin_basis: Option<GraphBasis>,
    pub from_basis: Option<GraphBasis>,
    pub to_basis: Option<GraphBasis>,
    pub basis_relations: ObservationBasisRelations,
    pub node_status: String,
    pub fingerprint_relation: String,
    pub path_changes: Vec<HistoricalPathChange>,
    pub node_changes: Vec<HistoricalNodeChange>,
    pub edge_changes: Vec<HistoricalEdgeChange>,
    pub flow_changes: Vec<HistoricalFlowChange>,
    pub lineage_candidates: Vec<String>,
    pub counts: HistoricalContinuityCounts,
    pub truncated: bool,
    pub omissions: Vec<String>,
    pub limitations: Vec<String>,
}
