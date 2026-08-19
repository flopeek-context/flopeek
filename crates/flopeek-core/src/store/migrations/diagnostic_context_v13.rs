/// Migrate Diagnostic Context basis, LKG candidate v2, and command receipts.
pub(crate) fn migration_v13(transaction: &Transaction<'_>) -> Result<(), String> {
    transaction
        .execute_batch(
            "ALTER TABLE diagnostic_contexts ADD COLUMN context_definition_revision INTEGER NOT NULL DEFAULT 1;
             ALTER TABLE diagnostic_contexts ADD COLUMN context_basis_fingerprint TEXT NOT NULL DEFAULT '';
             ALTER TABLE diagnostic_contexts ADD COLUMN memory_revision INTEGER NOT NULL DEFAULT 0;
             ALTER TABLE last_known_good_candidates ADD COLUMN context_definition_revision INTEGER NOT NULL DEFAULT 1;
             ALTER TABLE last_known_good_candidates ADD COLUMN context_basis_fingerprint TEXT NOT NULL DEFAULT '';
             CREATE TABLE last_known_good_command_receipts (
                 context_id TEXT NOT NULL REFERENCES diagnostic_contexts(id),
                 idempotency_key TEXT NOT NULL,
                 command_kind TEXT NOT NULL CHECK(command_kind IN ('PROPOSE','CONFIRM','REJECT','REVOKE')),
                 request_fingerprint TEXT NOT NULL,
                 result_candidate_id TEXT REFERENCES last_known_good_candidates(candidate_id),
                 result_event_id TEXT NOT NULL REFERENCES last_known_good_events(event_id),
                 created_at INTEGER NOT NULL,
                 PRIMARY KEY(context_id, idempotency_key)
             );
             CREATE INDEX last_known_good_receipt_result_event_idx
                 ON last_known_good_command_receipts(result_event_id);",
        )
        .map_err(|error| format!("Unable to initialize Diagnostic Context/LKG v13: {error}"))?;

    let contexts = load_contexts(transaction)?;
    migrate_candidates(transaction, &contexts)?;
    migrate_states(transaction)?;
    Ok(())
}

fn load_contexts(
    transaction: &Transaction<'_>,
) -> Result<std::collections::BTreeMap<String, crate::model::DiagnosticContext>, String> {
    let rows = {
        let mut statement = transaction
            .prepare("SELECT id, payload_json FROM diagnostic_contexts ORDER BY id")
            .map_err(|error| format!("Unable to inspect Diagnostic Context v13 rows: {error}"))?;
        statement
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
            .map_err(|error| format!("Unable to query Diagnostic Context v13 rows: {error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("Unable to read Diagnostic Context v13 rows: {error}"))?
    };
    let mut contexts = std::collections::BTreeMap::new();
    for (context_id, payload) in rows {
        let mut value = serde_json::from_str::<serde_json::Value>(&payload)
            .map_err(|error| format!("Diagnostic Context {context_id} cannot migrate to v13: {error}"))?;
        let object = value
            .as_object_mut()
            .ok_or_else(|| format!("Diagnostic Context {context_id} is not an object."))?;
        if !object.contains_key("id")
            || !object.contains_key("projectId")
            || !object.contains_key("expectedBehavior")
        {
            // Early schemas allowed opaque engineering-memory payloads. Preserve
            // them byte-for-byte; they are not promoted to a canonical v7 Context.
            continue;
        }
        object.remove("revision");
        object.insert("schemaVersion".into(), serde_json::json!(crate::model::DIAGNOSTIC_CONTEXT_SCHEMA));
        object.insert("contextDefinitionRevision".into(), serde_json::json!(1));
        let memory_revision = transaction
            .query_row(
                "SELECT COALESCE(MAX(revision), 0) FROM diagnostic_assertions WHERE context_id = ?1",
                params![&context_id],
                |row| row.get::<_, i64>(0),
            )
            .map_err(|error| format!("Unable to derive Context memory revision: {error}"))?;
        object.insert("memoryRevision".into(), serde_json::json!(memory_revision));
        object.insert("contextBasisFingerprint".into(), serde_json::json!(""));
        let provisional = serde_json::from_value::<crate::model::DiagnosticContext>(value.clone())
            .map_err(|error| format!("Diagnostic Context {context_id} cannot decode for basis migration: {error}"))?;
        let fingerprint = crate::model::diagnostic_context_basis_fingerprint(&provisional);
        value["contextBasisFingerprint"] = serde_json::json!(&fingerprint);
        let context = serde_json::from_value::<crate::model::DiagnosticContext>(value)
            .map_err(|error| format!("Diagnostic Context {context_id} cannot decode after basis migration: {error}"))?;
        crate::diagnostic::validate_context(&context)?;
        let encoded = serde_json::to_string(&context)
            .map_err(|error| format!("Unable to encode Diagnostic Context {context_id}: {error}"))?;
        transaction
            .execute(
                "UPDATE diagnostic_contexts
                 SET revision = 1, context_definition_revision = 1,
                     context_basis_fingerprint = ?1, memory_revision = ?2,
                     payload_json = ?3 WHERE id = ?4",
                params![fingerprint, memory_revision, encoded, context_id],
            )
            .map_err(|error| format!("Unable to persist Diagnostic Context v13: {error}"))?;
        contexts.insert(context.id.clone(), context);
    }
    Ok(contexts)
}

fn migrate_candidates(
    transaction: &Transaction<'_>,
    contexts: &std::collections::BTreeMap<String, crate::model::DiagnosticContext>,
) -> Result<(), String> {
    let rows = {
        let mut statement = transaction
            .prepare("SELECT candidate_id, context_id, payload_json FROM last_known_good_candidates ORDER BY candidate_id")
            .map_err(|error| format!("Unable to inspect LKG candidate v13 rows: {error}"))?;
        statement
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)))
            .map_err(|error| format!("Unable to query LKG candidate v13 rows: {error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("Unable to read LKG candidate v13 rows: {error}"))?
    };
    for (candidate_id, context_id, payload) in rows {
        let context = contexts
            .get(&context_id)
            .ok_or_else(|| format!("LKG candidate {candidate_id} has no Context."))?;
        let mut value = serde_json::from_str::<serde_json::Value>(&payload)
            .map_err(|error| format!("LKG candidate {candidate_id} cannot migrate: {error}"))?;
        let expected_matches = value["expectedBehaviorFingerprint"]
            == serde_json::json!(crate::model::expected_behavior_fingerprint(&context.expected_behavior));
        let fingerprint = if expected_matches {
            context.context_basis_fingerprint.clone()
        } else {
            "unavailable".to_string()
        };
        let object = value
            .as_object_mut()
            .ok_or_else(|| format!("LKG candidate {candidate_id} is not an object."))?;
        object.remove("contextRevision");
        object.insert("schemaVersion".into(), serde_json::json!(crate::model::LKG_CANDIDATE_SCHEMA));
        object.insert("contextDefinitionRevision".into(), serde_json::json!(1));
        object.insert("contextBasisFingerprint".into(), serde_json::json!(&fingerprint));
        if !expected_matches {
            let integrity = object
                .get_mut("integrity")
                .and_then(serde_json::Value::as_object_mut)
                .ok_or_else(|| format!("LKG candidate {candidate_id} integrity is invalid."))?;
            integrity.insert("status".into(), serde_json::json!(crate::model::LKG_INTEGRITY_INVALID));
            let limitations = integrity
                .entry("limitations")
                .or_insert_with(|| serde_json::json!([]))
                .as_array_mut()
                .ok_or_else(|| format!("LKG candidate {candidate_id} limitations are invalid."))?;
            limitations.push(serde_json::json!("legacy-context-basis-unavailable"));
        }
        let candidate = serde_json::from_value::<crate::model::LastKnownGoodCandidate>(value)
            .map_err(|error| format!("LKG candidate {candidate_id} cannot decode after migration: {error}"))?;
        let encoded = serde_json::to_string(&candidate)
            .map_err(|error| format!("Unable to encode LKG candidate {candidate_id}: {error}"))?;
        transaction
            .execute(
                "UPDATE last_known_good_candidates
                 SET context_revision = 1, context_definition_revision = 1,
                     context_basis_fingerprint = ?1, integrity_json = ?2,
                     payload_json = ?3 WHERE candidate_id = ?4",
                params![
                    candidate.context_basis_fingerprint,
                    serde_json::to_string(&candidate.integrity).map_err(|error| error.to_string())?,
                    encoded,
                    candidate_id
                ],
            )
            .map_err(|error| format!("Unable to persist LKG candidate v13: {error}"))?;
    }
    Ok(())
}

fn migrate_states(transaction: &Transaction<'_>) -> Result<(), String> {
    let rows = {
        let mut statement = transaction
            .prepare("SELECT context_id, payload_json FROM last_known_good_state ORDER BY context_id")
            .map_err(|error| format!("Unable to inspect LKG state v13 rows: {error}"))?;
        statement
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
            .map_err(|error| format!("Unable to query LKG state v13 rows: {error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("Unable to read LKG state v13 rows: {error}"))?
    };
    for (context_id, payload) in rows {
        let mut value = serde_json::from_str::<serde_json::Value>(&payload)
            .map_err(|error| format!("LKG state {context_id} cannot migrate: {error}"))?;
        value["schemaVersion"] = serde_json::json!(crate::model::LKG_STATE_SCHEMA);
        value["applicabilityStatus"] = serde_json::json!("unavailable");
        let encoded = serde_json::to_string(&value)
            .map_err(|error| format!("Unable to encode LKG state {context_id}: {error}"))?;
        transaction
            .execute(
                "UPDATE last_known_good_state SET payload_json = ?1 WHERE context_id = ?2",
                params![encoded, context_id],
            )
            .map_err(|error| format!("Unable to persist LKG state v13: {error}"))?;
    }
    Ok(())
}
