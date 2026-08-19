use serde::{Deserialize, Serialize};

use super::{
    ContextFlow, EntryEvidence, FlowRef, GraphBasis, ModuleResolutionBasis, RelatedTestEvidence,
    ResolutionEvidence, SourceFile,
};
use crate::identity::IdentityBasis;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GraphNode {
    pub id: String,
    pub kind: String,
    pub path: Option<String>,
    pub name: Option<String>,
    pub language: Option<String>,
    #[serde(default)]
    pub evidence_fingerprint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct GraphEdge {
    pub from: String,
    pub to: String,
    pub kind: String,
    pub evidence: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GraphSnapshot {
    pub schema_version: String,
    pub product: String,
    pub project_id: String,
    pub graph_id: String,
    pub graph_version: u64,
    pub source_revision: String,
    #[serde(default)]
    pub source_fingerprint: String,
    #[serde(default)]
    pub observation_id: String,
    #[serde(default)]
    pub identity_basis: IdentityBasis,
    pub files: Vec<SourceFile>,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    #[serde(default)]
    pub resolution_evidence: ResolutionEvidence,
    #[serde(default)]
    pub module_resolution: ModuleResolutionBasis,
    #[serde(default)]
    pub entry_evidence: EntryEvidence,
    #[serde(default)]
    pub related_test_evidence: RelatedTestEvidence,
    #[serde(default)]
    pub flows: Vec<ContextFlow>,
    pub truncated: bool,
    pub omissions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContextRef {
    pub schema_version: String,
    pub uri: String,
    pub project_id: String,
    pub graph_id: String,
    pub graph_version: u64,
    pub node_id: String,
    pub status: String,
    #[serde(default)]
    pub origin_observation_id: String,
    #[serde(default)]
    pub origin_source_revision: String,
    #[serde(default)]
    pub origin_fingerprint: String,
    #[serde(default)]
    pub fingerprint_scope: String,
    #[serde(default)]
    pub fingerprint_contract: String,
    #[serde(default)]
    pub freshness_reason: String,
    #[serde(default)]
    pub origin_basis: Option<GraphBasis>,
    #[serde(default)]
    pub current_basis: Option<GraphBasis>,
    #[serde(default)]
    pub current_event_id: String,
    #[serde(default)]
    pub successor_uri: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoreStatus {
    pub schema_version: String,
    pub path: String,
    pub project_id: String,
    pub current_graph_id: Option<String>,
    pub current_graph_version: Option<u64>,
    pub graph_count: u64,
    pub node_count: u64,
    pub edge_count: u64,
    #[serde(default)]
    pub current_observation_id: Option<String>,
    #[serde(default)]
    pub identity_basis: Option<crate::identity::IdentityBasis>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScanResult {
    pub schema_version: String,
    pub product: String,
    pub project_id: String,
    pub graph: GraphSnapshot,
    #[serde(default)]
    pub identity_basis: IdentityBasis,
    pub context_refs: Vec<ContextRef>,
    #[serde(default)]
    pub flow_refs: Vec<FlowRef>,
    pub limitations: Vec<String>,
}
