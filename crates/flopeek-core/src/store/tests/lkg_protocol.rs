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
            revision: 0,
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
fn pure_reducer_allows_only_empty_state_direct_confirmation() {
    let candidate = LastKnownGoodCandidate {
        schema_version: LKG_CANDIDATE_SCHEMA.to_string(),
        candidate_id: "candidate-direct".to_string(),
        repository_id: "repo_test".to_string(),
        project_id: "project_test".to_string(),
        context_id: "context-direct".to_string(),
        context_revision: 0,
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
    let reduced = reduce_last_known_good(&context_id, &[candidate], &[event])
        .expect("direct confirmation from empty state");
    assert_eq!(reduced.state.lifecycle_status, "active");
}
