//! Stable, JSON-safe data structures shared by discovery, graph, context and storage.
//!
//! These types deliberately contain evidence and references, never source bodies.  A
//! caller that needs source text must read the repository at the graph basis it was
//! given; the persisted product record remains bounded and portable.

use serde::{Deserialize, Serialize};

pub const PRODUCT_IDENTITY: &str = "flopeek-repository-memory";
pub const PRODUCT_CONTRACT_SCHEMA: &str = "flopeek-product-contract/v1";
pub const GRAPH_SCHEMA: &str = "flopeek-graph/v1";
pub const CONTEXT_REF_SCHEMA: &str = "flopeek-context-ref/v1";
pub const PROTOCOL_SCHEMA: &str = "flopeek-protocol/v1";
pub const STORE_SCHEMA: &str = "flopeek-sqlite/v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TypeScriptDeclaration {
    pub name: String,
    pub kind: String,
    pub exported: bool,
    pub position: SourcePosition,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TypeScriptCall {
    pub callee: Option<String>,
    pub dynamic: bool,
    pub position: SourcePosition,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TypeScriptFacts {
    pub path: String,
    pub language: String,
    pub source_hash: String,
    pub parser: String,
    pub parse_status: String,
    pub imports: Vec<TypeScriptImport>,
    pub declarations: Vec<TypeScriptDeclaration>,
    pub calls: Vec<TypeScriptCall>,
    pub unsupported: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GraphNode {
    pub id: String,
    pub kind: String,
    pub path: Option<String>,
    pub name: Option<String>,
    pub language: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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
    pub files: Vec<SourceFile>,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
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
    pub max_packet_bytes: usize,
}

impl Default for DiagnosticLimits {
    fn default() -> Self {
        Self {
            max_commits: 128,
            max_candidates: 64,
            max_paths: 256,
            max_packet_bytes: 128 * 1024,
        }
    }
}
