//! Observation continuity and Context Ref reconciliation migration.

use super::*;

pub(super) fn migration_v7(transaction: &Transaction<'_>) -> Result<(), String> {
    add_column(
        transaction,
        "context_refs",
        "fingerprint_contract",
        "TEXT NOT NULL DEFAULT 'legacy-file-v1'",
    )?;
    add_column(transaction, "project_state", "current_event_id", "TEXT")?;
    transaction
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS observation_events (
                 event_id TEXT PRIMARY KEY NOT NULL,
                 project_id TEXT NOT NULL,
                 observation_id TEXT NOT NULL REFERENCES graph_observations(observation_id),
                 predecessor_event_id TEXT REFERENCES observation_events(event_id),
                 observed_at INTEGER NOT NULL,
                 UNIQUE(project_id, predecessor_event_id, observation_id)
             );
             CREATE INDEX IF NOT EXISTS observation_events_project_idx
                 ON observation_events(project_id, observed_at, event_id);",
        )
        .map_err(|error| format!("Unable to initialize observation continuity schema: {error}"))?;

    transaction
        .execute(
            "UPDATE context_refs
             SET fingerprint_contract = CASE
                 WHEN fingerprint_scope = 'ast-and-direct-edges'
                     THEN 'node-ast-and-direct-edges/v1'
                 WHEN fingerprint_scope = 'legacy-file-v1'
                     THEN 'legacy-file-v1'
                 ELSE 'legacy-file-v1'
             END",
            [],
        )
        .map_err(|error| {
            format!("Unable to backfill Context Ref fingerprint contracts: {error}")
        })?;

    let projects = {
        let mut statement = transaction
            .prepare(
                "SELECT project_id, current_observation_id
                 FROM project_state ORDER BY project_id",
            )
            .map_err(|error| format!("Unable to inspect current project observations: {error}"))?;
        statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|error| format!("Unable to enumerate current project observations: {error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("Unable to decode current project observations: {error}"))?
    };

    for (project_id, observation_id) in projects {
        let Some((observed_at, predecessor)) = transaction
            .query_row(
                "SELECT observed_at, NULL FROM graph_observations
                 WHERE observation_id = ?1 AND project_id = ?2",
                params![observation_id, project_id],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<String>>(1)?)),
            )
            .optional()
            .map_err(|error| format!("Unable to resolve legacy observation root: {error}"))?
        else {
            continue;
        };
        let event_id = crate::temporal::observation_event_id(
            &project_id,
            predecessor.as_deref(),
            &observation_id,
        );
        transaction
            .execute(
                "INSERT OR IGNORE INTO observation_events(
                    event_id, project_id, observation_id, predecessor_event_id, observed_at
                 ) VALUES(?1, ?2, ?3, ?4, ?5)",
                params![
                    event_id,
                    project_id,
                    observation_id,
                    predecessor,
                    observed_at
                ],
            )
            .map_err(|error| format!("Unable to backfill observation continuity root: {error}"))?;
        transaction
            .execute(
                "UPDATE project_state SET current_event_id = ?1 WHERE project_id = ?2",
                params![event_id, project_id],
            )
            .map_err(|error| format!("Unable to backfill current observation event: {error}"))?;
    }
    Ok(())
}
