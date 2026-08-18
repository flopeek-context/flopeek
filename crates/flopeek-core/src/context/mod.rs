//! Context Ref identity and freshness resolution.

use crate::model::{CONTEXT_REF_SCHEMA, ContextRef, GraphBasis, GraphSnapshot};
use rusqlite::{Connection, OptionalExtension, Transaction, params};

pub const MAX_CONTEXT_REFS: usize = 256;

pub fn uri(project_id: &str, graph_id: &str, node_id: &str) -> String {
    format!("fp://local/{project_id}/{graph_id}/{node_id}")
}

mod persistence;
mod resolution;
#[cfg(test)]
mod tests;

pub use persistence::for_snapshot;
pub use resolution::resolve;
