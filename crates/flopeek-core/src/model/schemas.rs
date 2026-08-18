//! Stable, JSON-safe data structures shared by discovery, graph, context and storage.
//!
//! These types deliberately contain evidence and references, never source bodies.  A
//! caller that needs source text must read the repository at the graph basis it was
//! given; the persisted product record remains bounded and portable.

use serde::{Deserialize, Serialize};

pub const PRODUCT_IDENTITY: &str = "flopeek-repository-memory";
pub const PRODUCT_CONTRACT_SCHEMA: &str = "flopeek-product-contract/v3";
pub const GRAPH_SCHEMA: &str = "flopeek-graph/v6";
pub const CONTEXT_REF_SCHEMA: &str = "flopeek-context-ref/v3";
pub const PROTOCOL_SCHEMA: &str = "flopeek-protocol/v7";
pub const STORE_SCHEMA: &str = "flopeek-sqlite/v5";
pub const TYPESCRIPT_FACTS_SCHEMA: &str = "flopeek-typescript-facts/v4";
pub const TYPESCRIPT_RESOLUTION_SCHEMA: &str = "flopeek-typescript-resolution/v3";
pub const DIAGNOSTIC_CONTEXT_SCHEMA: &str = "flopeek-diagnostic-context/v3";
pub const DIAGNOSTIC_ASSERTION_SCHEMA: &str = "flopeek-diagnostic-assertion/v2";
pub const HISTORICAL_CANDIDATE_SCHEMA: &str = "flopeek-historical-candidate/v2";
pub const HISTORICAL_DIAGNOSIS_SCHEMA: &str = "flopeek-historical-diagnosis/v1";
pub const DIAGNOSTIC_PACKET_SCHEMA: &str = "flopeek-diagnostic-packet/v4";
pub const HISTORICAL_SNAPSHOT_SCHEMA: &str = "flopeek-historical-snapshot/v6";
pub const ENTRY_EVIDENCE_SCHEMA: &str = "flopeek-entry-evidence/v1";
pub const CONTEXT_FLOW_SCHEMA: &str = "flopeek-context-flow/v1";
pub const RELATED_TEST_EVIDENCE_SCHEMA: &str = "flopeek-related-test-evidence/v1";
pub const FLOW_REF_SCHEMA: &str = "flopeek-flow-ref/v1";
pub const OBSERVATION_CONTINUITY_SCHEMA: &str = "flopeek-observation-continuity/v1";
pub const CONTEXT_RECONCILIATION_SCHEMA: &str = "flopeek-context-reconciliation/v1";

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
