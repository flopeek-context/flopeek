#![allow(clippy::pedantic)]

pub mod context;
pub mod contract;
pub mod diagnostic;
pub mod discovery;
pub mod flow;
pub mod flow_ref;
pub mod graph;
pub mod history_store;
pub mod identity;
pub mod model;
pub mod module_resolution;
pub mod protocol;
pub mod store;
pub mod temporal;
pub mod typescript;

#[cfg(test)]
mod architecture_contract_tests {
    #[test]
    fn public_core_contract_paths_remain_stable() {
        assert_eq!(crate::model::GRAPH_SCHEMA, "flopeek-graph/v7");
        assert_eq!(crate::model::PROTOCOL_SCHEMA, "flopeek-protocol/v14");
        let _: fn(&std::path::Path) -> Result<crate::model::ScanResult, String> =
            crate::protocol::scan_project;
        let _: fn(&std::path::Path, &str) -> Result<crate::model::ContextRef, String> =
            crate::store::resolve_context;
    }

    #[test]
    fn typescript_evidence_identity_and_flow_ids_remain_stable() {
        assert_eq!(
            crate::typescript::PARSER_IDENTITY,
            "tree-sitter-typescript/0.23.2"
        );
        assert_eq!(
            crate::graph::GRAPH_DERIVATION_ID,
            "typescript-structural-evidence-v7"
        );
        assert_eq!(
            crate::flow::flow_id("project", "script", "start"),
            crate::flow::flow_id("project", "script", "start")
        );
    }

    #[test]
    fn evidence_boundaries_keep_public_facades_and_derivation_identity() {
        let _ = crate::module_resolution::ModuleResolver::load(std::path::Path::new("."));
        assert!(crate::flow::flow_id("project", "script", "start").starts_with("flow_"));
        assert_eq!(
            crate::graph::GRAPH_DERIVATION_ID,
            "typescript-structural-evidence-v7"
        );
        assert_eq!(
            crate::module_resolution::MODULE_RESOLUTION_SCHEMA,
            "flopeek-typescript-module-resolution/v1"
        );
    }

    #[test]
    fn persistence_and_diagnostic_contract_paths_remain_stable() {
        assert_eq!(crate::store::CURRENT_USER_VERSION, 12);
        assert_eq!(
            crate::model::PRODUCT_CONTRACT_SCHEMA,
            "flopeek-product-contract/v10"
        );
        assert_eq!(
            crate::model::LKG_CANDIDATE_SCHEMA,
            "flopeek-last-known-good-candidate/v1"
        );
        assert_eq!(
            crate::model::LKG_EVENT_SCHEMA,
            "flopeek-last-known-good-event/v1"
        );
        assert_eq!(
            crate::model::LKG_STATE_SCHEMA,
            "flopeek-last-known-good-state/v1"
        );
        assert_eq!(
            crate::model::LKG_REVIEW_PACKET_SCHEMA,
            "flopeek-last-known-good-review-packet/v1"
        );
        assert_eq!(
            crate::model::LAST_KNOWN_GOOD_SCHEMA,
            "flopeek-last-known-good/v2"
        );
        assert_eq!(crate::model::CONTEXT_REF_SCHEMA, "flopeek-context-ref/v4");
        assert_eq!(crate::model::FLOW_REF_SCHEMA, "flopeek-flow-ref/v2");
        assert_eq!(
            crate::model::DIAGNOSTIC_PACKET_SCHEMA,
            "flopeek-diagnostic-packet/v9"
        );
        assert_eq!(
            crate::model::HISTORICAL_SNAPSHOT_SCHEMA,
            "flopeek-historical-snapshot/v7"
        );
        let _: fn(&std::path::Path) -> Result<crate::model::ScanResult, String> =
            crate::protocol::scan_project;
        let _: fn(&std::path::Path, &str) -> Result<crate::model::FlowRef, String> =
            crate::store::resolve_flow;
    }

    #[test]
    fn temporal_contract_identity_is_deterministic() {
        assert_eq!(
            crate::model::OBSERVATION_CONTINUITY_SCHEMA,
            "flopeek-observation-continuity/v2"
        );
        assert_eq!(
            crate::model::CONTEXT_RECONCILIATION_SCHEMA,
            "flopeek-context-reconciliation/v2"
        );
        assert_eq!(
            crate::model::OBSERVATION_DELTA_SCHEMA,
            "flopeek-observation-delta/v2"
        );
        assert_eq!(
            crate::model::HISTORICAL_CONTEXT_CONTINUITY_SCHEMA,
            "flopeek-historical-context-continuity/v1"
        );
        assert_eq!(
            crate::temporal::observation_event_id("project", None, "observation"),
            crate::temporal::observation_event_id("project", None, "observation")
        );
        assert_eq!(
            crate::temporal::fingerprint_contract("ast-and-direct-edges"),
            "node-ast-and-direct-edges/v1"
        );
    }
}
