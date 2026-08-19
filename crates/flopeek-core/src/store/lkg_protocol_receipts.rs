// Immutable command identity receipts for LKG retries.

struct CommandReceipt {
    command_kind: String,
    request_fingerprint: String,
    result_candidate_id: Option<String>,
    result_event_id: String,
}

struct NewCommandReceipt<'a> {
    context_id: &'a str,
    idempotency_key: &'a str,
    command_kind: &'a str,
    request_fingerprint: &'a str,
    candidate_id: Option<&'a str>,
    event_id: &'a str,
    created_at: u64,
}

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
) -> Result<Option<CommandReceipt>, String> {
    let receipt = transaction
        .query_row(
            "SELECT command_kind, request_fingerprint, result_candidate_id, result_event_id
             FROM last_known_good_command_receipts
             WHERE context_id = ?1 AND idempotency_key = ?2",
            params![context_id, idempotency_key],
            |row| {
                Ok(CommandReceipt {
                    command_kind: row.get(0)?,
                    request_fingerprint: row.get(1)?,
                    result_candidate_id: row.get(2)?,
                    result_event_id: row.get(3)?,
                })
            },
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
    let Some(receipt) =
        receipt_row(transaction, &request.context_id, &request.idempotency_key)?
    else {
        return Ok(None);
    };
    if receipt.command_kind != "PROPOSE" || receipt.request_fingerprint != fingerprint {
        return Err("idempotency-conflict".to_string());
    }
    let candidate_id = receipt
        .result_candidate_id
        .ok_or_else(|| "lkg-idempotency-result-unavailable".to_string())?;
    let event_kind = transaction
        .query_row(
            "SELECT event_type FROM last_known_good_events WHERE event_id = ?1",
            params![receipt.result_event_id],
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
    let Some(receipt) =
        receipt_row(transaction, &request.context_id, &request.idempotency_key)?
    else {
        return Ok(None);
    };
    if receipt.command_kind != command_kind || receipt.request_fingerprint != fingerprint {
        return Err("idempotency-conflict".to_string());
    }
    let payload = transaction
        .query_row(
            "SELECT payload_json FROM last_known_good_events WHERE event_id = ?1",
            params![receipt.result_event_id],
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
    receipt: NewCommandReceipt<'_>,
) -> Result<(), String> {
    transaction
        .execute(
            "INSERT INTO last_known_good_command_receipts(
                 context_id, idempotency_key, command_kind, request_fingerprint,
                 result_candidate_id, result_event_id, created_at
             ) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![
                receipt.context_id,
                receipt.idempotency_key,
                receipt.command_kind,
                receipt.request_fingerprint,
                receipt.candidate_id,
                receipt.event_id,
                receipt.created_at as i64
            ],
        )
        .map_err(|error| format!("Unable to persist LKG command receipt: {error}"))?;
    Ok(())
}
