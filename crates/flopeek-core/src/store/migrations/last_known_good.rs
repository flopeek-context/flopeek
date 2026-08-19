//! Append-only last-known-good binding schema.

use super::*;

pub(crate) fn migration_v10(transaction: &Transaction<'_>) -> Result<(), String> {
    transaction
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS last_known_good_bindings (
                 binding_id TEXT PRIMARY KEY NOT NULL,
                 repository_id TEXT NOT NULL,
                 project_id TEXT NOT NULL,
                 context_id TEXT NOT NULL REFERENCES diagnostic_contexts(id),
                 git_revision TEXT NOT NULL,
                 observation_id TEXT,
                 event_id TEXT,
                 graph_basis_json TEXT,
                 actor TEXT NOT NULL,
                 actor_kind TEXT NOT NULL,
                 evidence_json TEXT NOT NULL,
                 status TEXT NOT NULL,
                 predecessor_binding_id TEXT,
                 superseded_binding_id TEXT,
                 payload_json TEXT NOT NULL,
                 created_at INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS last_known_good_context_idx
                 ON last_known_good_bindings(context_id, created_at, binding_id);
             CREATE INDEX IF NOT EXISTS last_known_good_status_idx
                 ON last_known_good_bindings(context_id, status, created_at);",
        )
        .map_err(|error| format!("Unable to initialize last-known-good schema: {error}"))
}
