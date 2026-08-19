//! SQLite authority for graph versions and Context Refs.
//!
//! Writes are transactional and facts contain hashes/structure only.  Source bodies
//! and credentials never enter this store.

use crate::context;
use crate::flow_ref;
use crate::model::{
    CONTEXT_RECONCILIATION_SCHEMA, CONTEXT_REF_SCHEMA, ContextReconciliation, ContextRef,
    GraphBasis, GraphEdge, GraphNode, GraphSnapshot, OBSERVATION_CONTINUITY_SCHEMA,
    ObservationContinuity, ObservationContinuityEvent, PRODUCT_IDENTITY, STORE_SCHEMA, ScanResult,
    SourceFile, StoreStatus, TypeScriptFacts,
};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const STORE_DIRECTORY: &str = ".flopeek";
pub const STORE_FILENAME: &str = "flopeek.sqlite3";
pub const CURRENT_USER_VERSION: i64 = 10;

mod change;
mod continuity;
mod graph_validation;
mod last_known_good;
mod memory;
mod migrations;
mod observation;
mod query;
mod scan;
#[cfg(test)]
mod tests;

pub use change::get_observation_delta;
pub use continuity::{get_observation_continuity, reconcile_context};
pub(crate) use last_known_good::confirmed_last_known_good;
pub use last_known_good::{
    create_last_known_good_binding, get_last_known_good, list_last_known_good_history,
    validate_last_known_good,
};
use memory::now_seconds;
pub(crate) use memory::now_seconds_for_sql;
pub use memory::{
    append_diagnostic_assertion, create_diagnostic_context, get_diagnostic_context,
    list_diagnostic_assertions, persist_historical_candidates,
};
use migrations::observation_id;
pub use query::{
    current_graph, get_flow, list_flows, node_details, related_tests, resolve_context,
    resolve_flow, status,
};
pub use scan::persist_scan;

pub fn database_path(root: &Path) -> PathBuf {
    root.join(STORE_DIRECTORY).join(STORE_FILENAME)
}

pub fn open(root: &Path) -> Result<Connection, String> {
    let directory = root.join(STORE_DIRECTORY);
    fs::create_dir_all(&directory).map_err(|error| {
        format!(
            "Unable to create SQLite directory {}: {error}",
            directory.display()
        )
    })?;
    let path = database_path(root);
    let mut connection = Connection::open(&path)
        .map_err(|error| format!("Unable to open SQLite database {}: {error}", path.display()))?;
    connection
        .busy_timeout(std::time::Duration::from_secs(5))
        .map_err(|error| format!("Unable to set SQLite busy timeout: {error}"))?;
    connection
        .execute_batch(
            "PRAGMA foreign_keys = ON;
             PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;",
        )
        .map_err(|error| format!("Unable to configure SQLite: {error}"))?;
    migrations::initialize_schema(&mut connection)?;
    Ok(connection)
}
