#![allow(clippy::pedantic)]

pub mod context;
pub mod contract;
pub mod diagnostic;
pub mod discovery;
pub mod flow;
pub mod flow_ref;
pub mod graph;
pub mod history_store;
pub mod model;
pub mod module_resolution;
pub mod protocol;
pub mod store;
pub mod typescript;

#[cfg(test)]
mod architecture_contract_tests {
    #[test]
    fn public_core_contract_paths_remain_stable() {
        assert_eq!(crate::model::GRAPH_SCHEMA, "flopeek-graph/v6");
        assert_eq!(crate::model::PROTOCOL_SCHEMA, "flopeek-protocol/v6");
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
            "typescript-structural-evidence-v6"
        );
        assert_eq!(
            crate::flow::flow_id("project", "script", "start"),
            crate::flow::flow_id("project", "script", "start")
        );
    }

    #[test]
    fn persistence_and_diagnostic_contract_paths_remain_stable() {
        assert_eq!(crate::store::CURRENT_USER_VERSION, 6);
        assert_eq!(crate::model::CONTEXT_REF_SCHEMA, "flopeek-context-ref/v2");
        assert_eq!(crate::model::FLOW_REF_SCHEMA, "flopeek-flow-ref/v1");
        assert_eq!(
            crate::model::DIAGNOSTIC_PACKET_SCHEMA,
            "flopeek-diagnostic-packet/v3"
        );
        assert_eq!(
            crate::model::HISTORICAL_SNAPSHOT_SCHEMA,
            "flopeek-historical-snapshot/v6"
        );
        let _: fn(&std::path::Path) -> Result<crate::model::ScanResult, String> =
            crate::protocol::scan_project;
        let _: fn(&std::path::Path, &str) -> Result<crate::model::FlowRef, String> =
            crate::store::resolve_flow;
    }
}
