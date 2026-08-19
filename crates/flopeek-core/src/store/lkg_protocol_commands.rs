pub fn propose_last_known_good(
    root: &Path,
    request: LastKnownGoodProposalRequest,
) -> Result<LastKnownGoodCandidate, String> {
    let request_fingerprint = command_request_fingerprint("PROPOSE", &request)?;
    let mut connection = open(root)?;
    let transaction = connection
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|error| format!("Unable to begin LKG proposal: {error}"))?;
    if let Some(existing) =
        proposal_receipt_result(&transaction, &request, &request_fingerprint)?
    {
        transaction.rollback().ok();
        return Ok(existing);
    }
    let identity = crate::identity::resolve(root)?;
    let repository_project = identity.project_id.clone();
    let resolved_revision = crate::diagnostic::resolve_last_known_good_revision(root, &request.git_revision)?;
    let observation_exists = transaction
        .query_row(
            "SELECT 1 FROM graph_observations
             WHERE project_id = ?1 AND git_revision = ?2 AND dirty = 0 LIMIT 1",
            params![repository_project, resolved_revision],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(|error| format!("Unable to inspect LKG observation materialization: {error}"))?
        .is_some();
    if !observation_exists {
        let max_paths = bound(request.max_paths, DEFAULT_MAX_PATHS, HARD_MAX_PATHS);
        let max_snapshot_bytes = bound(
            request.max_snapshot_bytes,
            DEFAULT_MAX_SNAPSHOT_BYTES,
            HARD_MAX_SNAPSHOT_BYTES,
        );
        let limits = crate::model::DiagnosticLimits {
            max_commits: 1,
            max_candidates: 1,
            max_paths,
            max_context_refs: 1,
            max_assertions: 1,
            max_snapshot_bytes,
            max_packet_bytes: max_snapshot_bytes.min(128 * 1024),
        };
        let materialization = crate::diagnostic::build_historical_graph_materialization(
            root,
            &resolved_revision,
            &limits,
        )?;
        super::scan::persist_detached_observation(
            &transaction,
            root,
            materialization.graph,
            &materialization.facts,
        )?;
    }
    let candidate = candidate_for_request(&transaction, root, &request)?;
    if transaction
        .query_row(
            "SELECT 1 FROM last_known_good_candidates WHERE candidate_id = ?1",
            params![candidate.candidate_id],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(|error| format!("Unable to inspect LKG idempotency: {error}"))?
        .is_some()
    {
        return Err("lkg-command-receipt-missing".to_string());
    }
    let (_, lifecycle) = reduced(&transaction, &request.context_id)?;
    check_expected_tip(&lifecycle, request.expected_tip_event_id.as_deref())?;
    let candidate_json = candidate_payload(&candidate)?;
    let graph_basis_json = candidate
        .graph_basis
        .as_ref()
        .map(|value| encode(value, "GraphBasis"))
        .transpose()?;
    let contract_json = candidate
        .evidence_contract
        .as_ref()
        .map(|value| encode(value, "EvidenceContract"))
        .transpose()?;
    let evidence_json = encode(&candidate.evidence, "LKG evidence")?;
    let integrity_json = encode(&candidate.integrity, "LKG integrity")?;
    transaction.execute(
        "INSERT INTO last_known_good_candidates(
             candidate_id, repository_id, project_id, context_id,
             context_revision, context_definition_revision,
             context_basis_fingerprint, expected_behavior_fingerprint, git_revision,
             observation_id, graph_basis_json, evidence_contract_json,
             proposed_by, proposed_at, evidence_json, reason, integrity_json,
             payload_json
         ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)",
        params![candidate.candidate_id, candidate.repository_id, candidate.project_id,
            candidate.context_id, candidate.context_definition_revision as i64,
            candidate.context_definition_revision as i64, candidate.context_basis_fingerprint,
            candidate.expected_behavior_fingerprint, candidate.git_revision,
            candidate.observation_id, graph_basis_json, contract_json,
            candidate.proposed_by, candidate.proposed_at as i64, evidence_json,
            candidate.reason, integrity_json, candidate_json],
    ).map_err(|error| format!("Unable to persist LKG candidate: {error}"))?;
    let event = LastKnownGoodEvent {
        schema_version: LKG_EVENT_SCHEMA.to_string(),
        event_id: format!("lkge_{}", blake3::hash(format!("{}\0{}", request.context_id, request.idempotency_key).as_bytes()).to_hex()),
        repository_id: candidate.repository_id.clone(),
        project_id: candidate.project_id.clone(),
        context_id: candidate.context_id.clone(),
        event_type: "PROPOSE".to_string(),
        candidate_id: candidate.candidate_id.clone(),
        replaces_candidate_id: None,
        predecessor_event_id: lifecycle.state.tip_event_id.clone(),
        actor: request.actor,
        actor_kind: "agent-or-tool".to_string(),
        actor_trust: "untrusted-transport".to_string(),
        reason: candidate.reason.clone(),
        evidence: candidate.evidence.clone(),
        created_at: now_seconds() as u64,
        idempotency_key: request.idempotency_key,
    };
    let event_json = event_payload(&event)?;
    transaction.execute(
        "INSERT INTO last_known_good_events(
             event_id, repository_id, project_id, context_id, event_type,
             candidate_id, replaces_candidate_id, predecessor_event_id, actor,
             actor_kind, actor_trust, reason, evidence_json, created_at,
             idempotency_key, payload_json
         ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)",
        params![event.event_id, event.repository_id, event.project_id, event.context_id,
            event.event_type, event.candidate_id, event.replaces_candidate_id,
            event.predecessor_event_id, event.actor, event.actor_kind,
            event.actor_trust, event.reason, evidence_json, event.created_at as i64,
            event.idempotency_key, event_json],
    ).map_err(|error| format!("Unable to persist LKG proposal event: {error}"))?;
    persist_command_receipt(
        &transaction,
        &event.context_id,
        &event.idempotency_key,
        "PROPOSE",
        &request_fingerprint,
        Some(&event.candidate_id),
        &event.event_id,
        event.created_at,
    )?;
    let (candidates, lifecycle) = reduced(&transaction, &candidate.context_id)?;
    let state = state_with_applicability(&transaction, root, &candidates, lifecycle.state)?;
    persist_state(&transaction, &state)?;
    update_context_projection(
        &transaction,
        &candidate.context_id,
        state.active_candidate_id.as_deref(),
    )?;
    transaction.commit().map_err(|error| format!("Unable to commit LKG proposal: {error}"))?;
    Ok(candidate)
}

fn transition_last_known_good(
    root: &Path,
    request: LastKnownGoodTransitionRequest,
    event_type: &str,
) -> Result<LastKnownGoodEvent, String> {
    validate_text(&request.actor, "actor", 256)?;
    validate_text(&request.reason, "reason", 4_096)?;
    validate_text(&request.idempotency_key, "idempotencyKey", 128)?;
    validate_evidence(&request.evidence)?;
    let request_fingerprint = command_request_fingerprint(event_type, &request)?;
    let mut connection = open(root)?;
    let transaction = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|error| format!("Unable to begin LKG transition: {error}"))?;
    if let Some(existing) = transition_receipt_result(
        &transaction,
        &request,
        event_type,
        &request_fingerprint,
    )? {
        return Ok(existing);
    }
    let (candidates, lifecycle) = reduced(&transaction, &request.context_id)?;
    check_expected_tip(&lifecycle, request.expected_tip_event_id.as_deref())?;
    let candidate_id = request.candidate_id.clone().or_else(|| {
        if event_type == "REVOKE" { lifecycle.state.active_candidate_id.clone() } else { lifecycle.state.pending_candidate_id.clone() }
    }).ok_or_else(|| format!("lkg-{}-target-unavailable", event_type.to_lowercase()))?;
    let candidate = candidates.iter().find(|candidate| candidate.candidate_id == candidate_id)
        .ok_or_else(|| "lkg-target-candidate-unavailable".to_string())?;
    if event_type == "CONFIRM" && candidate.integrity.status != LKG_INTEGRITY_COMPLETE {
        return Err("lkg-confirm-candidate-not-complete".to_string());
    }
    if event_type == "CONFIRM" {
        let context = context_snapshot(&transaction, &request.context_id)?;
        let applicability = evaluate_applicability(&transaction, root, candidate, &context)?;
        if applicability.status != "applicable" {
            return Err(format!(
                "lkg-confirm-candidate-not-applicable:{}",
                applicability.status
            ));
        }
    }
    let replaces = if event_type == "CONFIRM" { lifecycle.state.active_candidate_id.clone() } else { None };
    let event = LastKnownGoodEvent {
        schema_version: LKG_EVENT_SCHEMA.to_string(),
        event_id: format!("lkge_{}", blake3::hash(format!("{}\0{}", request.context_id, request.idempotency_key).as_bytes()).to_hex()),
        repository_id: candidate.repository_id.clone(),
        project_id: candidate.project_id.clone(),
        context_id: request.context_id.clone(),
        event_type: event_type.to_string(),
        candidate_id,
        replaces_candidate_id: replaces,
        predecessor_event_id: lifecycle.state.tip_event_id.clone(),
        actor: request.actor,
        actor_kind: "human".to_string(),
        actor_trust: "local-trusted-action-caller-attributed".to_string(),
        reason: request.reason,
        evidence: request.evidence,
        created_at: now_seconds() as u64,
        idempotency_key: request.idempotency_key,
    };
    let event_json = event_payload(&event)?;
    let evidence_json = encode(&event.evidence, "LKG evidence")?;
    transaction.execute(
        "INSERT INTO last_known_good_events(
             event_id, repository_id, project_id, context_id, event_type,
             candidate_id, replaces_candidate_id, predecessor_event_id, actor,
             actor_kind, actor_trust, reason, evidence_json, created_at,
             idempotency_key, payload_json
         ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)",
        params![event.event_id, event.repository_id, event.project_id, event.context_id,
            event.event_type, event.candidate_id, event.replaces_candidate_id,
            event.predecessor_event_id, event.actor, event.actor_kind,
            event.actor_trust, event.reason, evidence_json, event.created_at as i64,
            event.idempotency_key, event_json],
    ).map_err(|error| format!("Unable to persist LKG transition: {error}"))?;
    persist_command_receipt(
        &transaction,
        &event.context_id,
        &event.idempotency_key,
        event_type,
        &request_fingerprint,
        Some(&event.candidate_id),
        &event.event_id,
        event.created_at,
    )?;
    let (candidates, lifecycle) = reduced(&transaction, &request.context_id)?;
    let state = state_with_applicability(&transaction, root, &candidates, lifecycle.state)?;
    persist_state(&transaction, &state)?;
    update_context_projection(
        &transaction,
        &request.context_id,
        state.active_candidate_id.as_deref(),
    )?;
    transaction.commit().map_err(|error| format!("Unable to commit LKG transition: {error}"))?;
    Ok(event)
}

pub fn confirm_last_known_good_local(root: &Path, request: LastKnownGoodTransitionRequest) -> Result<LastKnownGoodEvent, String> {
    transition_last_known_good(root, request, "CONFIRM")
}

pub fn reject_last_known_good_local(root: &Path, request: LastKnownGoodTransitionRequest) -> Result<LastKnownGoodEvent, String> {
    transition_last_known_good(root, request, "REJECT")
}

pub fn revoke_last_known_good_local(root: &Path, request: LastKnownGoodTransitionRequest) -> Result<LastKnownGoodEvent, String> {
    transition_last_known_good(root, request, "REVOKE")
}
