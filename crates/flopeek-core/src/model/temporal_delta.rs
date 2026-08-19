//! Bounded, source-body-free structural differences between adjacent observations.

use super::{GraphBasis, GraphEdge, GraphNode};
use serde::{Deserialize, Serialize};

pub const OBSERVATION_DELTA_SCHEMA: &str = "flopeek-observation-delta/v2";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceContract {
    pub graph_schema_version: String,
    pub graph_derivation_id: String,
    pub node_fingerprint_contract: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct ObservationBasisRelations {
    pub typescript_source: String,
    pub module_resolution_exact: String,
    pub module_resolution_effective: String,
    pub entry_manifest_exact: String,
    pub entry_manifest_effective: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct SourceChange {
    pub path: String,
    pub status: String,
    pub before_hash: Option<String>,
    pub after_hash: Option<String>,
    pub before_bytes: Option<u64>,
    pub after_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct NodeChange {
    pub node_id: String,
    pub status: String,
    pub before: Option<GraphNode>,
    pub after: Option<GraphNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct EdgeChange {
    pub status: String,
    pub edge: GraphEdge,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct FlowChange {
    pub flow_id: String,
    pub status: String,
    pub before_fingerprint: Option<String>,
    pub after_fingerprint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct ObservationDeltaCounts {
    pub source_added: usize,
    pub source_changed: usize,
    pub source_removed: usize,
    pub node_added: usize,
    pub node_changed: usize,
    pub node_removed: usize,
    pub edge_added: usize,
    pub edge_removed: usize,
    pub flow_added: usize,
    pub flow_changed: usize,
    pub flow_removed: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct ObservationDelta {
    pub schema_version: String,
    pub project_id: String,
    pub status: String,
    pub reason: String,
    pub from_event_id: Option<String>,
    pub to_event_id: Option<String>,
    pub relation: String,
    pub from_basis: Option<GraphBasis>,
    pub to_basis: Option<GraphBasis>,
    pub from_contract: Option<EvidenceContract>,
    pub to_contract: Option<EvidenceContract>,
    pub contract_compatible: bool,
    pub graph_relation: String,
    pub basis_relations: ObservationBasisRelations,
    pub counts: ObservationDeltaCounts,
    pub source_changes: Vec<SourceChange>,
    pub node_changes: Vec<NodeChange>,
    pub edge_changes: Vec<EdgeChange>,
    pub flow_changes: Vec<FlowChange>,
    pub truncated: bool,
    pub omissions: Vec<String>,
    pub limitations: Vec<String>,
}
