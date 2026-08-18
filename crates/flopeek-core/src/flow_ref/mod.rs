//! Canonical Flow Ref persistence and freshness resolution.

use crate::model::{ContextFlow, FLOW_REF_SCHEMA, FlowRef, GraphBasis, GraphSnapshot};
use rusqlite::{Connection, OptionalExtension, Transaction, params};

pub fn uri(project_id: &str, graph_id: &str, flow_id: &str) -> String {
    format!("fp://local/{project_id}/{graph_id}/flow/{flow_id}")
}

mod persistence;
mod resolution;

pub use persistence::for_snapshot;
pub use resolution::resolve;
use resolution::{resolve_transaction, validate_canonical};
