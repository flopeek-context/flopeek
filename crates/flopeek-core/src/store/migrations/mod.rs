//! SQLite schema initialization and migrations.

#[allow(unused_imports)]
use super::*;

mod base;
mod versions;

pub(super) use base::{migration_v1, migration_v2, migration_v3, migration_v4};
pub(super) use versions::{migration_v5, migration_v6};

#[allow(unused_imports)]
use super::*;

pub(super) fn initialize_schema(connection: &mut Connection) -> Result<(), String> {
    let mut version = connection
        .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
        .map_err(|error| format!("Unable to read SQLite schema version: {error}"))?;
    if version > CURRENT_USER_VERSION {
        return Err(format!(
            "SQLite database schema version {version} is newer than supported version {CURRENT_USER_VERSION}."
        ));
    }

    while version < CURRENT_USER_VERSION {
        let target = version + 1;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| {
                format!("Unable to begin SQLite migration {version}->{target}: {error}")
            })?;
        match target {
            1 => migration_v1(&transaction)?,
            2 => migration_v2(&transaction)?,
            3 => migration_v3(&transaction)?,
            4 => migration_v4(&transaction)?,
            5 => migration_v5(&transaction)?,
            6 => migration_v6(&transaction)?,
            _ => unreachable!("migration target is bounded by CURRENT_USER_VERSION"),
        }
        transaction
            .execute_batch(&format!("PRAGMA user_version = {target};"))
            .map_err(|error| {
                format!("Unable to record SQLite migration version {target}: {error}")
            })?;
        transaction.commit().map_err(|error| {
            format!("Unable to commit SQLite migration {version}->{target}: {error}")
        })?;
        version = target;
    }
    Ok(())
}

fn migrate_context_flow_refs(transaction: &Transaction<'_>) -> Result<(), String> {
    let rows = {
        let mut statement = transaction
            .prepare("SELECT id, payload_json FROM diagnostic_contexts")
            .map_err(|error| format!("Unable to inspect Diagnostic Context payloads: {error}"))?;
        statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|error| format!("Unable to enumerate Diagnostic Context payloads: {error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("Unable to decode Diagnostic Context payloads: {error}"))?
    };
    for (id, payload) in rows {
        let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&payload) else {
            continue;
        };
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "schemaVersion".to_string(),
                serde_json::json!(crate::model::DIAGNOSTIC_CONTEXT_SCHEMA),
            );
            object
                .entry("focusFlowRefs")
                .or_insert_with(|| serde_json::json!([]));
            let encoded = serde_json::to_string(&value).map_err(|error| {
                format!("Unable to encode Diagnostic Context migration: {error}")
            })?;
            transaction
                .execute(
                    "UPDATE diagnostic_contexts SET payload_json = ?1 WHERE id = ?2",
                    params![encoded, id],
                )
                .map_err(|error| format!("Unable to migrate Diagnostic Context {id}: {error}"))?;
        }
    }
    Ok(())
}

fn add_column(
    transaction: &Transaction<'_>,
    table: &str,
    column: &str,
    definition: &str,
) -> Result<(), String> {
    if !table_columns(transaction, table)?
        .iter()
        .any(|name| name == column)
    {
        transaction
            .execute(
                &format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"),
                [],
            )
            .map_err(|error| format!("Unable to migrate {table}.{column}: {error}"))?;
    }
    Ok(())
}

pub(super) fn observation_id(
    project_id: &str,
    source_revision: &str,
    source_fingerprint: &str,
    module_resolution_fingerprint: &str,
    entry_manifest_fingerprint: &str,
    graph_id: &str,
) -> String {
    let input = format!(
        "flopeek-observation-v3\0{project_id}\0{source_revision}\0{source_fingerprint}\0{module_resolution_fingerprint}\0{entry_manifest_fingerprint}\0{graph_id}"
    );
    format!("observation_{}", blake3::hash(input.as_bytes()).to_hex())
}

fn legacy_observation_id(
    project_id: &str,
    source_revision: &str,
    source_fingerprint: &str,
    graph_id: &str,
) -> String {
    let input = format!(
        "flopeek-observation-v1\0{project_id}\0{source_revision}\0{source_fingerprint}\0{graph_id}"
    );
    format!("observation_{}", blake3::hash(input.as_bytes()).to_hex())
}

fn migrate_context_payloads(transaction: &Transaction<'_>) -> Result<(), String> {
    let rows = {
        let mut statement = transaction
            .prepare("SELECT id, payload_json FROM diagnostic_contexts")
            .map_err(|error| format!("Unable to inspect Diagnostic Context payloads: {error}"))?;
        statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|error| format!("Unable to enumerate Diagnostic Context payloads: {error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("Unable to decode Diagnostic Context payloads: {error}"))?
    };
    for (id, payload) in rows {
        let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&payload) else {
            continue;
        };
        let Some(basis) = value
            .get_mut("currentGraphBasis")
            .and_then(serde_json::Value::as_object_mut)
        else {
            continue;
        };
        let Some(graph_version) = basis
            .get("graphVersion")
            .and_then(serde_json::Value::as_i64)
        else {
            continue;
        };
        let observation = transaction
            .query_row(
                "SELECT observation_id FROM graph_observations WHERE graph_version = ?1 ORDER BY observed_at DESC LIMIT 1",
                params![graph_version],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| format!("Unable to resolve Diagnostic Context observation: {error}"))?;
        basis.insert(
            "observationId".to_string(),
            serde_json::Value::String(observation.unwrap_or_default()),
        );
        if let Some(schema) = value.get_mut("schemaVersion") {
            *schema =
                serde_json::Value::String(crate::model::DIAGNOSTIC_CONTEXT_SCHEMA.to_string());
        }
        let updated = serde_json::to_string(&value)
            .map_err(|error| format!("Unable to encode migrated Diagnostic Context: {error}"))?;
        transaction
            .execute(
                "UPDATE diagnostic_contexts SET payload_json = ?1 WHERE id = ?2",
                params![updated, id],
            )
            .map_err(|error| format!("Unable to persist migrated Diagnostic Context: {error}"))?;
    }
    migrate_assertion_payloads(transaction)
}

fn migrate_assertion_payloads(transaction: &Transaction<'_>) -> Result<(), String> {
    let rows = {
        let mut statement = transaction
            .prepare("SELECT id, payload_json FROM diagnostic_assertions")
            .map_err(|error| format!("Unable to inspect Diagnostic Assertion payloads: {error}"))?;
        statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|error| format!("Unable to enumerate Diagnostic Assertion payloads: {error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("Unable to decode Diagnostic Assertion payloads: {error}"))?
    };
    for (id, payload) in rows {
        let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&payload) else {
            continue;
        };
        if let Some(schema) = value.get_mut("schemaVersion") {
            *schema =
                serde_json::Value::String(crate::model::DIAGNOSTIC_ASSERTION_SCHEMA.to_string());
        }
        let updated = serde_json::to_string(&value)
            .map_err(|error| format!("Unable to encode migrated Diagnostic Assertion: {error}"))?;
        transaction
            .execute(
                "UPDATE diagnostic_assertions SET payload_json = ?1 WHERE id = ?2",
                params![updated, id],
            )
            .map_err(|error| format!("Unable to persist migrated Diagnostic Assertion: {error}"))?;
    }
    Ok(())
}

fn table_exists(transaction: &Transaction<'_>, table: &str) -> Result<bool, String> {
    transaction
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
            params![table],
            |row| row.get::<_, i64>(0),
        )
        .map(|value| value != 0)
        .map_err(|error| format!("Unable to inspect SQLite table {table}: {error}"))
}

pub(super) fn table_columns(
    transaction: &Transaction<'_>,
    table: &str,
) -> Result<Vec<String>, String> {
    let mut statement = transaction
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(|error| format!("Unable to inspect {table} schema: {error}"))?;
    statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|error| format!("Unable to inspect {table} columns: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("Unable to decode {table} columns: {error}"))
}
