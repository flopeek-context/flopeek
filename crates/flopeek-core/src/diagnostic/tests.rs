//! Diagnostic behavior tests.

use super::*;
use crate::model::{DIAGNOSTIC_ASSERTION_SCHEMA, DIAGNOSTIC_CONTEXT_SCHEMA, EvidenceReference};
use crate::{graph, store};
use rusqlite::OptionalExtension;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn fixture_root() -> PathBuf {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("flopeek-diagnostic-{suffix}"));
    fs::create_dir_all(root.join("src")).expect("mkdir");
    git(&root, &["init"]);
    git(
        &root,
        &["config", "user.email", "flopeek-test@example.invalid"],
    );
    git(&root, &["config", "user.name", "Flopeek Test"]);
    root
}

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .expect("git");
    assert!(
        output.status.success(),
        "git {:?}: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn commit(root: &Path, message: &str) -> String {
    git(root, &["add", "."]);
    git(root, &["commit", "-m", message]);
    git(root, &["rev-parse", "HEAD"])
}

fn context_for(root: &Path) -> (DiagnosticContext, String) {
    let (snapshot, facts) = graph::build(root).expect("build");
    let lkg = git(root, &["rev-list", "--max-parents=0", "HEAD"]);
    let result = store::persist_scan(root, snapshot, &facts).expect("persist");
    let payment_ref = result
        .context_refs
        .iter()
        .find(|reference| {
            result.graph.nodes.iter().any(|node| {
                node.id == reference.node_id && node.path.as_deref() == Some("src/payment.ts")
            })
        })
        .or_else(|| {
            result.context_refs.iter().find(|reference| {
                result.graph.nodes.iter().any(|node| {
                    node.id == reference.node_id && node.path.as_deref() == Some("src/main.ts")
                })
            })
        })
        .expect("payment Context Ref")
        .uri
        .clone();
    let context = DiagnosticContext {
        schema_version: DIAGNOSTIC_CONTEXT_SCHEMA.to_string(),
        id: "checkout-timeout".to_string(),
        project_id: result.project_id,
        revision: 0,
        intent: "diagnose".to_string(),
        symptom: "checkout intermittently times out".to_string(),
        expected_behavior: "checkout completes once within the configured timeout".to_string(),
        focus_context_refs: vec![payment_ref.clone()],
        focus_flow_refs: Vec::new(),
        current_graph_basis: crate::diagnostic::graph_basis(&result.graph),
        last_known_good_basis: Some(GitBasis { revision: lkg }),
        constraints: vec!["Static evidence only".to_string()],
        acceptance_criteria: vec![
            "Retry and timeout changes remain candidates, never causes".to_string(),
        ],
        unresolved_questions: vec!["What runtime branch was executed?".to_string()],
        actor: "test-agent".to_string(),
        created_at: 0,
        status: "open".to_string(),
        supersedes: None,
    };
    (context, payment_ref)
}

fn copy_fixture_tree(source: &Path, destination: &Path) {
    for entry in fs::read_dir(source).expect("fixture directory") {
        let entry = entry.expect("fixture entry");
        let from = entry.path();
        let to = destination.join(entry.file_name());
        if from.is_dir() {
            fs::create_dir_all(&to).expect("fixture destination directory");
            copy_fixture_tree(&from, &to);
        } else {
            if let Some(parent) = to.parent() {
                fs::create_dir_all(parent).expect("fixture parent");
            }
            fs::copy(from, to).expect("fixture file");
        }
    }
}

#[test]
fn real_fixture_merge_is_reported_as_first_parent_candidate() {
    let root = fixture_root();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/typescript/history");
    copy_fixture_tree(&fixture.join("A"), &root);
    let a = commit(&root, "A: checkout payment last-known-good");
    let main_branch = git(&root, &["branch", "--show-current"]);
    git(&root, &["switch", "-c", "retry-topic"]);
    copy_fixture_tree(&fixture.join("B"), &root);
    let topic = commit(&root, "topic: introduce retry path");
    git(&root, &["switch", &main_branch]);
    let _merge_output = git(
        &root,
        &[
            "merge",
            "--no-ff",
            "retry-topic",
            "-m",
            "B: merge retry path",
        ],
    );
    let merge = git(&root, &["rev-parse", "HEAD"]);
    assert_ne!(merge, topic);
    fs::write(root.join("README.md"), "unrelated documentation\n").expect("C");
    let _c = commit(&root, "C: unrelated documentation");
    copy_fixture_tree(&fixture.join("D"), &root);
    let d = commit(&root, "D: change timeout branch");
    copy_fixture_tree(&fixture.join("E"), &root);
    fs::OpenOptions::new()
        .append(true)
        .open(root.join("src/checkout.ts"))
        .expect("E checkout")
        .write_all(b"\nexport const currentBadState = true;\n")
        .expect("E current state");
    let _e = commit(&root, "E: current bad state");

    let (context, _) = context_for(&root);
    let context = store::create_diagnostic_context(&root, context).expect("context");
    let diagnosis =
        diagnose_history(&root, &context.id, DiagnosticLimits::default()).expect("history");
    assert_eq!(
        diagnosis
            .last_known_good_basis
            .as_ref()
            .map(|basis| basis.revision.as_str()),
        Some(a.as_str())
    );
    assert!(
        diagnosis
            .candidates
            .iter()
            .any(|candidate| candidate.commit == merge)
    );
    assert!(
        !diagnosis
            .candidates
            .iter()
            .any(|candidate| candidate.commit == topic)
    );
    assert!(
        diagnosis
            .candidates
            .iter()
            .any(|candidate| candidate.commit == d)
    );
    assert!(diagnosis.candidates.iter().all(|candidate| {
        candidate
            .relevance_reasons
            .iter()
            .all(|reason| reason != "root-cause")
    }));
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn history_fixture_ranks_typescript_candidates_and_excludes_unrelated_changes() {
    let root = fixture_root();
    fs::write(
        root.join("src/checkout.ts"),
        "import { payment } from './payment'; export function checkout() { return payment(); }\n",
    )
    .expect("checkout A");
    fs::write(
        root.join("src/payment.ts"),
        "export function payment() { return 'ok'; }\n",
    )
    .expect("payment A");
    let _a = commit(&root, "A: checkout payment last-known-good");
    fs::write(
        root.join("src/payment.ts"),
        "export function retry() { return 'ok'; } export function payment() { return retry(); }\n",
    )
    .expect("payment B");
    let b = commit(&root, "B: introduce retry path");
    fs::write(root.join("README.md"), "unrelated documentation\n").expect("readme C");
    let _c = commit(&root, "C: unrelated documentation");
    fs::write(
        root.join("src/payment.ts"),
        "export function retry() { return 'ok'; } export function payment() { return retryWithTimeout(); } export function retryWithTimeout() { return 'ok'; }\n",
    )
    .expect("payment D");
    let d = commit(&root, "D: change timeout branch");
    fs::write(
        root.join("src/checkout.ts"),
        "import { payment } from './payment'; export function checkout() { return payment(); } export const current = true;\n",
    )
    .expect("checkout E");
    let _e = commit(&root, "E: current bad state");

    let (context, _) = context_for(&root);
    let context = store::create_diagnostic_context(&root, context).expect("context");
    let diagnosis =
        diagnose_history(&root, &context.id, DiagnosticLimits::default()).expect("history");
    assert!(!diagnosis.truncated);
    assert!(
        diagnosis
            .candidates
            .iter()
            .any(|candidate| candidate.commit == b)
    );
    assert!(
        diagnosis
            .candidates
            .iter()
            .any(|candidate| candidate.commit == d)
    );
    assert!(
        diagnosis
            .candidates
            .iter()
            .all(|candidate| !candidate.summary.contains("unrelated documentation"))
    );
    assert!(diagnosis.candidates.iter().all(|candidate| {
        !candidate
            .relevance_reasons
            .iter()
            .any(|reason| reason == "root-cause")
    }));
    assert!(
        diagnosis
            .limitations
            .iter()
            .any(|limitation| limitation.contains("not runtime causes"))
    );
    let repeat =
        diagnose_history(&root, &context.id, DiagnosticLimits::default()).expect("repeat history");
    assert_eq!(diagnosis.candidates, repeat.candidates);
    let bounded = diagnose_history(
        &root,
        &context.id,
        DiagnosticLimits {
            max_commits: 1,
            ..DiagnosticLimits::default()
        },
    )
    .expect("bounded history");
    assert!(bounded.truncated);
    assert_eq!(bounded.commits_inspected, 1);
    let connection = store::open(&root).expect("sqlite candidates");
    let candidate_count = connection
        .query_row(
            "SELECT COUNT(*) FROM historical_candidates WHERE context_id = ?1",
            rusqlite::params![context.id],
            |row| row.get::<_, i64>(0),
        )
        .expect("candidate count");
    assert!(candidate_count > 0);
    drop(connection);
    let history_cache = root.join(".flopeek/diagnostics/history");
    assert!(history_cache.is_dir());
    assert!(
        fs::read_dir(history_cache)
            .expect("history cache")
            .next()
            .is_some()
    );
    for entry in fs::read_dir(root.join(".flopeek/diagnostics/history")).expect("history entries") {
        let entry = entry.expect("history entry");
        let snapshot = fs::read_to_string(entry.path()).expect("snapshot");
        assert!(!snapshot.contains("export async function"));
    }
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn flow_focused_history_and_packet_keep_exact_evidence_and_candidate_language() {
    let root = fixture_root();
    fs::write(
        root.join("package.json"),
        r#"{"scripts":{"start":"tsx src/main.ts"}}"#,
    )
    .expect("package A");
    fs::write(
        root.join("src/main.ts"),
        "export function main() { return 'source-body-sentinel'; }\n",
    )
    .expect("main A");
    fs::create_dir_all(root.join("tests")).expect("tests directory");
    fs::write(
        root.join("tests/main.test.ts"),
        "import { main } from '../src/main'; main();\n",
    )
    .expect("test A");
    let a = commit(&root, "A: stable package entry");
    fs::write(
        root.join("package.json"),
        r#"{"scripts":{"start":"tsx src/other.ts"}}"#,
    )
    .expect("package B");
    fs::write(
        root.join("src/other.ts"),
        "export function other() { return 'changed-source-body-sentinel'; }\n",
    )
    .expect("other B");
    let b = commit(&root, "B: change static entry target");

    let (snapshot, facts) = graph::build(&root).expect("build current");
    let result = store::persist_scan(&root, snapshot, &facts).expect("persist current");
    let flow_ref = result.flow_refs.first().expect("flow ref").uri.clone();
    let node_ref = result.context_refs.first().expect("node ref").uri.clone();
    let context = DiagnosticContext {
        schema_version: DIAGNOSTIC_CONTEXT_SCHEMA.to_string(),
        id: "flow-focused-context".to_string(),
        project_id: result.project_id,
        revision: 0,
        intent: "diagnose".to_string(),
        symptom: "the static entry target changed".to_string(),
        expected_behavior: "the declared entry remains stable".to_string(),
        focus_context_refs: vec![node_ref],
        focus_flow_refs: vec![flow_ref.clone()],
        current_graph_basis: graph_basis(&result.graph),
        last_known_good_basis: Some(GitBasis {
            revision: a.clone(),
        }),
        constraints: vec!["Static evidence only".to_string()],
        acceptance_criteria: vec!["Candidates remain non-causal".to_string()],
        unresolved_questions: vec!["Was the entry invoked?".to_string()],
        actor: "test-agent".to_string(),
        created_at: 0,
        status: "open".to_string(),
        supersedes: None,
    };
    let context = store::create_diagnostic_context(&root, context).expect("context");
    let diagnosis =
        diagnose_history(&root, &context.id, DiagnosticLimits::default()).expect("flow diagnosis");
    let candidate = diagnosis
        .candidates
        .iter()
        .find(|candidate| candidate.commit == b)
        .expect("entry candidate");
    assert!(
        candidate
            .relevance_reasons
            .iter()
            .any(|reason| reason == "focused-flow-changed")
    );
    assert!(
        candidate
            .relevance_reasons
            .iter()
            .any(|reason| reason == "focused-entry-changed")
    );
    assert!(
        candidate
            .relevance_reasons
            .iter()
            .all(|reason| reason != "root-cause")
    );
    assert_eq!(diagnosis.last_known_good_basis.unwrap().revision, a);

    let packet = build_packet(&root, &context.id, DiagnosticLimits::default()).expect("packet");
    assert_eq!(packet.focus_flow_refs.len(), 1);
    assert_eq!(packet.focus_flow_refs[0].status, "current");
    assert_eq!(packet.focus_flows.len(), 1);
    assert!(
        packet
            .historical
            .candidates
            .iter()
            .any(|candidate| candidate.commit == b)
    );
    let packet_json = serde_json::to_string(&packet).expect("packet json");
    assert!(!packet_json.contains("source-body-sentinel"));
    assert!(!packet_json.contains("changed-source-body-sentinel"));
    assert!(!packet_json.contains(root.to_string_lossy().as_ref()));
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn assertion_lifecycle_is_versioned_and_evidence_classes_stay_separate() {
    let root = fixture_root();
    fs::write(root.join("src/main.ts"), "export const main = 1;\n").expect("source");
    commit(&root, "source");
    let (context, _) = context_for(&root);
    let context = store::create_diagnostic_context(&root, context).expect("context");
    let connection = store::open(&root).expect("sqlite");
    let diagnostic_table = connection
        .query_row(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'diagnostic_contexts'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional();
    assert!(diagnostic_table.expect("sqlite query").is_some());
    assert!(!root.join(".flopeek/diagnostics/diagnostics.json").exists());
    drop(connection);
    let first = store::append_diagnostic_assertion(
        &root,
        DiagnosticAssertion {
            schema_version: DIAGNOSTIC_ASSERTION_SCHEMA.to_string(),
            id: "observation-1".to_string(),
            context_id: context.id.clone(),
            revision: 0,
            kind: "observation".to_string(),
            status: "proposed".to_string(),
            actor: "human".to_string(),
            statement: "CI recorded a timeout".to_string(),
            evidence: vec![EvidenceReference {
                evidence_class: "observation".to_string(),
                kind: "ci-test".to_string(),
                reference: "run:123".to_string(),
            }],
            supersedes: None,
            created_at: 0,
        },
    )
    .expect("assertion");
    assert_eq!(first.revision, 2);
    let second = store::append_diagnostic_assertion(
        &root,
        DiagnosticAssertion {
            schema_version: DIAGNOSTIC_ASSERTION_SCHEMA.to_string(),
            id: "finding-1".to_string(),
            context_id: context.id.clone(),
            revision: 0,
            kind: "finding".to_string(),
            status: "confirmed".to_string(),
            actor: "reviewer".to_string(),
            statement: "The retry path is historically relevant".to_string(),
            evidence: vec![EvidenceReference {
                evidence_class: "static".to_string(),
                kind: "historical-candidate".to_string(),
                reference: "candidate:abc".to_string(),
            }],
            supersedes: None,
            created_at: 0,
        },
    )
    .expect("finding");
    assert_eq!(second.revision, 3);
    assert_eq!(
        store::get_diagnostic_context(&root, &context.id)
            .expect("context")
            .revision,
        3
    );
    assert_eq!(
        store::list_diagnostic_assertions(&root, &context.id)
            .expect("assertions")
            .len(),
        2
    );
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn missing_last_known_good_is_explicit_and_packet_is_bounded() {
    let root = fixture_root();
    fs::write(root.join("src/main.ts"), "export const main = 1;\n").expect("source");
    commit(&root, "source");
    let (mut context, _) = context_for(&root);
    context.last_known_good_basis = None;
    let context = store::create_diagnostic_context(&root, context).expect("context");
    let diagnosis =
        diagnose_history(&root, &context.id, DiagnosticLimits::default()).expect("history");
    assert!(diagnosis.last_known_good_basis.is_none());
    assert!(
        diagnosis
            .limitations
            .iter()
            .any(|limitation| limitation.contains("last-known-good basis is unavailable"))
    );
    let packet = build_packet(
        &root,
        &context.id,
        DiagnosticLimits {
            max_packet_bytes: 8 * 1024,
            ..DiagnosticLimits::default()
        },
    )
    .expect("packet");
    assert!(serde_json::to_vec(&packet).expect("packet json").len() <= 8 * 1024);
    fs::write(root.join("src/main.ts"), "export const main = 2;\n").expect("change");
    let (snapshot, facts) = graph::build(&root).expect("changed graph");
    store::persist_scan(&root, snapshot, &facts).expect("persist changed graph");
    let stale_packet =
        build_packet(&root, &context.id, DiagnosticLimits::default()).expect("stale packet");
    assert!(
        stale_packet
            .focus_context_refs
            .iter()
            .any(|reference| reference.status == "stale")
    );
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn corrupted_diagnostic_metadata_is_reported_without_silent_recovery() {
    let root = fixture_root();
    fs::write(root.join("src/main.ts"), "export const main = 1;\n").expect("source");
    commit(&root, "source");
    let (context, _) = context_for(&root);
    let context = store::create_diagnostic_context(&root, context).expect("context");
    let connection = store::open(&root).expect("sqlite");
    connection
        .execute(
            "UPDATE diagnostic_contexts SET payload_json = 'not-json' WHERE id = ?1",
            rusqlite::params![context.id],
        )
        .expect("corrupt metadata");
    drop(connection);
    let error = store::get_diagnostic_context(&root, &context.id).expect_err("corruption");
    assert!(error.contains("corrupted"));
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn invalid_schema_wrong_project_and_credential_evidence_are_rejected() {
    let root = fixture_root();
    fs::write(root.join("src/main.ts"), "export const main = 1;\n").expect("source");
    commit(&root, "source");
    let (mut context, _) = context_for(&root);
    context.schema_version = "legacy-diagnostic/v0".to_string();
    assert!(validate_context(&context).is_err());
    context.schema_version = DIAGNOSTIC_CONTEXT_SCHEMA.to_string();
    context.project_id = "project_wrong".to_string();
    assert!(store::create_diagnostic_context(&root, context).is_err());
    assert!(
        validate_evidence(&EvidenceReference {
            evidence_class: "observation".to_string(),
            kind: "log".to_string(),
            reference: "token=secret".to_string(),
        })
        .is_err()
    );
    fs::remove_dir_all(root).expect("cleanup");
}
