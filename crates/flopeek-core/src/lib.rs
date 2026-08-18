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
}
