// Immutable command identity receipts for LKG retries.

fn command_request_fingerprint<T: serde::Serialize>(
    command_kind: &str,
    request: &T,
) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    let canonical = serde_json::to_vec(&(
        "flopeek-lkg-command-request/v1",
        command_kind,
        request,
    ))
    .map_err(|error| format!("Unable to encode LKG command request: {error}"))?;
    let mut hasher = Sha256::new();
    hasher.update(canonical);
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

fn receipt_row(
    transaction: &Transaction<'_>,
    context_id: &str,
    idempotency_key: &str,
) -> Result<Option<(String, String, Option<String>, String)>, String> {
    let receipt = transaction
        .query_row(
            "SELECT command_kind, request_fingerprint, result_candidate_id, result_event_id
             FROM last_known_good_command_receipts
             WHERE context_id = ?1 AND idempotency_key = ?2",
            params![context_id, idempotency_key],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()
        .map_err(|error| format!("Unable to inspect LKG command receipt: {error}"))?;
    if receipt.is_none() {
        let legacy_event = transaction
            .query_row(
                "SELECT 1 FROM last_known_good_events
                 WHERE context_id = ?1 AND idempotency_key = ?2",
                params![context_id, idempotency_key],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map_err(|error| format!("Unable to inspect legacy LKG idempotency: {error}"))?;
        if legacy_event.is_some() {
            return Err("legacy-lkg-idempotency-replay-unavailable".to_string());
        }
    }
    Ok(receipt)
}

fn proposal_receipt_result(
    transaction: &Transaction<'_>,
    request: &LastKnownGoodProposalRequest,
    fingerprint: &str,
) -> Result<Option<LastKnownGoodCandidate>, String> {
    let Some((kind, stored, candidate_id, event_id)) =
        receipt_row(transaction, &request.context_id, &request.idempotency_key)?
    else {
        return Ok(None);
    };
    if kind != "PROPOSE" || stored != fingerprint {
        return Err("idempotency-conflict".to_string());
    }
    let candidate_id = candidate_id.ok_or_else(|| "lkg-idempotency-result-unavailable".to_string())?;
    let event_kind = transaction
        .query_row(
            "SELECT event_type FROM last_known_good_events WHERE event_id = ?1",
            params![event_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| format!("Unable to validate LKG receipt event: {error}"))?;
    if event_kind.as_deref() != Some("PROPOSE") {
        return Err("lkg-idempotency-result-unavailable".to_string());
    }
    let payload = transaction
        .query_row(
            "SELECT payload_json FROM last_known_good_candidates WHERE candidate_id = ?1",
            params![candidate_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| format!("Unable to read idempotent LKG candidate: {error}"))?
        .ok_or_else(|| "lkg-idempotency-result-unavailable".to_string())?;
    decode(&payload, "LastKnownGoodCandidate").map(Some)
}

fn transition_receipt_result(
    transaction: &Transaction<'_>,
    request: &LastKnownGoodTransitionRequest,
    command_kind: &str,
    fingerprint: &str,
) -> Result<Option<LastKnownGoodEvent>, String> {
    let Some((kind, stored, _, event_id)) =
        receipt_row(transaction, &request.context_id, &request.idempotency_key)?
    else {
        return Ok(None);
    };
    if kind != command_kind || stored != fingerprint {
        return Err("idempotency-conflict".to_string());
    }
    let payload = transaction
        .query_row(
            "SELECT payload_json FROM last_known_good_events WHERE event_id = ?1",
            params![event_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| format!("Unable to read idempotent LKG event: {error}"))?
        .ok_or_else(|| "lkg-idempotency-result-unavailable".to_string())?;
    let event = decode::<LastKnownGoodEvent>(&payload, "LastKnownGoodEvent")?;
    if event.event_type != command_kind {
        return Err("lkg-idempotency-result-unavailable".to_string());
    }
    Ok(Some(event))
}

fn persist_command_receipt(
    transaction: &Transaction<'_>,
    context_id: &str,
    idempotency_key: &str,
    command_kind: &str,
    request_fingerprint: &str,
    candidate_id: Option<&str>,
    event_id: &str,
    created_at: u64,
) -> Result<(), String> {
    transaction
        .execute(
            "INSERT INTO last_known_good_command_receipts(
                 context_id, idempotency_key, command_kind, request_fingerprint,
                 result_candidate_id, result_event_id, created_at
             ) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![
                context_id,
                idempotency_key,
                command_kind,
                request_fingerprint,
                candidate_id,
                event_id,
                created_at as i64
            ],
        )
        .map_err(|error| format!("Unable to persist LKG command receipt: {error}"))?;
    Ok(())
}

