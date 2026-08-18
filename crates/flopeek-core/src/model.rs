//! Stable, JSON-safe data structures shared by discovery, graph, context and storage.
//!
//! These types deliberately contain evidence and references, never source bodies.  A
//! caller that needs source text must read the repository at the graph basis it was
//! given; the persisted product record remains bounded and portable.

use serde::{Deserialize, Serialize};

pub const PRODUCT_IDENTITY: &str = "flopeek-repository-memory";
pub const PRODUCT_CONTRACT_SCHEMA: &str = "flopeek-product-contract/v1";
pub const GRAPH_SCHEMA: &str = "flopeek-graph/v3";
pub const CONTEXT_REF_SCHEMA: &str = "flopeek-context-ref/v2";
pub const PROTOCOL_SCHEMA: &str = "flopeek-protocol/v3";
pub const STORE_SCHEMA: &str = "flopeek-sqlite/v2";
pub const TYPESCRIPT_FACTS_SCHEMA: &str = "flopeek-typescript-facts/v2";
pub const TYPESCRIPT_RESOLUTION_SCHEMA: &str = "flopeek-typescript-resolution/v1";
pub const DIAGNOSTIC_CONTEXT_SCHEMA: &str = "flopeek-diagnostic-context/v2";
pub const DIAGNOSTIC_ASSERTION_SCHEMA: &str = "flopeek-diagnostic-assertion/v2";
pub const HISTORICAL_CANDIDATE_SCHEMA: &str = "flopeek-historical-candidate/v2";
pub const HISTORICAL_DIAGNOSIS_SCHEMA: &str = "flopeek-historical-diagnosis/v1";
pub const DIAGNOSTIC_PACKET_SCHEMA: &str = "flopeek-diagnostic-packet/v2";
pub const HISTORICAL_SNAPSHOT_SCHEMA: &str = "flopeek-historical-snapshot/v3";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SourceFile {
    pub path: String,
    pub language: String,
    pub bytes: u64,
    pub hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourcePosition {
    pub start_line: usize,
    pub start_column: usize,
    pub end_line: usize,
    pub end_column: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TypeScriptImport {
    pub specifier: String,
    pub kind: String,
    pub position: SourcePosition,
    #[serde(default)]
    pub local_name: Option<String>,
    #[serde(default)]
    pub imported_name: Option<String>,
    #[serde(default)]
    pub type_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TypeScriptDeclaration {
    pub name: String,
    pub kind: String,
    pub exported: bool,
    pub position: SourcePosition,
    #[serde(default)]
    pub qualified_name: String,
    #[serde(default)]
    pub ast_fingerprint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TypeScriptCall {
    pub callee: Option<String>,
    pub dynamic: bool,
    pub position: SourcePosition,
    #[serde(default)]
    pub caller: Option<String>,
    #[serde(default)]
    pub callee_form: String,
    #[serde(default)]
    pub receiver: Option<String>,
    #[serde(default)]
    pub shadowed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TypeScriptExport {
    pub exported_name: String,
    pub local_name: Option<String>,
    pub kind: String,
    pub source: Option<String>,
    pub type_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SymbolResolution {
    pub path: String,
    pub caller_node_id: String,
    pub reference: String,
    pub form: String,
    pub status: String,
    pub reason: String,
    pub candidate_node_ids: Vec<String>,
    pub occurrence_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TypeScriptFacts {
    #[serde(default)]
    pub schema_version: String,
    pub path: String,
    pub language: String,
    pub source_hash: String,
    pub parser: String,
    pub parse_status: String,
    pub imports: Vec<TypeScriptImport>,
    pub declarations: Vec<TypeScriptDeclaration>,
    #[serde(default)]
    pub exports: Vec<TypeScriptExport>,
    pub calls: Vec<TypeScriptCall>,
    pub unsupported: Vec<String>,
    #[serde(default)]
    pub resolution_records: Vec<SymbolResolution>,
    #[serde(default)]
    pub canonical_fingerprint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ResolutionEvidence {
    pub schema_version: String,
    pub status: String,
    pub records: Vec<SymbolResolution>,
    pub truncated: bool,
    pub omissions: Vec<String>,
}

impl Default for ResolutionEvidence {
    fn default() -> Self {
        Self {
            schema_version: TYPESCRIPT_RESOLUTION_SCHEMA.to_string(),
            status: "unavailable".to_string(),
            records: Vec::new(),
            truncated: false,
            omissions: vec![
                "resolution evidence is unavailable until a v2 TypeScript facts scan".to_string(),
            ],
        }
    }
}

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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
    pub files: Vec<SourceFile>,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    #[serde(default)]
    pub resolution_evidence: ResolutionEvidence,
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
    pub freshness_reason: String,
    #[serde(default)]
    pub origin_basis: Option<GraphBasis>,
    #[serde(default)]
    pub current_basis: Option<GraphBasis>,
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
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScanResult {
    pub schema_version: String,
    pub product: String,
    pub project_id: String,
    pub graph: GraphSnapshot,
    pub context_refs: Vec<ContextRef>,
    pub limitations: Vec<String>,
}

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
    pub current_graph_basis: GraphBasis,
    pub last_known_good_basis: Option<GitBasis>,
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
    pub files: Vec<SourceFile>,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    #[serde(default)]
    pub resolution_evidence: ResolutionEvidence,
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
    pub focus_context_refs: Vec<ContextRef>,
    pub focus_nodes: Vec<GraphNode>,
    pub assertions: Vec<DiagnosticAssertion>,
    pub historical: HistoricalDiagnosis,
    pub limitations: Vec<String>,
    pub omissions: Vec<String>,
    pub truncated: bool,
}
