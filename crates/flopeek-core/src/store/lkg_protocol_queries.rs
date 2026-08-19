pub fn get_last_known_good_protocol(root: &Path, context_id: &str) -> Result<LastKnownGoodState, String> {
    let connection = open(root)?;
    let transaction = connection.unchecked_transaction().map_err(|error| format!("Unable to read LKG state: {error}"))?;
    let (candidates, lifecycle) = match reduced(&transaction, context_id) {
        Ok(value) => value,
        Err(reason) => {
            transaction.rollback().ok();
            return Ok(LastKnownGoodState {
                schema_version: crate::model::LKG_STATE_SCHEMA.to_string(),
                context_id: context_id.to_string(),
                lifecycle_status: "corrupt".to_string(),
                applicability_status: "unavailable".to_string(),
                limitations: vec![reason],
                ..Default::default()
            });
        }
    };
    let stored = transaction
        .query_row(
            "SELECT payload_json FROM last_known_good_state WHERE context_id = ?1",
            params![context_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| format!("Unable to read materialized LKG state: {error}"))?;
    let mut state = lifecycle.state;
    if let Some(payload) = stored {
        match decode::<LastKnownGoodState>(&payload, "LastKnownGoodState") {
            Ok(materialized)
                if materialized.schema_version == state.schema_version
                    && materialized.context_id == state.context_id
                    && materialized.tip_event_id == state.tip_event_id
                    && materialized.active_candidate_id == state.active_candidate_id
                    && materialized.pending_candidate_id == state.pending_candidate_id
                    && materialized.lifecycle_status == state.lifecycle_status => {}
            Ok(_) | Err(_) => {
                state.lifecycle_status = "corrupt".to_string();
                state.applicability_status = "unavailable".to_string();
                state.limitations.push("lkg-materialized-state-mismatch".to_string());
                transaction.rollback().ok();
                return Ok(state);
            }
        }
    } else if !lifecycle.events.is_empty() {
        state.lifecycle_status = "corrupt".to_string();
        state.applicability_status = "unavailable".to_string();
        state.limitations.push("lkg-materialized-state-missing".to_string());
        transaction.rollback().ok();
        return Ok(state);
    }
    let context = context_snapshot(&transaction, context_id)?;
    if context.last_known_good_candidate_id != state.active_candidate_id {
        state.lifecycle_status = "corrupt".to_string();
        state.applicability_status = "unavailable".to_string();
        state.limitations.push("lkg-context-projection-mismatch".to_string());
        transaction.rollback().ok();
        return Ok(state);
    }
    let candidate_id = state
        .active_candidate_id
        .as_deref()
        .or(state.pending_candidate_id.as_deref());
    if let Some(candidate_id) = candidate_id {
        if let Some(candidate) = candidates.iter().find(|candidate| candidate.candidate_id == candidate_id) {
            let applicability = evaluate_applicability(&transaction, root, candidate, &context)?;
            state.applicability_status = applicability.status;
            state.limitations.extend(applicability.limitations);
        } else {
            state.applicability_status = "unavailable".to_string();
            state.limitations.push("lkg-state-candidate-unavailable".to_string());
        }
    }
    transaction.rollback().ok();
    state.limitations.sort();
    state.limitations.dedup();
    Ok(state)
}

pub fn list_last_known_good_protocol(
    root: &Path,
    context_id: &str,
    limit: usize,
) -> Result<LastKnownGoodHistory, String> {
    let connection = open(root)?;
    let transaction = connection
        .unchecked_transaction()
        .map_err(|error| format!("Unable to read LKG history: {error}"))?;
    let lifecycle = match reduced(&transaction, context_id) {
        Ok((_, lifecycle)) => lifecycle,
        Err(reason) => {
            transaction.rollback().ok();
            return Ok(LastKnownGoodHistory {
                schema_version: LKG_HISTORY_SCHEMA.to_string(),
                status: "unavailable".to_string(),
                reason: Some(reason.clone()),
                context_id: context_id.to_string(),
                tip_event_id: None,
                total_events: 0,
                events: Vec::new(),
                truncated: false,
                omissions: Vec::new(),
                limitations: vec![reason],
            });
        }
    };
    transaction.rollback().ok();
    let hard_limit = limit.min(1_024);
    let total_events = lifecycle.events.len();
    let omitted = total_events.saturating_sub(hard_limit);
    let events = lifecycle.events.into_iter().skip(omitted).collect();
    Ok(LastKnownGoodHistory {
        schema_version: LKG_HISTORY_SCHEMA.to_string(),
        status: "complete".to_string(),
        reason: None,
        context_id: context_id.to_string(),
        tip_event_id: lifecycle.state.tip_event_id,
        total_events,
        events,
        truncated: omitted > 0,
        omissions: (omitted > 0)
            .then(|| format!("{omitted} earlier LKG events omitted by history limit"))
            .into_iter()
            .collect(),
        limitations: vec![
            "LKG history order follows predecessorEventId; createdAt is display metadata only."
                .to_string(),
        ],
    })
}

pub fn validate_last_known_good_protocol(root: &Path, context_id: &str) -> Result<LastKnownGoodState, String> {
    get_last_known_good_protocol(root, context_id)
}

pub fn get_last_known_good_review_packet(root: &Path, context_id: &str) -> Result<LastKnownGoodReviewPacket, String> {
    let connection = open(root)?;
    let transaction = connection.unchecked_transaction().map_err(|error| format!("Unable to read LKG review packet: {error}"))?;
    let (candidates, reduced) = reduced(&transaction, context_id)?;
    let candidate_id = reduced.state.pending_candidate_id.as_ref().or(reduced.state.active_candidate_id.as_ref())
        .ok_or_else(|| "lkg-review-candidate-unavailable".to_string())?;
    let candidate = candidates
        .iter()
        .find(|value| &value.candidate_id == candidate_id)
        .cloned()
        .ok_or_else(|| "lkg-review-candidate-unavailable".to_string())?;
    let context = context_snapshot(&transaction, context_id)?;
    let applicability = evaluate_applicability(&transaction, root, &candidate, &context)?;
    let state = state_with_applicability(&transaction, root, &candidates, reduced.state)?;
    let current_observation_id = transaction
        .query_row(
            "SELECT current_observation_id FROM project_state WHERE project_id = ?1",
            params![candidate.project_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| format!("Unable to read current LKG observation: {error}"))?;
    let candidate_observation_id = candidate.observation_id.clone();
    let confirmable = candidate.integrity.status == LKG_INTEGRITY_COMPLETE
        && applicability.status == "applicable"
        && state.pending_candidate_id.is_some();
    transaction.rollback().ok();
    let structural_delta = match (candidate_observation_id.as_deref(), current_observation_id.as_deref()) {
        (Some(from), Some(to)) => Some(serde_json::to_value(crate::store::change::compare_observation_ids(
            &connection,
            &candidate.project_id,
            from,
            to,
            crate::temporal::DeltaLimits::default(),
        )?).map_err(|error| format!("Unable to encode LKG structural delta: {error}"))?),
        _ => Some(serde_json::json!({
            "schemaVersion": crate::model::OBSERVATION_DELTA_SCHEMA,
            "status": "unavailable",
            "reason": "candidate-to-current-observation-unavailable",
            "truncated": false,
            "omissions": ["candidate-to-current-structural-delta-unavailable"],
            "limitations": ["LKG review does not infer structural or runtime evidence when an observation basis is missing."]
        })),
    };
    let mut limitations = state.limitations.clone();
    limitations.extend(applicability.limitations.clone());
    if let Some(delta) = structural_delta.as_ref() {
        if let Some(values) = delta.get("omissions").and_then(serde_json::Value::as_array) {
            limitations.extend(values.iter().filter_map(serde_json::Value::as_str).map(ToOwned::to_owned));
        }
        if let Some(values) = delta.get("limitations").and_then(serde_json::Value::as_array) {
            limitations.extend(values.iter().filter_map(serde_json::Value::as_str).map(ToOwned::to_owned));
        }
    }
    limitations.sort();
    limitations.dedup();
    Ok(LastKnownGoodReviewPacket {
        schema_version: LKG_REVIEW_PACKET_SCHEMA.to_string(),
        context_id: context_id.to_string(),
        context,
        candidate,
        state,
        applicability,
        structural_delta,
        confirmable,
        limitations,
    })
}

pub(crate) struct ActiveProtocolEvaluation {
    pub candidate: Option<LastKnownGoodCandidate>,
    pub state: LastKnownGoodState,
    pub applicability: Option<LastKnownGoodApplicability>,
    pub usable_for_diagnosis: bool,
}

pub(crate) fn active_protocol_evaluation(
    root: &Path,
    context_id: &str,
) -> Result<ActiveProtocolEvaluation, String> {
    let materialized = get_last_known_good_protocol(root, context_id)?;
    if materialized.lifecycle_status == "corrupt" {
        return Ok(ActiveProtocolEvaluation {
            candidate: None,
            state: materialized,
            applicability: None,
            usable_for_diagnosis: false,
        });
    }
    let connection = open(root)?;
    let transaction = connection.unchecked_transaction().map_err(|error| format!("Unable to inspect active LKG: {error}"))?;
    let (candidates, reduced) = reduced(&transaction, context_id)?;
    let Some(active) = reduced.state.active_candidate_id else {
        transaction.rollback().ok();
        return Ok(ActiveProtocolEvaluation {
            candidate: None,
            state: materialized,
            applicability: None,
            usable_for_diagnosis: false,
        });
    };
    let candidate = candidates.into_iter().find(|value| value.candidate_id == active);
    let applicability = if let Some(candidate) = candidate.as_ref() {
        let context = context_snapshot(&transaction, context_id)?;
        Some(evaluate_applicability(&transaction, root, candidate, &context)?)
    } else {
        None
    };
    transaction.rollback().ok();
    let usable_for_diagnosis = candidate
        .as_ref()
        .is_some_and(|value| value.integrity.status == LKG_INTEGRITY_COMPLETE)
        && applicability
            .as_ref()
            .is_some_and(|value| value.status == "applicable");
    Ok(ActiveProtocolEvaluation {
        candidate,
        state: materialized,
        applicability,
        usable_for_diagnosis,
    })
}

pub(crate) fn evaluate_applicability(
    transaction: &Transaction<'_>,
    root: &Path,
    candidate: &LastKnownGoodCandidate,
    context: &crate::model::DiagnosticContext,
) -> Result<LastKnownGoodApplicability, String> {
    let limitations = Vec::new();
    let identity = crate::identity::resolve(root)?;
    if candidate.repository_id != identity.repository_id.as_deref().unwrap_or_default()
        || candidate.project_id != identity.project_id
    {
        return Ok(LastKnownGoodApplicability {
            status: "repository-mismatch".to_string(),
            limitations: vec!["repository-identity-mismatch".to_string()],
        });
    }
    if candidate.context_definition_revision != context.context_definition_revision
        || candidate.context_basis_fingerprint != context.context_basis_fingerprint
        || candidate.expected_behavior_fingerprint
            != crate::model::expected_behavior_fingerprint(&context.expected_behavior)
    {
        return Ok(LastKnownGoodApplicability {
            status: "context-basis-mismatch".to_string(),
            limitations: vec!["diagnostic-context-definition-or-expected-behavior-changed".to_string()],
        });
    }
    if candidate.integrity.status != LKG_INTEGRITY_COMPLETE {
        return Ok(LastKnownGoodApplicability {
            status: "basis-unavailable".to_string(),
            limitations: candidate.integrity.limitations.clone(),
        });
    }
    let Some(basis) = candidate.graph_basis.as_ref() else {
        return Ok(LastKnownGoodApplicability {
            status: "basis-unavailable".to_string(),
            limitations: vec!["candidate-graph-basis-unavailable".to_string()],
        });
    };
    if basis.project_id != candidate.project_id
        || basis.source_revision != candidate.git_revision
        || candidate.observation_id.as_deref() != Some(basis.observation_id.as_str())
    {
        return Ok(LastKnownGoodApplicability {
            status: "basis-unavailable".to_string(),
            limitations: vec!["last-known-good-basis-provenance-mismatch".to_string()],
        });
    }
    let observation_matches = transaction
        .query_row(
            "SELECT project_id, graph_version, git_revision, dirty
             FROM graph_observations WHERE observation_id = ?1",
            params![basis.observation_id],
            |row| Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)? as u64,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)? != 0,
            )),
        )
        .optional()
        .map_err(|error| format!("Unable to validate candidate observation: {error}"))?
        .is_some_and(|(project, version, revision, dirty)| {
            project == candidate.project_id
                && version == basis.graph_version
                && revision == candidate.git_revision
                && !dirty
        });
    if !observation_matches {
        return Ok(LastKnownGoodApplicability {
            status: "basis-unavailable".to_string(),
            limitations: vec!["candidate-observation-unavailable-or-mismatched".to_string()],
        });
    }
    let graph_contract = transaction
        .query_row(
            "SELECT graph_id, graph_schema_version, graph_derivation_id, node_fingerprint_contract
             FROM graph_versions WHERE graph_version = ?1",
            params![basis.graph_version as i64],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .optional()
        .map_err(|error| format!("Unable to validate candidate evidence contract: {error}"))?;
    let Some((graph_id, schema, derivation, fingerprint)) = graph_contract else {
        return Ok(LastKnownGoodApplicability {
            status: "basis-unavailable".to_string(),
            limitations: vec!["last-known-good-graph-contract-unavailable".to_string()],
        });
    };
    let Some(candidate_contract) = candidate.evidence_contract.as_ref() else {
        return Ok(LastKnownGoodApplicability {
            status: "basis-unavailable".to_string(),
            limitations: vec!["last-known-good-evidence-contract-unavailable".to_string()],
        });
    };
    let contract_matches = graph_id == basis.graph_id
        && schema == crate::model::GRAPH_SCHEMA
        && derivation == crate::graph::GRAPH_DERIVATION_ID
        && fingerprint == crate::temporal::NODE_FINGERPRINT_CONTRACT
        && candidate_contract.graph_schema_version == schema
        && candidate_contract.graph_derivation_id == derivation
        && candidate_contract.node_fingerprint_contract == fingerprint;
    if !contract_matches {
        return Ok(LastKnownGoodApplicability {
            status: "contract-incompatible".to_string(),
            limitations: vec!["candidate-evidence-contract-incompatible".to_string()],
        });
    }
    if crate::diagnostic::validate_first_parent_range(root, &candidate.git_revision).is_err() {
        return Ok(LastKnownGoodApplicability {
            status: "out-of-lineage".to_string(),
            limitations: vec!["git-revision-not-on-current-first-parent-lineage".to_string()],
        });
    }
    Ok(LastKnownGoodApplicability {
        status: "applicable".to_string(),
        limitations,
    })
}
