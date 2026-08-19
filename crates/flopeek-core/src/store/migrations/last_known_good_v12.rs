/// Canonical Last-Known-Good Protocol 1.0 storage.  The v2 binding table is
/// intentionally retained as raw legacy evidence; migration never deletes or
/// rewrites that historical authority.
pub(crate) fn migration_v12(transaction: &Transaction<'_>) -> Result<(), String> {
    transaction
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS last_known_good_candidates (
                 candidate_id TEXT PRIMARY KEY NOT NULL,
                 repository_id TEXT NOT NULL,
                 project_id TEXT NOT NULL,
                 context_id TEXT NOT NULL REFERENCES diagnostic_contexts(id),
                 context_revision INTEGER NOT NULL,
                 expected_behavior_fingerprint TEXT NOT NULL,
                 git_revision TEXT NOT NULL,
                 observation_id TEXT,
                 graph_basis_json TEXT,
                 evidence_contract_json TEXT,
                 proposed_by TEXT NOT NULL,
                 proposed_at INTEGER NOT NULL,
                 evidence_json TEXT NOT NULL,
                 reason TEXT NOT NULL,
                 integrity_json TEXT NOT NULL,
                 payload_json TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS last_known_good_events (
                 event_id TEXT PRIMARY KEY NOT NULL,
                 repository_id TEXT NOT NULL,
                 project_id TEXT NOT NULL,
                 context_id TEXT NOT NULL REFERENCES diagnostic_contexts(id),
                 event_type TEXT NOT NULL CHECK(event_type IN ('PROPOSE','CONFIRM','REJECT','REVOKE')),
                 candidate_id TEXT NOT NULL REFERENCES last_known_good_candidates(candidate_id),
                 replaces_candidate_id TEXT REFERENCES last_known_good_candidates(candidate_id),
                 predecessor_event_id TEXT REFERENCES last_known_good_events(event_id),
                 actor TEXT NOT NULL,
                 actor_kind TEXT NOT NULL,
                 actor_trust TEXT NOT NULL,
                 reason TEXT NOT NULL,
                 evidence_json TEXT NOT NULL,
                 created_at INTEGER NOT NULL,
                 idempotency_key TEXT NOT NULL,
                 payload_json TEXT NOT NULL,
                 UNIQUE(context_id, idempotency_key)
             );
             CREATE TABLE IF NOT EXISTS last_known_good_state (
                 context_id TEXT PRIMARY KEY NOT NULL REFERENCES diagnostic_contexts(id),
                 tip_event_id TEXT REFERENCES last_known_good_events(event_id),
                 active_candidate_id TEXT REFERENCES last_known_good_candidates(candidate_id),
                 pending_candidate_id TEXT REFERENCES last_known_good_candidates(candidate_id),
                 payload_json TEXT NOT NULL
             );
             CREATE INDEX IF NOT EXISTS last_known_good_candidate_context_idx
                 ON last_known_good_candidates(context_id, proposed_at, candidate_id);
             CREATE INDEX IF NOT EXISTS last_known_good_event_context_idx
                 ON last_known_good_events(context_id, created_at, event_id);
             CREATE INDEX IF NOT EXISTS last_known_good_event_candidate_idx
                 ON last_known_good_events(context_id, candidate_id, created_at);
             CREATE INDEX IF NOT EXISTS last_known_good_event_idempotency_idx
                 ON last_known_good_events(context_id, idempotency_key);
             CREATE INDEX IF NOT EXISTS last_known_good_state_tip_idx
                 ON last_known_good_state(tip_event_id);",
        )
        .map_err(|error| format!("Unable to initialize LKG Protocol 1.0 schema: {error}"))?;

    let contexts = {
        let mut statement = transaction
            .prepare("SELECT id, payload_json FROM diagnostic_contexts")
            .map_err(|error| format!("Unable to inspect Diagnostic Context schemas: {error}"))?;
        statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|error| format!("Unable to enumerate Diagnostic Context schemas: {error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("Unable to read Diagnostic Context schemas: {error}"))?
    };
    for (context_id, payload) in contexts {
        let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&payload) else {
            continue;
        };
        let Some(object) = value.as_object_mut() else {
            continue;
        };
        if !object.contains_key("id") || !object.contains_key("expectedBehavior") {
            continue;
        }
        object.insert(
            "schemaVersion".to_string(),
            serde_json::Value::String(crate::model::DIAGNOSTIC_CONTEXT_SCHEMA.to_string()),
        );
        object
            .entry("lastKnownGoodCandidateId".to_string())
            .or_insert(serde_json::Value::Null);
        let encoded = serde_json::to_string(&value).map_err(|error| {
            format!("Unable to encode Diagnostic Context {context_id}: {error}")
        })?;
        transaction
            .execute(
                "UPDATE diagnostic_contexts SET payload_json = ?1 WHERE id = ?2",
                params![encoded, context_id],
            )
            .map_err(|error| {
                format!("Unable to migrate Diagnostic Context {context_id}: {error}")
            })?;
    }
    migrate_legacy_lkg(transaction)?;
    Ok(())
}

fn migrate_legacy_lkg(transaction: &Transaction<'_>) -> Result<(), String> {
    let rows = {
        let mut statement = transaction
            .prepare(
                "SELECT payload_json FROM last_known_good_bindings
                 ORDER BY context_id, created_at, binding_id",
            )
            .map_err(|error| format!("Unable to inspect legacy LKG rows: {error}"))?;
        statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|error| format!("Unable to enumerate legacy LKG rows: {error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("Unable to read legacy LKG rows: {error}"))?
    };
    let mut grouped =
        std::collections::BTreeMap::<String, Vec<crate::model::LastKnownGoodBinding>>::new();
    for payload in rows {
        let binding = serde_json::from_str::<crate::model::LastKnownGoodBinding>(&payload)
            .map_err(|error| format!("Unable to decode legacy LKG binding: {error}"))?;
        grouped
            .entry(binding.context_id.clone())
            .or_default()
            .push(binding);
    }
    for (context_id, mut bindings) in grouped {
        bindings.sort_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then_with(|| left.binding_id.cmp(&right.binding_id))
        });
        let context_payload = transaction
            .query_row(
                "SELECT payload_json FROM diagnostic_contexts WHERE id = ?1",
                params![context_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| format!("Unable to read legacy LKG Context: {error}"))?;
        let Some(context_payload) = context_payload else {
            continue;
        };
        let Ok(context) = serde_json::from_str::<crate::model::DiagnosticContext>(&context_payload)
        else {
            continue;
        };
        let reduced = crate::model::reduce_last_known_good_lifecycle(bindings.clone());
        let Ok(reduced) = reduced else {
            let state = crate::model::LastKnownGoodState {
                schema_version: crate::model::LKG_STATE_SCHEMA.to_string(),
                context_id: context_id.clone(),
                lifecycle_status: "corrupt".to_string(),
                applicability_status: "unavailable".to_string(),
                limitations: vec!["legacy-lkg-semantics-ambiguous".to_string()],
                ..Default::default()
            };
            let payload = serde_json::to_string(&state).map_err(|error| error.to_string())?;
            transaction.execute(
                "INSERT OR REPLACE INTO last_known_good_state(context_id, payload_json) VALUES(?1,?2)",
                params![context_id, payload],
            ).map_err(|error| format!("Unable to quarantine legacy LKG state: {error}"))?;
            continue;
        };
        let mut candidates =
            std::collections::BTreeMap::<String, crate::model::LastKnownGoodCandidate>::new();
        let mut candidate_for_binding = std::collections::BTreeMap::<String, String>::new();
        let mut events = Vec::new();
        let mut previous_event: Option<String> = None;
        let mut quarantine = false;
        for binding in reduced.history {
            if binding.status == "superseded" {
                quarantine = true;
                break;
            }
            let graph_contract = if let Some(basis) = binding.graph_basis.as_ref() {
                transaction
                    .query_row(
                        "SELECT graph_schema_version, graph_derivation_id, node_fingerprint_contract
                         FROM graph_versions WHERE graph_version = ?1",
                        params![basis.graph_version as i64],
                        |row| Ok(crate::model::EvidenceContract {
                            graph_schema_version: row.get(0)?,
                            graph_derivation_id: row.get(1)?,
                            node_fingerprint_contract: row.get(2)?,
                        }),
                    )
                    .optional()
                    .map_err(|error| {
                        format!(
                            "Unable to validate legacy LKG evidence contract for {}: {error}",
                            binding.binding_id
                        )
                    })?
            } else {
                None
            };
            let target_candidate = binding
                .target_binding_id
                .as_ref()
                .and_then(|target| candidate_for_binding.get(target).cloned());
            match binding.status.as_str() {
                "proposed" => {
                    let candidate_id = format!("lkgc_legacy_{}", binding.binding_id);
                    let candidate =
                        legacy_candidate(&binding, &context, candidate_id.clone(), graph_contract);
                    candidate_for_binding.insert(binding.binding_id.clone(), candidate_id.clone());
                    candidates.insert(candidate_id.clone(), candidate.clone());
                    events.push(legacy_event(
                        &binding,
                        "PROPOSE",
                        candidate_id.clone(),
                        None,
                        previous_event.clone(),
                        "proposal",
                        binding.created_at,
                    ));
                }
                "confirmed" => {
                    let candidate_id = if let Some(target) = target_candidate {
                        target
                    } else {
                        let candidate_id = format!("lkgc_legacy_{}", binding.binding_id);
                        let candidate = legacy_candidate(
                            &binding,
                            &context,
                            candidate_id.clone(),
                            graph_contract,
                        );
                        candidate_for_binding
                            .insert(binding.binding_id.clone(), candidate_id.clone());
                        candidates.insert(candidate_id.clone(), candidate);
                        let proposal = legacy_event(
                            &binding,
                            "PROPOSE",
                            candidate_id.clone(),
                            None,
                            previous_event.clone(),
                            "synthetic-proposal",
                            binding.created_at.saturating_sub(1),
                        );
                        previous_event = Some(proposal.event_id.clone());
                        events.push(proposal);
                        candidate_id
                    };
                    let replaces = binding
                        .supersedes_binding_id
                        .as_ref()
                        .and_then(|target| candidate_for_binding.get(target).cloned());
                    let event = legacy_event(
                        &binding,
                        "CONFIRM",
                        candidate_id.clone(),
                        replaces,
                        previous_event.clone(),
                        "confirmation",
                        binding.created_at,
                    );
                    candidate_for_binding.insert(binding.binding_id.clone(), candidate_id.clone());
                    previous_event = Some(event.event_id.clone());
                    events.push(event);
                }
                "rejected" => {
                    let Some(candidate_id) = target_candidate else {
                        quarantine = true;
                        break;
                    };
                    let event = legacy_event(
                        &binding,
                        "REJECT",
                        candidate_id,
                        None,
                        previous_event.clone(),
                        "rejection",
                        binding.created_at,
                    );
                    previous_event = Some(event.event_id.clone());
                    events.push(event);
                }
                "revoked" => {
                    let Some(candidate_id) = target_candidate.or_else(|| {
                        binding
                            .target_binding_id
                            .as_ref()
                            .and_then(|target| candidate_for_binding.get(target).cloned())
                    }) else {
                        quarantine = true;
                        break;
                    };
                    let event = legacy_event(
                        &binding,
                        "REVOKE",
                        candidate_id,
                        None,
                        previous_event.clone(),
                        "revocation",
                        binding.created_at,
                    );
                    previous_event = Some(event.event_id.clone());
                    events.push(event);
                }
                _ => {
                    quarantine = true;
                    break;
                }
            }
        }
        if quarantine {
            let state = crate::model::LastKnownGoodState {
                schema_version: crate::model::LKG_STATE_SCHEMA.to_string(),
                context_id: context_id.clone(),
                lifecycle_status: "corrupt".to_string(),
                applicability_status: "unavailable".to_string(),
                limitations: vec!["legacy-lkg-semantics-ambiguous".to_string()],
                ..Default::default()
            };
            let payload = serde_json::to_string(&state).map_err(|error| error.to_string())?;
            transaction.execute(
                "INSERT OR REPLACE INTO last_known_good_state(context_id, payload_json) VALUES(?1,?2)",
                params![context_id, payload],
            ).map_err(|error| format!("Unable to quarantine legacy LKG state: {error}"))?;
            continue;
        }
        for candidate in candidates.values() {
            let payload = serde_json::to_string(candidate).map_err(|error| error.to_string())?;
            let graph_basis = candidate
                .graph_basis
                .as_ref()
                .map(serde_json::to_string)
                .transpose()
                .map_err(|error| error.to_string())?;
            let evidence_contract = candidate
                .evidence_contract
                .as_ref()
                .map(serde_json::to_string)
                .transpose()
                .map_err(|error| error.to_string())?;
            let evidence =
                serde_json::to_string(&candidate.evidence).map_err(|error| error.to_string())?;
            let integrity =
                serde_json::to_string(&candidate.integrity).map_err(|error| error.to_string())?;
            transaction
                .execute(
                    "INSERT OR IGNORE INTO last_known_good_candidates(
                     candidate_id, repository_id, project_id, context_id, context_revision,
                     expected_behavior_fingerprint, git_revision, observation_id,
                     graph_basis_json, evidence_contract_json, proposed_by, proposed_at,
                     evidence_json, reason, integrity_json, payload_json
                 ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)",
                    params![
                        candidate.candidate_id,
                        candidate.repository_id,
                        candidate.project_id,
                        candidate.context_id,
                        candidate.context_revision as i64,
                        candidate.expected_behavior_fingerprint,
                        candidate.git_revision,
                        candidate.observation_id,
                        graph_basis,
                        evidence_contract,
                        candidate.proposed_by,
                        candidate.proposed_at as i64,
                        evidence,
                        candidate.reason,
                        integrity,
                        payload
                    ],
                )
                .map_err(|error| format!("Unable to migrate legacy LKG candidate: {error}"))?;
        }
        for event in &events {
            let payload = serde_json::to_string(event).map_err(|error| error.to_string())?;
            let evidence =
                serde_json::to_string(&event.evidence).map_err(|error| error.to_string())?;
            transaction
                .execute(
                    "INSERT OR IGNORE INTO last_known_good_events(
                     event_id, repository_id, project_id, context_id, event_type, candidate_id,
                     replaces_candidate_id, predecessor_event_id, actor, actor_kind, actor_trust,
                     reason, evidence_json, created_at, idempotency_key, payload_json
                 ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)",
                    params![
                        event.event_id,
                        event.repository_id,
                        event.project_id,
                        event.context_id,
                        event.event_type,
                        event.candidate_id,
                        event.replaces_candidate_id,
                        event.predecessor_event_id,
                        event.actor,
                        event.actor_kind,
                        event.actor_trust,
                        event.reason,
                        evidence,
                        event.created_at as i64,
                        event.idempotency_key,
                        payload
                    ],
                )
                .map_err(|error| format!("Unable to migrate legacy LKG event: {error}"))?;
        }
        let candidate_values = candidates.values().cloned().collect::<Vec<_>>();
        let reduced_new =
            crate::model::reduce_last_known_good(&context_id, &candidate_values, &events)
                .map_err(|reason| format!("legacy-lkg-migration-reduction-failed:{reason}"))?;
        let state_payload =
            serde_json::to_string(&reduced_new.state).map_err(|error| error.to_string())?;
        transaction
            .execute(
                "INSERT OR REPLACE INTO last_known_good_state(
                 context_id, tip_event_id, active_candidate_id, pending_candidate_id, payload_json
             ) VALUES(?1,?2,?3,?4,?5)",
                params![
                    context_id,
                    reduced_new.state.tip_event_id,
                    reduced_new.state.active_candidate_id,
                    reduced_new.state.pending_candidate_id,
                    state_payload
                ],
            )
            .map_err(|error| format!("Unable to persist migrated LKG state: {error}"))?;
        transaction.execute(
            "UPDATE diagnostic_contexts SET payload_json = json_set(payload_json, '$.lastKnownGoodCandidateId', ?1) WHERE id = ?2",
            params![reduced_new.state.active_candidate_id, context_id],
        ).map_err(|error| format!("Unable to update migrated LKG projection: {error}"))?;
    }
    Ok(())
}

fn legacy_candidate(
    binding: &crate::model::LastKnownGoodBinding,
    context: &crate::model::DiagnosticContext,
    candidate_id: String,
    evidence_contract: Option<crate::model::EvidenceContract>,
) -> crate::model::LastKnownGoodCandidate {
    let (status, limitations) = if binding.validation.status == "valid" {
        ("complete", Vec::new())
    } else if binding.graph_basis.is_some() {
        (
            "invalid",
            vec!["legacy-candidate-validation-unverified".to_string()],
        )
    } else {
        (
            "partial",
            vec!["legacy-observation-basis-unavailable".to_string()],
        )
    };
    crate::model::LastKnownGoodCandidate {
        schema_version: crate::model::LKG_CANDIDATE_SCHEMA.to_string(),
        candidate_id,
        repository_id: binding.repository_id.clone(),
        project_id: binding.project_id.clone(),
        context_id: binding.context_id.clone(),
        context_revision: context.revision,
        expected_behavior_fingerprint: crate::model::expected_behavior_fingerprint(
            &context.expected_behavior,
        ),
        git_revision: binding.git_revision.clone(),
        observation_id: binding.observation_id.clone(),
        graph_basis: binding.graph_basis.clone(),
        evidence_contract,
        proposed_by: binding.actor.clone(),
        proposed_at: binding.created_at,
        evidence: binding.evidence.clone(),
        reason: "migrated-from-last-known-good-binding-v2".to_string(),
        integrity: crate::model::LastKnownGoodIntegrity {
            status: status.to_string(),
            revision_available: binding.validation.revision_available,
            observation_available: binding.observation_id.is_some(),
            graph_basis_available: binding.graph_basis.is_some(),
            evidence_contract_compatible: binding.validation.evidence_contract_compatible,
            limitations,
        },
    }
}

fn legacy_event(
    binding: &crate::model::LastKnownGoodBinding,
    event_type: &str,
    candidate_id: String,
    replaces_candidate_id: Option<String>,
    predecessor_event_id: Option<String>,
    suffix: &str,
    created_at: u64,
) -> crate::model::LastKnownGoodEvent {
    let id_material = format!(
        "flopeek-lkg-legacy-event/v1\0{}\0{}\0{}",
        binding.binding_id, event_type, suffix
    );
    crate::model::LastKnownGoodEvent {
        schema_version: crate::model::LKG_EVENT_SCHEMA.to_string(),
        event_id: format!("lkge_{}", blake3::hash(id_material.as_bytes()).to_hex()),
        repository_id: binding.repository_id.clone(),
        project_id: binding.project_id.clone(),
        context_id: binding.context_id.clone(),
        event_type: event_type.to_string(),
        candidate_id,
        replaces_candidate_id,
        predecessor_event_id,
        actor: binding.actor.clone(),
        actor_kind: binding.actor_kind.clone(),
        actor_trust: "legacy-v2-binding".to_string(),
        reason: "migrated-from-last-known-good-binding-v2".to_string(),
        evidence: binding.evidence.clone(),
        created_at,
        idempotency_key: format!("legacy:{}:{}:{}", binding.binding_id, event_type, suffix),
    }
}
