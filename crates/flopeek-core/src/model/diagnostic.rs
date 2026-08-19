use serde::{Deserialize, Serialize};

use super::{
    ContextFlow, ContextReconciliation, ContextRef, EntryEvidence, EntryManifest, EvidenceContract,
    FlowRef, GraphEdge, GraphNode, LastKnownGoodApplicability, LastKnownGoodBinding,
    LastKnownGoodCandidate, LastKnownGoodState, ModuleResolutionBasis, ModuleResolutionConfigFile,
    RelatedTestEvidence, ResolutionEvidence, SourceFile,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiagnosticLimits {
    pub max_commits: usize,
    pub max_candidates: usize,
    pub max_paths: usize,
    pub max_context_refs: usize,
    pub max_assertions: usize,
    pub max_snapshot_bytes: usize,
    pub max_packet_bytes: usize,
}

impl Default for DiagnosticLimits {
    fn default() -> Self {
        Self {
            max_commits: 128,
            max_candidates: 64,
            max_paths: 256,
            max_context_refs: 256,
            max_assertions: 256,
            max_snapshot_bytes: 4 * 1024 * 1024,
            max_packet_bytes: 128 * 1024,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct GraphBasis {
    pub project_id: String,
    pub graph_id: String,
    pub graph_version: u64,
    pub source_revision: String,
    #[serde(default)]
    pub observation_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct GraphObservation {
    pub observation_id: String,
    pub project_id: String,
    pub graph_version: u64,
    pub git_revision: String,
    pub source_fingerprint: String,
    pub source_manifest: Vec<SourceFile>,
    pub dirty: bool,
    #[serde(default)]
    pub module_resolution_status: String,
    #[serde(default)]
    pub module_resolution_fingerprint: String,
    #[serde(default)]
    pub module_resolution_effective_fingerprint: String,
    #[serde(default)]
    pub module_resolution_manifest: Vec<ModuleResolutionConfigFile>,
    #[serde(default)]
    pub entry_manifest_status: String,
    #[serde(default)]
    pub entry_manifest_fingerprint: String,
    #[serde(default)]
    pub entry_effective_fingerprint: String,
    #[serde(default)]
    pub entry_manifest: Option<EntryManifest>,
    pub observed_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct GitBasis {
    pub revision: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticContext {
    pub schema_version: String,
    pub id: String,
    pub project_id: String,
    pub revision: u64,
    pub intent: String,
    pub symptom: String,
    pub expected_behavior: String,
    pub focus_context_refs: Vec<String>,
    #[serde(default)]
    pub focus_flow_refs: Vec<String>,
    pub current_graph_basis: GraphBasis,
    pub last_known_good_basis: Option<GitBasis>,
    #[serde(default)]
    pub last_known_good_binding_id: Option<String>,
    #[serde(default)]
    pub last_known_good_candidate_id: Option<String>,
    pub constraints: Vec<String>,
    pub acceptance_criteria: Vec<String>,
    pub unresolved_questions: Vec<String>,
    pub actor: String,
    pub created_at: u64,
    pub status: String,
    pub supersedes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceReference {
    pub evidence_class: String,
    pub kind: String,
    pub reference: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticAssertion {
    pub schema_version: String,
    pub id: String,
    pub context_id: String,
    pub revision: u64,
    pub kind: String,
    pub status: String,
    pub actor: String,
    pub statement: String,
    pub evidence: Vec<EvidenceReference>,
    pub supersedes: Option<String>,
    pub created_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct HistoricalCandidate {
    pub schema_version: String,
    pub id: String,
    pub project_id: String,
    pub context_id: String,
    pub current_graph_basis: GraphBasis,
    pub last_known_good_revision: String,
    pub commit: String,
    pub parents: Vec<String>,
    pub summary: String,
    pub changed_paths: Vec<String>,
    pub changed_paths_truncated: bool,
    pub relevance_reasons: Vec<String>,
    pub score: u32,
    pub retention_status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct HistoricalSnapshot {
    pub schema_version: String,
    pub project_id: String,
    pub source_revision: String,
    #[serde(default)]
    pub repository_identity_id: Option<String>,
    #[serde(default)]
    pub evidence_contract: Option<EvidenceContract>,
    pub files: Vec<SourceFile>,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    #[serde(default)]
    pub resolution_evidence: ResolutionEvidence,
    #[serde(default)]
    pub module_resolution: ModuleResolutionBasis,
    #[serde(
        default,
        skip_serializing_if = "crate::model::flow::is_default_entry_evidence"
    )]
    pub entry_evidence: EntryEvidence,
    #[serde(
        default,
        skip_serializing_if = "crate::model::flow::is_default_related_test_evidence"
    )]
    pub related_test_evidence: RelatedTestEvidence,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flows: Vec<ContextFlow>,
    pub truncated: bool,
    pub omissions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct HistoricalDiagnosis {
    pub schema_version: String,
    pub context_id: String,
    pub current_graph_basis: GraphBasis,
    pub last_known_good_basis: Option<GitBasis>,
    #[serde(default)]
    pub last_known_good_binding: Option<LastKnownGoodBinding>,
    #[serde(default)]
    pub last_known_good_candidate: Option<LastKnownGoodCandidate>,
    #[serde(default)]
    pub last_known_good_state: Option<LastKnownGoodState>,
    #[serde(default)]
    pub last_known_good_applicability: Option<LastKnownGoodApplicability>,
    #[serde(default)]
    pub last_known_good_status: String,
    pub range: Option<String>,
    pub commits_inspected: usize,
    pub candidates: Vec<HistoricalCandidate>,
    pub truncated: bool,
    pub omissions: Vec<String>,
    pub limitations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticPacket {
    pub schema_version: String,
    pub context: DiagnosticContext,
    pub current_graph_basis: GraphBasis,
    pub last_known_good_basis: Option<GitBasis>,
    #[serde(default)]
    pub last_known_good_binding: Option<LastKnownGoodBinding>,
    #[serde(default)]
    pub last_known_good_candidate: Option<LastKnownGoodCandidate>,
    #[serde(default)]
    pub last_known_good_state: Option<LastKnownGoodState>,
    #[serde(default)]
    pub last_known_good_applicability: Option<LastKnownGoodApplicability>,
    pub focus_context_refs: Vec<ContextRef>,
    #[serde(default)]
    pub focus_flow_refs: Vec<FlowRef>,
    pub focus_nodes: Vec<GraphNode>,
    #[serde(default)]
    pub focus_flows: Vec<ContextFlow>,
    #[serde(default)]
    pub related_tests: RelatedTestEvidence,
    #[serde(default)]
    pub context_reconciliation: Vec<ContextReconciliation>,
    pub assertions: Vec<DiagnosticAssertion>,
    pub historical: HistoricalDiagnosis,
    pub limitations: Vec<String>,
    pub omissions: Vec<String>,
    pub truncated: bool,
}
