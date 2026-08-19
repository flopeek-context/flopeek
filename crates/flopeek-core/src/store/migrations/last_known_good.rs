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

pub(crate) fn migration_v11(transaction: &Transaction<'_>) -> Result<(), String> {
    add_column(
        transaction,
        "last_known_good_bindings",
        "target_binding_id",
        "TEXT",
    )?;
    add_column(
        transaction,
        "last_known_good_bindings",
        "supersedes_binding_id",
        "TEXT",
    )?;

    #[derive(Clone)]
    struct LegacyRow {
        binding_id: String,
        predecessor: Option<String>,
        status: String,
        legacy_target: Option<String>,
        payload: String,
        created_at: i64,
    }
    let rows = {
        let mut statement = transaction
            .prepare(
                "SELECT binding_id, context_id, status, predecessor_binding_id,
                        superseded_binding_id, payload_json, created_at
                 FROM last_known_good_bindings
                 ORDER BY context_id, created_at, binding_id",
            )
            .map_err(|error| format!("Unable to inspect legacy last-known-good rows: {error}"))?;
        statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    LegacyRow {
                        binding_id: row.get(0)?,
                        predecessor: row.get(3)?,
                        status: row.get(2)?,
                        legacy_target: row.get(4)?,
                        payload: row.get(5)?,
                        created_at: row.get(6)?,
                    },
                ))
            })
            .map_err(|error| format!("Unable to enumerate legacy last-known-good rows: {error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("Unable to read legacy last-known-good rows: {error}"))?
    };

    let mut by_context = std::collections::BTreeMap::<String, Vec<LegacyRow>>::new();
    for (_binding_id, context_id, row) in rows {
        by_context.entry(context_id).or_default().push(row);
    }
    for (_context_id, context_rows) in by_context {
        let by_id = context_rows
            .iter()
            .map(|row| (row.binding_id.as_str(), row))
            .collect::<std::collections::BTreeMap<_, _>>();
        let mut successors = std::collections::BTreeMap::<&str, &str>::new();
        let mut roots = Vec::new();
        let mut chain_valid = true;
        for row in &context_rows {
            match row.predecessor.as_deref() {
                Some(predecessor)
                    if predecessor != row.binding_id && by_id.contains_key(predecessor) =>
                {
                    if successors
                        .insert(predecessor, row.binding_id.as_str())
                        .is_some()
                    {
                        chain_valid = false;
                    }
                }
                Some(_) => chain_valid = false,
                None => roots.push(row.binding_id.as_str()),
            }
        }
        if roots.len() != 1 {
            chain_valid = false;
        }
        let mut ordered_ids = Vec::with_capacity(context_rows.len());
        if chain_valid {
            let mut current = roots[0];
            let mut seen = std::collections::BTreeSet::new();
            while seen.insert(current) {
                ordered_ids.push(current);
                let Some(next) = successors.get(current) else {
                    break;
                };
                current = next;
            }
            if ordered_ids.len() != context_rows.len() {
                chain_valid = false;
            }
        }
        if !chain_valid {
            let mut fallback = context_rows.iter().collect::<Vec<_>>();
            fallback.sort_by(|left, right| {
                left.created_at
                    .cmp(&right.created_at)
                    .then_with(|| left.binding_id.cmp(&right.binding_id))
            });
            ordered_ids = fallback
                .into_iter()
                .map(|row| row.binding_id.as_str())
                .collect();
        }

        let mut active: Option<String> = None;
        let mut pending: Option<String> = None;
        for binding_id in ordered_ids {
            let row = by_id[binding_id];
            let mut value =
                serde_json::from_str::<serde_json::Value>(&row.payload).map_err(|error| {
                    format!("Unable to decode legacy last-known-good {binding_id}: {error}")
                })?;
            let object = value.as_object_mut().ok_or_else(|| {
                format!("Legacy last-known-good {binding_id} payload is not an object.")
            })?;
            let mut target = object
                .get("targetBindingId")
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned)
                .or_else(|| row.legacy_target.clone());
            let mut supersedes = object
                .get("supersedesBindingId")
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned);
            if chain_valid {
                match row.status.as_str() {
                    "proposed" => {
                        target = None;
                        supersedes = None;
                        pending = Some(row.binding_id.clone());
                    }
                    "confirmed" => {
                        target = target.or_else(|| pending.clone());
                        supersedes = supersedes.or_else(|| active.clone());
                        active = Some(row.binding_id.clone());
                        pending = None;
                    }
                    "rejected" => {
                        target = target.or_else(|| pending.clone());
                        pending = None;
                    }
                    "revoked" | "superseded" => {
                        target = target.or_else(|| active.clone());
                        active = None;
                    }
                    _ => chain_valid = false,
                }
            }
            object.insert(
                "schemaVersion".to_string(),
                serde_json::Value::String(crate::model::LAST_KNOWN_GOOD_SCHEMA.to_string()),
            );
            object.remove("supersededBindingId");
            if let Some(target) = target.as_deref() {
                object.insert(
                    "targetBindingId".to_string(),
                    serde_json::Value::String(target.to_string()),
                );
            } else {
                object.remove("targetBindingId");
            }
            if let Some(supersedes) = supersedes.as_deref() {
                object.insert(
                    "supersedesBindingId".to_string(),
                    serde_json::Value::String(supersedes.to_string()),
                );
            } else {
                object.remove("supersedesBindingId");
            }
            let has_graph_basis = object.get("graphBasis").is_some();
            let validation = object
                .entry("validation")
                .or_insert_with(|| serde_json::json!({}));
            if let Some(validation) = validation.as_object_mut() {
                if has_graph_basis || !chain_valid {
                    validation.insert(
                        "status".to_string(),
                        serde_json::Value::String("invalid".to_string()),
                    );
                }
                validation.insert(
                    "basisProvenanceConsistent".to_string(),
                    serde_json::Value::Bool(!has_graph_basis),
                );
                if has_graph_basis {
                    let limitations = validation
                        .entry("limitations")
                        .or_insert_with(|| serde_json::json!([]));
                    if let Some(limitations) = limitations.as_array_mut() {
                        limitations.push(serde_json::Value::String(
                            "legacy-basis-provenance-unverified".to_string(),
                        ));
                    }
                }
                if !chain_valid {
                    let limitations = validation
                        .entry("limitations")
                        .or_insert_with(|| serde_json::json!([]));
                    if let Some(limitations) = limitations.as_array_mut() {
                        limitations.push(serde_json::Value::String(
                            "legacy-lifecycle-order-unavailable".to_string(),
                        ));
                    }
                }
            }
            let encoded = serde_json::to_string(&value).map_err(|error| {
                format!("Unable to encode migrated last-known-good {binding_id}: {error}")
            })?;
            transaction
                .execute(
                    "UPDATE last_known_good_bindings
                     SET target_binding_id = ?1, supersedes_binding_id = ?2, payload_json = ?3
                     WHERE binding_id = ?4",
                    params![target, supersedes, encoded, binding_id],
                )
                .map_err(|error| {
                    format!("Unable to persist migrated last-known-good {binding_id}: {error}")
                })?;
        }
    }
    Ok(())
}
