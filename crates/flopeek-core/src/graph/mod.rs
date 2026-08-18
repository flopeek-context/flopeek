//! Deterministic graph assembly for TypeScript/TSX evidence.

use crate::discovery::{discover, read_source};
use crate::flow;
use crate::module_resolution::{
    ModuleResolver, NonRelativeResolution, resolve_relative as resolve_module_relative,
};

use crate::model::{
    GraphEdge, GraphNode, GraphSnapshot, PRODUCT_IDENTITY, ResolutionEvidence, SourceFile,
    SymbolResolution, TYPESCRIPT_RESOLUTION_SCHEMA, TypeScriptDeclaration, TypeScriptFacts,
};
use crate::typescript;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;

pub const MAX_SOURCE_FILES: usize = 10_000;
pub const MAX_GRAPH_NODES: usize = 50_000;
pub const MAX_GRAPH_EDGES: usize = 100_000;
pub const MAX_RESOLUTION_RECORDS: usize = 100_000;
pub const MAX_REEXPORT_DEPTH: usize = 32;
pub const GRAPH_DERIVATION_ID: &str = "typescript-structural-evidence-v6";

mod builder;
mod call_resolution;
mod calls;
mod exports;
mod fingerprints;
mod resolution_records;
mod targets;
#[cfg(test)]
mod tests;

pub use builder::build;
pub(super) use call_resolution::{ResolutionOutcome, resolve_call};
pub(super) use calls::{ResolutionContext, resolve_calls, resolve_heritage, resolve_import_raw};
pub(super) use exports::{ExportBinding, ModuleExports, build_module_exports};
pub(super) use fingerprints::{
    assign_node_fingerprints, exact_source_fingerprint, is_non_relative_alias,
    normalize_symbol_kind,
};
pub use fingerprints::{node_id, project_id, resolve_relative, source_revision};
pub(super) use resolution_records::coalesce_resolutions;
pub use resolution_records::resolution_evidence;
pub(super) use targets::{
    callable_candidates, combine_outcomes, resolve_export, resolve_external_or_unresolved,
    resolve_import_module, resolve_import_target, unique_or_unresolved, unresolved,
    unresolved_with_candidates, unresolved_with_status,
};
