//! Attributed, append-only last-known-good bindings.

use super::{EvidenceReference, GitBasis, GraphBasis, LAST_KNOWN_GOOD_SCHEMA};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct LastKnownGoodValidation {
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub revision_available: bool,
    #[serde(default)]
    pub repository_match: bool,
    #[serde(default)]
    pub first_parent_range_available: bool,
    #[serde(default)]
    pub evidence_contract_compatible: bool,
    #[serde(default)]
    pub limitations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct LastKnownGoodBinding {
    pub schema_version: String,
    pub binding_id: String,
    pub repository_id: String,
    pub project_id: String,
    pub context_id: String,
    pub git_revision: String,
    #[serde(default)]
    pub observation_id: Option<String>,
    #[serde(default)]
    pub event_id: Option<String>,
    #[serde(default)]
    pub graph_basis: Option<GraphBasis>,
    pub actor: String,
    pub actor_kind: String,
    pub evidence: Vec<EvidenceReference>,
    pub status: String,
    #[serde(default)]
    pub predecessor_binding_id: Option<String>,
    #[serde(default)]
    pub superseded_binding_id: Option<String>,
    pub created_at: u64,
    #[serde(default)]
    pub validation: LastKnownGoodValidation,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct LastKnownGoodResolution {
    pub schema_version: String,
    pub context_id: String,
    pub status: String,
    #[serde(default)]
    pub binding: Option<LastKnownGoodBinding>,
    #[serde(default)]
    pub legacy_basis: Option<GitBasis>,
    pub limitations: Vec<String>,
}

impl LastKnownGoodBinding {
    pub fn new_schema_version() -> String {
        LAST_KNOWN_GOOD_SCHEMA.to_string()
    }
}
