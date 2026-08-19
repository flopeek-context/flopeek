use super::*;
use crate::model::{
    DIAGNOSTIC_CONTEXT_SCHEMA, DiagnosticContext, LKG_CANDIDATE_SCHEMA, LKG_EVENT_SCHEMA,
    LastKnownGoodCandidate, LastKnownGoodEvent, LastKnownGoodIntegrity,
    LastKnownGoodProposalRequest, LastKnownGoodTransitionRequest, reduce_last_known_good,
};
use std::fs;
use std::path::Path;
use std::process::Command;

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .expect("git");
    assert!(output.status.success(), "git {args:?}");
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn setup() -> (std::path::PathBuf, DiagnosticContext, String) {
    let root = fixture_root();
    fs::write(
        root.join(crate::identity::MANIFEST_PATH),
        r#"{"schemaVersion":"flopeek-repository-identity/v1","repositoryId":"repo_123e4567-e89b-12d3-a456-426614174000"}"#,
    )
    .expect("manifest");
    fs::write(root.join(".gitignore"), "/.flopeek/\n").expect("gitignore");
    git(&root, &["init"]);
    git(
        &root,
        &["config", "user.email", "flopeek-test@example.invalid"],
    );
    git(&root, &["config", "user.name", "Flopeek Test"]);
    git(&root, &["add", "."]);
    git(&root, &["commit", "-m", "baseline"]);
    let revision = git(&root, &["rev-parse", "HEAD"]);
    let (snapshot, facts) = crate::graph::build(&root).expect("build");
    let scan = persist_scan(&root, snapshot, &facts).expect("scan");
    let context = create_diagnostic_context(
        &root,
        DiagnosticContext {
            schema_version: DIAGNOSTIC_CONTEXT_SCHEMA.to_string(),
            id: "lkg-protocol-context".to_string(),
            project_id: scan.project_id.clone(),
            context_definition_revision: 0,
            context_basis_fingerprint: String::new(),
            memory_revision: 0,
            intent: "diagnose".to_string(),
            symptom: "timeout".to_string(),
            expected_behavior: "completes".to_string(),
            focus_context_refs: vec![scan.context_refs[0].uri.clone()],
            focus_flow_refs: Vec::new(),
            current_graph_basis: crate::diagnostic::graph_basis(&scan.graph),
            last_known_good_basis: None,
            last_known_good_binding_id: None,
            last_known_good_candidate_id: None,
            constraints: Vec::new(),
            acceptance_criteria: Vec::new(),
            unresolved_questions: Vec::new(),
            actor: "test".to_string(),
            created_at: 0,
            status: "open".to_string(),
            supersedes: None,
        },
    )
    .expect("context");
    (root, context, revision)
}

fn proposal(
    context: &DiagnosticContext,
    revision: &str,
    key: &str,
) -> LastKnownGoodProposalRequest {
    LastKnownGoodProposalRequest {
        context_id: context.id.clone(),
        git_revision: revision.to_string(),
        actor: "agent".to_string(),
        reason: "explicit fixture proposal".to_string(),
        evidence: Vec::new(),
        expected_tip_event_id: None,
        idempotency_key: key.to_string(),
        max_paths: None,
        max_snapshot_bytes: None,
    }
}

fn transition(
    context: &DiagnosticContext,
    state: &crate::model::LastKnownGoodState,
    key: &str,
) -> LastKnownGoodTransitionRequest {
    LastKnownGoodTransitionRequest {
        context_id: context.id.clone(),
        actor: "human-reviewer".to_string(),
        reason: "explicit human transition".to_string(),
        evidence: Vec::new(),
        expected_tip_event_id: state.tip_event_id.clone(),
        idempotency_key: key.to_string(),
        candidate_id: None,
    }
}

#[test]
fn protocol_reduces_pending_reject_replacement_and_revoke() {
    let (root, context, revision) = setup();
    let first = propose_last_known_good(&root, proposal(&context, &revision, "p1")).expect("p1");
    assert_eq!(first.integrity.status, "complete");
    let pending = get_last_known_good_protocol(&root, &context.id).expect("pending");
    assert_eq!(pending.lifecycle_status, "pending");
    let confirmed = confirm_last_known_good_local(&root, transition(&context, &pending, "c1"))
        .expect("confirm");
    assert_eq!(confirmed.event_type, "CONFIRM");
    let active = get_last_known_good_protocol(&root, &context.id).expect("active");
    assert_eq!(active.lifecycle_status, "active");
    let mut second_request = proposal(&context, &revision, "p2");
    second_request.expected_tip_event_id = active.tip_event_id.clone();
    propose_last_known_good(&root, second_request).expect("p2");
    let pending = get_last_known_good_protocol(&root, &context.id).expect("pending 2");
    reject_last_known_good_local(&root, transition(&context, &pending, "r2")).expect("reject");
    let still_active =
        get_last_known_good_protocol(&root, &context.id).expect("active after reject");
    assert_eq!(still_active.lifecycle_status, "active");
    assert_eq!(still_active.active_candidate_id, active.active_candidate_id);
    let mut third_request = proposal(&context, &revision, "p3");
    third_request.expected_tip_event_id = still_active.tip_event_id.clone();
    propose_last_known_good(&root, third_request).expect("p3");
    let pending = get_last_known_good_protocol(&root, &context.id).expect("pending 3");
    confirm_last_known_good_local(&root, transition(&context, &pending, "c3"))
        .expect("replacement");
    let replaced = get_last_known_good_protocol(&root, &context.id).expect("replaced");
    assert_eq!(replaced.lifecycle_status, "active");
    let revoked =
        revoke_last_known_good_local(&root, transition(&context, &replaced, "v3")).expect("revoke");
    assert_eq!(revoked.event_type, "REVOKE");
    assert_eq!(
        get_last_known_good_protocol(&root, &context.id)
            .expect("inactive")
            .active_candidate_id,
        None
    );
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn protocol_allows_graph_reuse_when_observation_revision_is_authority() {
    let (root, context, revision) = setup();
    let graph_version = current_graph(&root)
        .expect("graph")
        .expect("current")
        .graph_version;
    let connection = open(&root).expect("db");
    connection
        .execute(
            "UPDATE graph_versions SET source_revision = 'legacy-materialization-revision' WHERE graph_version = ?1",
            rusqlite::params![graph_version as i64],
        )
        .expect("mutate structural materialization");
    drop(connection);
    let candidate = propose_last_known_good(&root, proposal(&context, &revision, "reuse"))
        .expect("observation-owned revision remains valid");
    assert_eq!(candidate.integrity.status, "complete");
    let state = get_last_known_good_protocol(&root, &context.id).expect("state");
    confirm_last_known_good_local(&root, transition(&context, &state, "reuse-confirm"))
        .expect("confirm graph reuse");
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn protocol_idempotency_partial_integrity_and_projection_corruption_fail_closed() {
    let (root, context, revision) = setup();
    let request = proposal(&context, &revision, "same-proposal");
    let first = propose_last_known_good(&root, request.clone()).expect("first proposal");
    let retry = propose_last_known_good(&root, request).expect("idempotent retry");
    assert_eq!(first.candidate_id, retry.candidate_id);
    let mut conflicting = proposal(&context, &revision, "same-proposal");
    conflicting.reason = "different payload".to_string();
    assert_eq!(
        propose_last_known_good(&root, conflicting).expect_err("idempotency conflict"),
        "idempotency-conflict"
    );

    let mut next = proposal(&context, &revision, "stale-tip");
    next.expected_tip_event_id = Some("not-the-tip".to_string());
    assert_eq!(
        propose_last_known_good(&root, next).expect_err("stale tip"),
        "stale-lifecycle-tip"
    );

    let mut state_payload = String::new();
    let connection = open(&root).expect("db");
    connection
        .query_row(
            "SELECT payload_json FROM last_known_good_state WHERE context_id = ?1",
            rusqlite::params![context.id],
            |row| row.get(0),
        )
        .map(|payload: String| state_payload = payload)
        .expect("state payload");
    drop(connection);
    let connection = open(&root).expect("db");
    connection
        .execute(
            "UPDATE last_known_good_state SET payload_json = ?1 WHERE context_id = ?2",
            rusqlite::params![state_payload.replace("pending", "active"), context.id],
        )
        .expect("corrupt state");
    drop(connection);
    let corrupt = get_last_known_good_protocol(&root, &context.id).expect("corrupt state response");
    assert_eq!(corrupt.lifecycle_status, "corrupt");
    assert!(
        corrupt
            .limitations
            .iter()
            .any(|value| value == "lkg-materialized-state-mismatch")
    );
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn proposal_idempotency_rejects_revision_changes_with_the_same_key() {
    let (root, context, revision) = setup();
    propose_last_known_good(&root, proposal(&context, &revision, "revision-key"))
        .expect("first proposal");
    fs::write(root.join("src/revision.ts"), "export const revision = 2;").expect("source");
    git(&root, &["add", "."]);
    git(&root, &["commit", "-m", "revision change"]);
    let changed_revision = git(&root, &["rev-parse", "HEAD"]);
    let conflicting = proposal(&context, &changed_revision, "revision-key");
    assert_eq!(
        propose_last_known_good(&root, conflicting).expect_err("revision conflict"),
        "idempotency-conflict"
    );
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn proposal_retry_uses_original_request_without_reresolving_head_or_context_memory() {
    let (root, context, original_revision) = setup();
    let request = proposal(&context, "HEAD", "stable-head-request");
    let first = propose_last_known_good(&root, request.clone()).expect("first proposal");
    assert_eq!(first.git_revision, original_revision);

    append_diagnostic_assertion(
        &root,
        crate::model::DiagnosticAssertion {
            schema_version: crate::model::DIAGNOSTIC_ASSERTION_SCHEMA.to_string(),
            id: "memory-after-proposal".to_string(),
            context_id: context.id.clone(),
            revision: 0,
            kind: "observation".to_string(),
            status: "proposed".to_string(),
            actor: "agent".to_string(),
            statement: "Engineering memory advanced without redefining the Context.".to_string(),
            evidence: Vec::new(),
            supersedes: None,
            created_at: 0,
        },
    )
    .expect("append memory");
    fs::write(root.join("src/revision.ts"), "export const revision = 2;").expect("source");
    git(&root, &["add", "."]);
    git(&root, &["commit", "-m", "advance head"]);

    let retried = propose_last_known_good(&root, request).expect("idempotent retry");
    assert_eq!(retried, first);
    let current_context = get_diagnostic_context(&root, &context.id).expect("context");
    assert_eq!(current_context.memory_revision, 1);
    assert_eq!(
        current_context.context_basis_fingerprint,
        context.context_basis_fingerprint
    );
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn transition_retry_returns_original_event_after_lifecycle_tip_advances() {
    let (root, context, revision) = setup();
    propose_last_known_good(&root, proposal(&context, &revision, "transition-p1"))
        .expect("proposal");
    let pending = get_last_known_good_protocol(&root, &context.id).expect("pending");
    let confirm_request = transition(&context, &pending, "transition-c1");
    let confirmed = confirm_last_known_good_local(&root, confirm_request.clone()).expect("confirm");
    let active = get_last_known_good_protocol(&root, &context.id).expect("active");
    let mut replacement = proposal(&context, &revision, "transition-p2");
    replacement.expected_tip_event_id = active.tip_event_id;
    propose_last_known_good(&root, replacement).expect("replacement proposal");

    let retried = confirm_last_known_good_local(&root, confirm_request).expect("retry confirm");
    assert_eq!(retried, confirmed);
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn missing_or_corrupt_command_receipt_fails_closed() {
    let (root, context, revision) = setup();
    let request = proposal(&context, &revision, "receipt-corruption");
    propose_last_known_good(&root, request.clone()).expect("proposal");
    let connection = open(&root).expect("database");
    connection
        .execute(
            "UPDATE last_known_good_command_receipts
             SET result_candidate_id = NULL
             WHERE context_id = ?1 AND idempotency_key = ?2",
            rusqlite::params![context.id, request.idempotency_key],
        )
        .expect("corrupt receipt");
    drop(connection);
    assert_eq!(
        propose_last_known_good(&root, request.clone()).expect_err("corrupt result"),
        "lkg-idempotency-result-unavailable"
    );

    let connection = open(&root).expect("database");
    connection
        .execute(
            "DELETE FROM last_known_good_command_receipts
             WHERE context_id = ?1 AND idempotency_key = ?2",
            rusqlite::params![context.id, request.idempotency_key],
        )
        .expect("remove receipt");
    drop(connection);
    assert_eq!(
        propose_last_known_good(&root, request).expect_err("legacy replay"),
        "legacy-lkg-idempotency-replay-unavailable"
    );
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn engineering_memory_does_not_invalidate_lkg_but_definition_changes_do() {
    let (root, context, revision) = setup();
    let candidate = propose_last_known_good(&root, proposal(&context, &revision, "memory-p1"))
        .expect("proposal");
    let pending = get_last_known_good_protocol(&root, &context.id).expect("pending");
    confirm_last_known_good_local(&root, transition(&context, &pending, "memory-c1"))
        .expect("confirm");

    for (index, kind) in [
        "observation",
        "hypothesis",
        "finding",
        "remediation",
        "verification",
    ]
    .into_iter()
    .enumerate()
    {
        append_diagnostic_assertion(
            &root,
            crate::model::DiagnosticAssertion {
                schema_version: crate::model::DIAGNOSTIC_ASSERTION_SCHEMA.to_string(),
                id: format!("memory-{index}"),
                context_id: context.id.clone(),
                revision: 0,
                kind: kind.to_string(),
                status: "proposed".to_string(),
                actor: "agent".to_string(),
                statement: format!("{kind} engineering memory"),
                evidence: Vec::new(),
                supersedes: None,
                created_at: 0,
            },
        )
        .expect("append assertion");
        let state = get_last_known_good_protocol(&root, &context.id).expect("state");
        assert_eq!(state.lifecycle_status, "active");
        assert_eq!(state.applicability_status, "applicable");
    }
    let mut redefined = get_diagnostic_context(&root, &context.id).expect("Context");
    assert_eq!(redefined.memory_revision, 5);
    assert_eq!(
        redefined.context_basis_fingerprint,
        candidate.context_basis_fingerprint
    );
    redefined.expected_behavior = "a newly defined outcome".to_string();
    redefined.context_definition_revision += 1;
    redefined.context_basis_fingerprint =
        crate::model::diagnostic_context_basis_fingerprint(&redefined);
    let payload = serde_json::to_string(&redefined).expect("Context payload");
    let connection = open(&root).expect("database");
    connection
        .execute(
            "UPDATE diagnostic_contexts
             SET context_definition_revision = ?1, context_basis_fingerprint = ?2,
                 payload_json = ?3 WHERE id = ?4",
            rusqlite::params![
                redefined.context_definition_revision,
                redefined.context_basis_fingerprint,
                payload,
                redefined.id
            ],
        )
        .expect("redefine Context fixture");
    drop(connection);
    let state = get_last_known_good_protocol(&root, &context.id).expect("redefined state");
    assert_eq!(state.lifecycle_status, "active");
    assert_eq!(state.applicability_status, "context-basis-mismatch");
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn history_uses_predecessor_order_and_reports_zero_and_bounded_omissions() {
    let (root, context, revision) = setup();
    propose_last_known_good(&root, proposal(&context, &revision, "history-p1")).expect("proposal");
    let pending = get_last_known_good_protocol(&root, &context.id).expect("pending");
    confirm_last_known_good_local(&root, transition(&context, &pending, "history-c1"))
        .expect("confirm");
    let active = get_last_known_good_protocol(&root, &context.id).expect("active");
    let mut replacement = proposal(&context, &revision, "history-p2");
    replacement.expected_tip_event_id = active.tip_event_id;
    propose_last_known_good(&root, replacement).expect("replacement");
    let pending = get_last_known_good_protocol(&root, &context.id).expect("pending");
    reject_last_known_good_local(&root, transition(&context, &pending, "history-r2"))
        .expect("reject");
    let connection = open(&root).expect("database");
    connection
        .execute(
            "UPDATE last_known_good_events SET created_at = 100 WHERE context_id = ?1",
            rusqlite::params![context.id],
        )
        .expect("same-second history");
    drop(connection);

    let full = list_last_known_good_protocol(&root, &context.id, 128).expect("history");
    assert_eq!(
        full.events
            .iter()
            .map(|event| event.event_type.as_str())
            .collect::<Vec<_>>(),
        ["PROPOSE", "CONFIRM", "PROPOSE", "REJECT"]
    );
    let bounded = list_last_known_good_protocol(&root, &context.id, 2).expect("bounded");
    assert_eq!(bounded.total_events, 4);
    assert!(bounded.truncated);
    assert_eq!(bounded.events.len(), 2);
    assert_eq!(bounded.events[0].event_type, "PROPOSE");
    assert_eq!(bounded.events[1].event_type, "REJECT");
    let zero = list_last_known_good_protocol(&root, &context.id, 0).expect("zero");
    assert!(zero.events.is_empty());
    assert_eq!(zero.total_events, 4);
    assert!(zero.truncated);
    assert!(!zero.omissions.is_empty());
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn diagnosis_preserves_active_but_out_of_lineage_candidate() {
    let (root, context, revision) = setup();
    propose_last_known_good(&root, proposal(&context, &revision, "diagnosis-p1"))
        .expect("proposal");
    let pending = get_last_known_good_protocol(&root, &context.id).expect("pending");
    confirm_last_known_good_local(&root, transition(&context, &pending, "diagnosis-c1"))
        .expect("confirm");
    git(&root, &["checkout", "--orphan", "divergent"]);
    fs::write(
        root.join("src/divergent.ts"),
        "export const divergent = true;",
    )
    .expect("source");
    git(&root, &["add", "."]);
    git(&root, &["commit", "-m", "divergent root"]);
    let (snapshot, facts) = crate::graph::build(&root).expect("divergent graph");
    persist_scan(&root, snapshot, &facts).expect("divergent scan");

    let diagnosis = crate::diagnostic::diagnose_history(
        &root,
        &context.id,
        crate::model::DiagnosticLimits::default(),
    )
    .expect("diagnosis");
    assert_eq!(diagnosis.last_known_good_status, "confirmed-inapplicable");
    assert_eq!(
        diagnosis
            .last_known_good_applicability
            .as_ref()
            .map(|value| value.status.as_str()),
        Some("out-of-lineage")
    );
    assert!(diagnosis.last_known_good_candidate.is_some());
    assert_eq!(
        diagnosis
            .last_known_good_state
            .as_ref()
            .map(|value| value.lifecycle_status.as_str()),
        Some("active")
    );
    assert!(diagnosis.range.is_none());
    assert!(diagnosis.candidates.is_empty());
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn review_packet_contains_bounded_candidate_to_current_delta() {
    let (root, context, revision) = setup();
    propose_last_known_good(&root, proposal(&context, &revision, "review-packet"))
        .expect("proposal");
    fs::write(root.join("src/review.ts"), "export const review = 2;").expect("source");
    git(&root, &["add", "."]);
    git(&root, &["commit", "-m", "review delta"]);
    let (snapshot, facts) = crate::graph::build(&root).expect("current build");
    persist_scan(&root, snapshot, &facts).expect("current scan");
    let packet = get_last_known_good_review_packet(&root, &context.id).expect("review packet");
    let delta = packet
        .structural_delta
        .as_ref()
        .expect("candidate-to-current delta");
    assert_eq!(delta["status"], "complete");
    assert!(!delta["sourceChanges"].as_array().unwrap().is_empty());
    assert!(
        serde_json::to_string(&packet)
            .expect("packet JSON")
            .contains("candidate-to-current comparison is observation evidence")
    );
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn replacement_review_packet_keeps_active_state_and_reviews_pending_candidate() {
    let (root, context, revision_a) = setup();
    propose_last_known_good(
        &root,
        proposal(&context, &revision_a, "replacement-a-propose"),
    )
    .expect("proposal A");
    let pending_a = get_last_known_good_protocol(&root, &context.id).expect("pending A");
    confirm_last_known_good_local(
        &root,
        transition(&context, &pending_a, "replacement-a-confirm"),
    )
    .expect("confirm A");
    let active_a = get_last_known_good_protocol(&root, &context.id).expect("active A");

    fs::write(
        root.join("src/main.ts"),
        "// observation-only change\nexport const main = 1;\n",
    )
    .expect("comment-only source");
    git(&root, &["add", "."]);
    git(&root, &["commit", "-m", "observation-only replacement"]);
    let revision_b = git(&root, &["rev-parse", "HEAD"]);
    let (snapshot_b, facts_b) = crate::graph::build(&root).expect("build B");
    persist_scan(&root, snapshot_b, &facts_b).expect("scan B");

    let mut request_b = proposal(&context, &revision_b, "replacement-b-propose");
    request_b.expected_tip_event_id = active_a.tip_event_id.clone();
    let candidate_b = propose_last_known_good(&root, request_b).expect("proposal B");
    let packet = get_last_known_good_review_packet(&root, &context.id).expect("review B");

    assert_eq!(packet.candidate.candidate_id, candidate_b.candidate_id);
    assert_eq!(
        packet.state.active_candidate_id,
        active_a.active_candidate_id
    );
    assert_eq!(
        packet.state.pending_candidate_id,
        Some(candidate_b.candidate_id.clone())
    );
    assert_eq!(packet.state.applicability_status, "applicable");
    assert_eq!(packet.applicability.status, "applicable");
    assert!(packet.confirmable);
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn detached_same_graph_reuse_uses_observation_manifest_not_source_rows() {
    let (root, context, revision_a) = setup();
    let graph_a = current_graph(&root).expect("graph A").expect("current A");
    fs::write(
        root.join("src/main.ts"),
        "// observation-only change\nexport const main = 1;\n",
    )
    .expect("comment-only source");
    git(&root, &["add", "."]);
    git(&root, &["commit", "-m", "observation-only current"]);
    let _revision_b = git(&root, &["rev-parse", "HEAD"]);
    let (snapshot_b, facts_b) = crate::graph::build(&root).expect("build B");
    let scan_b = persist_scan(&root, snapshot_b, &facts_b).expect("scan B");
    assert_eq!(graph_a.graph_id, scan_b.graph.graph_id);
    assert_eq!(graph_a.graph_version, scan_b.graph.graph_version);

    let candidate = propose_last_known_good(&root, proposal(&context, &revision_a, "detached-a"))
        .expect("historical A proposal");
    assert_eq!(candidate.integrity.status, "complete");
    let candidate_observation = candidate.observation_id.expect("historical observation");
    let current_observation = scan_b.graph.observation_id.clone();
    assert_ne!(candidate_observation, current_observation);
    let connection = open(&root).expect("open database");
    let historical_manifest: String = connection
        .query_row(
            "SELECT source_manifest_json FROM graph_observations WHERE observation_id = ?1",
            rusqlite::params![candidate_observation],
            |row| row.get(0),
        )
        .expect("historical manifest");
    let current_manifest: String = connection
        .query_row(
            "SELECT source_manifest_json FROM graph_observations WHERE observation_id = ?1",
            rusqlite::params![current_observation],
            |row| row.get(0),
        )
        .expect("current manifest");
    assert_ne!(historical_manifest, current_manifest);
    assert!(!historical_manifest.contains("observation-only change"));
    drop(connection);
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn detached_reuse_rejects_corrupt_structural_rows_and_contracts() {
    let (root, context, _revision_a) = setup();
    let graph_a = current_graph(&root).expect("graph A").expect("current A");
    fs::write(
        root.join("src/main.ts"),
        "// observation-only change\nexport const main = 1;\n",
    )
    .expect("comment-only source");
    git(&root, &["add", "."]);
    git(&root, &["commit", "-m", "structural validation current"]);
    let revision_b = git(&root, &["rev-parse", "HEAD"]);
    let graph_version = graph_a.graph_version as i64;
    let connection = open(&root).expect("open database");
    let original_node: String = connection
        .query_row(
            "SELECT evidence_fingerprint FROM graph_nodes WHERE graph_version = ?1 LIMIT 1",
            rusqlite::params![graph_version],
            |row| row.get(0),
        )
        .expect("node");
    let changed = connection
        .execute(
            "UPDATE graph_nodes SET evidence_fingerprint = 'corrupt-node'
             WHERE rowid = (SELECT rowid FROM graph_nodes WHERE graph_version = ?1 LIMIT 1)",
            rusqlite::params![graph_version],
        )
        .expect("corrupt node");
    assert_eq!(changed, 1);
    drop(connection);
    assert!(
        propose_last_known_good(&root, proposal(&context, &revision_b, "corrupt-node"))
            .expect_err("node corruption")
            .contains("historical-observation-graph-rows-mismatch")
    );

    let connection = open(&root).expect("open node repair");
    connection
        .execute(
            "UPDATE graph_nodes SET evidence_fingerprint = ?1
             WHERE rowid = (SELECT rowid FROM graph_nodes WHERE graph_version = ?2 LIMIT 1)",
            rusqlite::params![original_node, graph_version],
        )
        .expect("restore node");
    let original_edge: String = connection
        .query_row(
            "SELECT evidence FROM graph_edges WHERE graph_version = ?1 LIMIT 1",
            rusqlite::params![graph_version],
            |row| row.get(0),
        )
        .expect("edge");
    connection
        .execute(
            "UPDATE graph_edges SET evidence = 'corrupt-edge'
             WHERE rowid = (SELECT rowid FROM graph_edges WHERE graph_version = ?1 LIMIT 1)",
            rusqlite::params![graph_version],
        )
        .expect("corrupt edge");
    drop(connection);
    assert!(
        propose_last_known_good(&root, proposal(&context, &revision_b, "corrupt-edge"))
            .expect_err("edge corruption")
            .contains("historical-observation-graph-rows-mismatch")
    );

    let connection = open(&root).expect("open edge repair");
    connection
        .execute(
            "UPDATE graph_edges SET evidence = ?1
             WHERE rowid = (SELECT rowid FROM graph_edges WHERE graph_version = ?2 LIMIT 1)",
            rusqlite::params![original_edge, graph_version],
        )
        .expect("restore edge");
    let original_entry: String = connection
        .query_row(
            "SELECT entry_json FROM graph_flow_evidence WHERE graph_version = ?1",
            rusqlite::params![graph_version],
            |row| row.get(0),
        )
        .expect("flow evidence");
    connection
        .execute(
            "UPDATE graph_flow_evidence SET entry_json = '{}' WHERE graph_version = ?1",
            rusqlite::params![graph_version],
        )
        .expect("corrupt flow evidence");
    drop(connection);
    assert!(
        propose_last_known_good(&root, proposal(&context, &revision_b, "corrupt-flow"))
            .expect_err("flow corruption")
            .contains("historical-observation-graph-rows-mismatch")
    );

    let connection = open(&root).expect("open flow repair");
    connection
        .execute(
            "UPDATE graph_flow_evidence SET entry_json = ?1 WHERE graph_version = ?2",
            rusqlite::params![original_entry, graph_version],
        )
        .expect("restore flow evidence");
    connection
        .execute(
            "UPDATE graph_versions SET graph_derivation_id = 'corrupt-contract' WHERE graph_version = ?1",
            rusqlite::params![graph_version],
        )
        .expect("corrupt contract");
    drop(connection);
    assert!(
        propose_last_known_good(&root, proposal(&context, &revision_b, "corrupt-contract"))
            .expect_err("contract corruption")
            .contains("historical-observation-graph-rows-mismatch")
    );
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn protocol_diagnosis_does_not_synthesize_confirmer_from_proposer() {
    let (root, context, revision) = setup();
    propose_last_known_good(&root, proposal(&context, &revision, "attribution-propose"))
        .expect("proposal");
    let pending = get_last_known_good_protocol(&root, &context.id).expect("pending");
    confirm_last_known_good_local(&root, transition(&context, &pending, "attribution-confirm"))
        .expect("human confirmation");
    let events = list_last_known_good_protocol(&root, &context.id, 16).expect("history");
    assert_eq!(events.events[0].actor, "agent");
    assert_eq!(events.events[0].actor_kind, "agent-or-tool");
    assert_eq!(events.events[1].actor, "human-reviewer");
    assert_eq!(events.events[1].actor_kind, "human");
    let diagnosis = crate::diagnostic::diagnose_history(
        &root,
        &context.id,
        crate::model::DiagnosticLimits::default(),
    )
    .expect("diagnosis");
    assert!(diagnosis.last_known_good_binding.is_none());
    assert_eq!(
        diagnosis
            .last_known_good_candidate
            .expect("candidate")
            .proposed_by,
        "agent"
    );
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn protocol_partial_historical_candidate_is_stored_but_not_confirmable() {
    let (root, context, _revision) = setup();
    fs::write(root.join("src/new.ts"), "export const newValue = 2;").expect("source");
    git(&root, &["add", "."]);
    git(&root, &["commit", "-m", "unscanned revision"]);
    let revision = git(&root, &["rev-parse", "HEAD"]);
    let mut request = proposal(&context, &revision, "partial-bounds");
    request.max_paths = Some(0);
    request.max_snapshot_bytes = Some(1);
    let candidate = propose_last_known_good(&root, request).expect("partial proposal");
    assert_eq!(candidate.integrity.status, "partial");
    let state = get_last_known_good_protocol(&root, &context.id).expect("pending");
    let error =
        confirm_last_known_good_local(&root, transition(&context, &state, "partial-confirm"))
            .expect_err("partial candidate cannot confirm");
    assert!(error.contains("not-complete"));
    let _ = revision;
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn protocol_side_branch_candidate_is_complete_but_out_of_lineage() {
    let (root, context, _revision) = setup();
    let base_branch = git(&root, &["branch", "--show-current"]);
    git(&root, &["checkout", "-b", "lkg-side-branch"]);
    fs::write(root.join("src/side.ts"), "export const side = 1;").expect("side source");
    git(&root, &["add", "."]);
    git(&root, &["commit", "-m", "side branch candidate"]);
    let side_revision = git(&root, &["rev-parse", "HEAD"]);
    git(&root, &["checkout", &base_branch]);
    let candidate =
        propose_last_known_good(&root, proposal(&context, &side_revision, "side-branch"))
            .expect("side branch proposal");
    assert_eq!(candidate.integrity.status, "complete");
    let state = get_last_known_good_protocol(&root, &context.id).expect("pending");
    let error = confirm_last_known_good_local(&root, transition(&context, &state, "side-confirm"))
        .expect_err("side branch is not applicable");
    assert!(error.contains("out-of-lineage"));
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn pure_reducer_rejects_confirmation_without_pending_candidate() {
    let candidate = LastKnownGoodCandidate {
        schema_version: LKG_CANDIDATE_SCHEMA.to_string(),
        candidate_id: "candidate-direct".to_string(),
        repository_id: "repo_test".to_string(),
        project_id: "project_test".to_string(),
        context_id: "context-direct".to_string(),
        context_definition_revision: 0,
        context_basis_fingerprint: "sha256:test-context".to_string(),
        expected_behavior_fingerprint: "sha256:test".to_string(),
        git_revision: "0123456789012345678901234567890123456789".to_string(),
        observation_id: None,
        graph_basis: None,
        evidence_contract: None,
        proposed_by: "agent".to_string(),
        proposed_at: 1,
        evidence: Vec::new(),
        reason: "direct fixture".to_string(),
        integrity: LastKnownGoodIntegrity {
            status: "complete".to_string(),
            revision_available: true,
            observation_available: false,
            graph_basis_available: false,
            evidence_contract_compatible: false,
            limitations: vec!["unit-test candidate".to_string()],
        },
    };
    let event = LastKnownGoodEvent {
        schema_version: LKG_EVENT_SCHEMA.to_string(),
        event_id: "event-direct".to_string(),
        repository_id: candidate.repository_id.clone(),
        project_id: candidate.project_id.clone(),
        context_id: candidate.context_id.clone(),
        event_type: "CONFIRM".to_string(),
        candidate_id: candidate.candidate_id.clone(),
        replaces_candidate_id: None,
        predecessor_event_id: None,
        actor: "human".to_string(),
        actor_kind: "human".to_string(),
        actor_trust: "local-trusted-action-caller-attributed".to_string(),
        reason: "direct confirmation".to_string(),
        evidence: Vec::new(),
        created_at: 2,
        idempotency_key: "direct-confirm".to_string(),
    };
    let context_id = candidate.context_id.clone();
    let error = reduce_last_known_good(&context_id, &[candidate], &[event])
        .expect_err("direct confirmation must require a pending candidate");
    assert_eq!(error, "lkg-confirm-target-not-pending");
}
