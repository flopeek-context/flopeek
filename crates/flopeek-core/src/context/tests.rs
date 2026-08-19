//! Context Ref tests.

use super::*;
use crate::model::GraphSnapshot;

#[test]
fn context_uri_is_deterministic_and_explicitly_branded() {
    assert_eq!(
        uri("project_a", "graph_b", "node_c"),
        "fp://local/project_a/graph_b/node_c"
    );
    let _ = GraphSnapshot {
        schema_version: "graph".to_string(),
        product: "product".to_string(),
        project_id: "project".to_string(),
        graph_id: "graph".to_string(),
        graph_version: 1,
        source_revision: "unavailable".to_string(),
        source_fingerprint: String::new(),
        observation_id: String::new(),
        identity_basis: crate::identity::IdentityBasis::default(),
        files: Vec::new(),
        nodes: Vec::new(),
        edges: Vec::new(),
        resolution_evidence: crate::model::ResolutionEvidence::default(),
        module_resolution: crate::model::ModuleResolutionBasis::default(),
        entry_evidence: crate::model::EntryEvidence::default(),
        related_test_evidence: crate::model::RelatedTestEvidence::default(),
        flows: Vec::new(),
        truncated: false,
        omissions: Vec::new(),
    };
}
