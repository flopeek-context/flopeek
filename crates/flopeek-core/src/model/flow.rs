use serde::{Deserialize, Serialize};

use super::{ENTRY_EVIDENCE_SCHEMA, GraphBasis, RELATED_TEST_EVIDENCE_SCHEMA};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct EntryManifest {
    pub path: String,
    pub bytes: u64,
    pub hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct EntryRecord {
    pub key: String,
    pub kind: String,
    pub runner: Option<String>,
    pub target_path: Option<String>,
    pub target_node_id: Option<String>,
    pub status: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct EntryEvidence {
    pub schema_version: String,
    pub status: String,
    pub manifest: Option<EntryManifest>,
    pub exact_fingerprint: String,
    pub effective_fingerprint: String,
    pub records: Vec<EntryRecord>,
    pub truncated: bool,
    pub omissions: Vec<String>,
    pub limitations: Vec<String>,
}

impl Default for EntryEvidence {
    fn default() -> Self {
        Self {
            schema_version: ENTRY_EVIDENCE_SCHEMA.to_string(),
            status: "unavailable".to_string(),
            manifest: None,
            exact_fingerprint: String::new(),
            effective_fingerprint: String::new(),
            records: Vec::new(),
            truncated: false,
            omissions: vec!["legacy-entry-basis-unavailable".to_string()],
            limitations: vec!["entry-manifest-evidence-unavailable".to_string()],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct RelatedTestRecord {
    pub test_path: String,
    pub test_node_id: String,
    pub target_node_id: String,
    pub relation: String,
    pub strength: String,
    pub status: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct RelatedTestEvidence {
    pub schema_version: String,
    pub status: String,
    pub records: Vec<RelatedTestRecord>,
    pub truncated: bool,
    pub omissions: Vec<String>,
}

pub(crate) fn is_default_entry_evidence(value: &EntryEvidence) -> bool {
    value.status == "unavailable"
        && value.manifest.is_none()
        && value.records.is_empty()
        && value.exact_fingerprint.is_empty()
        && value.effective_fingerprint.is_empty()
}

pub(crate) fn is_default_related_test_evidence(value: &RelatedTestEvidence) -> bool {
    value.status == "unavailable" && value.records.is_empty()
}

impl Default for RelatedTestEvidence {
    fn default() -> Self {
        Self {
            schema_version: RELATED_TEST_EVIDENCE_SCHEMA.to_string(),
            status: "unavailable".to_string(),
            records: Vec::new(),
            truncated: false,
            omissions: vec!["related-test-evidence-unavailable".to_string()],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct FlowStep {
    pub index: usize,
    pub node_id: String,
    pub path: Option<String>,
    pub name: Option<String>,
    pub role: String,
    pub evidence_fingerprint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct FlowEdge {
    pub from: String,
    pub to: String,
    pub kind: String,
    pub evidence: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct ContextFlow {
    pub schema_version: String,
    pub flow_id: String,
    pub entry_node_id: String,
    pub entry_kind: String,
    pub entry_key: String,
    pub steps: Vec<FlowStep>,
    pub traversed_edges: Vec<FlowEdge>,
    pub related_tests: Vec<RelatedTestRecord>,
    pub fingerprint: String,
    pub status: String,
    pub truncated: bool,
    pub omissions: Vec<String>,
    pub limitations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct FlowRef {
    pub schema_version: String,
    pub uri: String,
    pub project_id: String,
    pub graph_id: String,
    pub graph_version: u64,
    pub flow_id: String,
    pub status: String,
    pub origin_observation_id: String,
    pub origin_source_revision: String,
    pub origin_fingerprint: String,
    pub fingerprint_scope: String,
    pub freshness_reason: String,
    pub origin_basis: Option<GraphBasis>,
    pub current_basis: Option<GraphBasis>,
}
