//! Versioned diagnostic memory and bounded historical candidates.
//!
//! This module deliberately keeps static graph facts, human/agent assertions and
//! historical candidates in separate records.  Git path history is evidence of a
//! change, never proof that the change caused a runtime symptom.

use crate::model::{
    ContextRef, DIAGNOSTIC_ASSERTION_SCHEMA, DIAGNOSTIC_CONTEXT_SCHEMA, DIAGNOSTIC_PACKET_SCHEMA,
    DiagnosticAssertion, DiagnosticContext, DiagnosticLimits, DiagnosticPacket, EvidenceReference,
    GitBasis, GraphBasis, GraphNode, HISTORICAL_DIAGNOSIS_SCHEMA, HISTORICAL_SNAPSHOT_SCHEMA,
    HistoricalCandidate, HistoricalDiagnosis, HistoricalSnapshot,
};
use crate::store;
use crate::typescript::PARSER_IDENTITY;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::path::Path;
use std::process::Command;

const MAX_ID_BYTES: usize = 128;
const MAX_TEXT_BYTES: usize = 8 * 1024;
const MAX_LIST_ITEMS: usize = 256;
const HISTORY_DERIVATION_ID: &str = "typescript-historical-delta-v3";

const ALLOWED_INTENTS: &[&str] = &["diagnose", "audit", "verify-fix"];
const ALLOWED_CONTEXT_STATUSES: &[&str] = &["open", "reconciled", "resolved", "superseded"];
const ALLOWED_ASSERTION_KINDS: &[&str] = &[
    "observation",
    "hypothesis",
    "finding",
    "remediation",
    "verification",
];
const ALLOWED_ASSERTION_STATUSES: &[&str] = &[
    "proposed",
    "confirmed",
    "rejected",
    "superseded",
    "implemented",
    "verified",
];
const ALLOWED_EVIDENCE_CLASSES: &[&str] = &[
    "static",
    "observation",
    "hypothesis",
    "finding",
    "remediation",
    "verification",
];

pub fn validate_context(context: &DiagnosticContext) -> Result<(), String> {
    if context.schema_version != DIAGNOSTIC_CONTEXT_SCHEMA {
        return Err(format!(
            "Diagnostic Context schema must be {DIAGNOSTIC_CONTEXT_SCHEMA}."
        ));
    }
    validate_id("Diagnostic Context id", &context.id)?;
    validate_id("Diagnostic Context project id", &context.project_id)?;
    validate_choice("intent", &context.intent, ALLOWED_INTENTS)?;
    validate_choice("status", &context.status, ALLOWED_CONTEXT_STATUSES)?;
    validate_text("symptom", &context.symptom)?;
    validate_text("expectedBehavior", &context.expected_behavior)?;
    validate_text("actor", &context.actor)?;
    validate_list(
        "focusContextRefs",
        &context.focus_context_refs,
        MAX_LIST_ITEMS,
    )?;
    for reference in &context.focus_context_refs {
        if !reference.starts_with("fp://local/") || reference.len() > 512 {
            return Err(
                "focusContextRefs must contain bounded fp://local Context Refs.".to_string(),
            );
        }
    }
    validate_basis(&context.current_graph_basis)?;
    if let Some(last_known_good) = &context.last_known_good_basis {
        validate_revision(&last_known_good.revision)?;
    }
    validate_string_list("constraints", &context.constraints)?;
    validate_string_list("acceptanceCriteria", &context.acceptance_criteria)?;
    validate_string_list("unresolvedQuestions", &context.unresolved_questions)?;
    if let Some(supersedes) = &context.supersedes {
        validate_id("supersedes", supersedes)?;
        if supersedes == &context.id {
            return Err("A Diagnostic Context cannot supersede itself.".to_string());
        }
    }
    Ok(())
}

pub fn validate_assertion(assertion: &DiagnosticAssertion) -> Result<(), String> {
    if assertion.schema_version != DIAGNOSTIC_ASSERTION_SCHEMA {
        return Err(format!(
            "Diagnostic Assertion schema must be {DIAGNOSTIC_ASSERTION_SCHEMA}."
        ));
    }
    validate_id("Diagnostic Assertion id", &assertion.id)?;
    validate_id("contextId", &assertion.context_id)?;
    validate_choice("kind", &assertion.kind, ALLOWED_ASSERTION_KINDS)?;
    validate_choice("status", &assertion.status, ALLOWED_ASSERTION_STATUSES)?;
    validate_text("actor", &assertion.actor)?;
    validate_text("statement", &assertion.statement)?;
    validate_list("evidence", &assertion.evidence, MAX_LIST_ITEMS)?;
    for evidence in &assertion.evidence {
        validate_evidence(evidence)?;
    }
    if assertion.status == "superseded" && assertion.supersedes.is_none() {
        return Err("A superseded assertion must declare supersedes.".to_string());
    }
    if let Some(supersedes) = &assertion.supersedes {
        validate_id("supersedes", supersedes)?;
        if supersedes == &assertion.id {
            return Err("A Diagnostic Assertion cannot supersede itself.".to_string());
        }
    }
    Ok(())
}

pub fn validate_evidence(evidence: &EvidenceReference) -> Result<(), String> {
    validate_choice(
        "evidenceClass",
        &evidence.evidence_class,
        ALLOWED_EVIDENCE_CLASSES,
    )?;
    validate_text("evidence kind", &evidence.kind)?;
    if evidence.reference.is_empty()
        || evidence.reference.len() > 1024
        || evidence.reference.contains(['\r', '\n', '\0'])
    {
        return Err("evidence reference must be bounded and single-line.".to_string());
    }
    let lower = evidence.reference.to_ascii_lowercase();
    if [
        "password=",
        "token=",
        "secret=",
        "private_key",
        "authorization:",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
    {
        return Err("evidence reference cannot contain credential material.".to_string());
    }
    Ok(())
}

pub fn diagnose_history(
    root: &Path,
    context_id: &str,
    limits: DiagnosticLimits,
) -> Result<HistoricalDiagnosis, String> {
    let context = store::get_diagnostic_context(root, context_id)?;
    let graph = store::current_graph(root)?.ok_or_else(|| {
        "A current graph is required before historical diagnosis can run.".to_string()
    })?;
    let current_basis = GraphBasis {
        project_id: graph.project_id.clone(),
        graph_id: graph.graph_id.clone(),
        graph_version: graph.graph_version,
        source_revision: graph.source_revision.clone(),
        observation_id: graph.observation_id.clone(),
    };
    let mut limitations = vec![
        "Historical candidates are deterministic path/topology relevance signals, not runtime causes or root-cause findings.".to_string(),
        "Runtime execution, dynamic dispatch, reflection, generated code and business intent remain unavailable.".to_string(),
    ];
    let Some(last_known_good) = context.last_known_good_basis.clone() else {
        limitations.push(
            "last-known-good basis is unavailable; no historical range was inspected.".to_string(),
        );
        return Ok(HistoricalDiagnosis {
            schema_version: HISTORICAL_DIAGNOSIS_SCHEMA.to_string(),
            context_id: context.id,
            current_graph_basis: current_basis,
            last_known_good_basis: None,
            range: None,
            commits_inspected: 0,
            candidates: Vec::new(),
            truncated: false,
            omissions: Vec::new(),
            limitations,
        });
    };

    let last_revision = resolve_revision(root, &last_known_good.revision)?;
    let current_revision = current_head(root)?;
    let range = format!("{last_revision}..{current_revision}");
    if graph.source_revision != current_revision || git_is_dirty(root) {
        limitations.push(
            "historical diagnosis is unavailable because the persisted graph does not match a clean current Git source state.".to_string(),
        );
        if graph.source_revision != current_revision {
            limitations.push(format!(
                "source revision mismatch: graph={} current={current_revision}",
                graph.source_revision
            ));
        }
        if git_is_dirty(root) {
            limitations.push(
                "Git working tree is dirty; historical candidates were not computed.".to_string(),
            );
        }
        let mut omissions = vec![
            "historical candidates unavailable for dirty or mismatched source state".to_string(),
        ];
        if limits.max_commits == 0 {
            omissions.push("history commits omitted because max_commits is zero".to_string());
        }
        return Ok(HistoricalDiagnosis {
            schema_version: HISTORICAL_DIAGNOSIS_SCHEMA.to_string(),
            context_id: context.id,
            current_graph_basis: current_basis,
            last_known_good_basis: Some(GitBasis {
                revision: last_revision,
            }),
            range: Some(range),
            commits_inspected: 0,
            candidates: Vec::new(),
            truncated: limits.max_commits == 0,
            omissions,
            limitations,
        });
    }
    if last_revision == current_revision {
        limitations.push(
            "last-known-good and current revisions are identical; the inspected range is empty."
                .to_string(),
        );
        return Ok(HistoricalDiagnosis {
            schema_version: HISTORICAL_DIAGNOSIS_SCHEMA.to_string(),
            context_id: context.id,
            current_graph_basis: current_basis,
            last_known_good_basis: Some(GitBasis {
                revision: last_revision,
            }),
            range: Some(range),
            commits_inspected: 0,
            candidates: Vec::new(),
            truncated: false,
            omissions: Vec::new(),
            limitations,
        });
    }

    if limits.max_commits == 0 {
        limitations
            .push("history limit max_commits is zero; no commits were inspected.".to_string());
        return Ok(HistoricalDiagnosis {
            schema_version: HISTORICAL_DIAGNOSIS_SCHEMA.to_string(),
            context_id: context.id,
            current_graph_basis: current_basis,
            last_known_good_basis: Some(GitBasis {
                revision: last_revision,
            }),
            range: Some(range),
            commits_inspected: 0,
            candidates: Vec::new(),
            truncated: true,
            omissions: vec!["history commits omitted because max_commits is zero".to_string()],
            limitations,
        });
    }

    let (focus_paths, cone_paths, mut focus_limitations) =
        focus_paths(root, &context, &graph, &limits)?;
    let focus_limit_truncated = context.focus_context_refs.len() > limits.max_context_refs;
    limitations.append(&mut focus_limitations);
    let log_limit = limits.max_commits.saturating_add(1);
    let commits = git_log(root, &last_revision, &current_revision, log_limit)?;
    let truncated_commits = commits.len() > limits.max_commits;
    let inspected = commits
        .into_iter()
        .take(limits.max_commits)
        .collect::<Vec<_>>();
    let mut candidates = Vec::new();
    let mut omissions = Vec::new();
    let mut snapshot_cache = BTreeMap::new();
    let path_bound_zero = limits.max_paths == 0;
    let snapshot_bound_zero = limits.max_snapshot_bytes == 0;
    if path_bound_zero {
        omissions.push("historical candidate paths capped at zero".to_string());
    }
    if snapshot_bound_zero {
        omissions.push("historical snapshot bytes capped at zero".to_string());
    }

    for commit in &inspected {
        let mut changed_paths = git_changed_paths(
            root,
            &commit.sha,
            commit.parents.first().map(String::as_str),
        )?;
        changed_paths.sort();
        changed_paths.dedup();
        let original_path_count = changed_paths.len();
        changed_paths.retain(|path| is_typescript_path(path));
        if changed_paths.is_empty() {
            continue;
        }
        if path_bound_zero {
            continue;
        }
        let mut reasons = Vec::new();
        let mut score = 10_u32;
        if changed_paths.iter().any(|path| focus_paths.contains(path)) {
            reasons.push("changed-path-in-focus-context".to_string());
            score += 100;
        }
        if changed_paths.iter().any(|path| cone_paths.contains(path)) {
            reasons.push("changed-path-in-dependency-cone".to_string());
            score += 60;
        }
        if changed_paths.iter().any(|path| is_test_path(path)) {
            reasons.push("related-test-structure-changed".to_string());
            score += 25;
        }
        if limits.max_snapshot_bytes > 0 && !commit.parents.is_empty() {
            match historical_delta_reasons(
                root,
                commit,
                &focus_paths,
                &cone_paths,
                &limits,
                &mut snapshot_cache,
            ) {
                Ok((delta_reasons, delta_score, snapshot_notes)) => {
                    for reason in delta_reasons {
                        if !reasons.contains(&reason) {
                            reasons.push(reason);
                        }
                    }
                    score += delta_score;
                    for note in snapshot_notes {
                        limitations.push(format!("commit {}: {note}", commit.sha));
                    }
                }
                Err(error) => limitations.push(format!(
                    "historical graph snapshot unavailable for {}: {error}",
                    commit.sha
                )),
            }
        }
        if reasons.is_empty() {
            continue;
        }
        reasons.push("introduced-after-last-known-good".to_string());
        let changed_paths_truncated = changed_paths.len() > limits.max_paths;
        if changed_paths_truncated {
            changed_paths.truncate(limits.max_paths);
            omissions.push(format!(
                "commit {} paths capped at {}",
                commit.sha, limits.max_paths
            ));
        }
        if original_path_count > changed_paths.len() && !changed_paths_truncated {
            omissions.push(format!(
                "commit {} contained non-TypeScript paths omitted from candidate evidence",
                commit.sha
            ));
        }
        let current_files = graph
            .files
            .iter()
            .map(|file| (file.path.as_str(), file.hash.as_str()))
            .collect::<BTreeMap<_, _>>();
        let mut retained_path = false;
        let mut changed_path = false;
        let mut removed_path = false;
        for path in &changed_paths {
            let Some(current_hash) = current_files.get(path.as_str()) else {
                removed_path = true;
                continue;
            };
            retained_path = true;
            match git_show_bytes(root, &commit.sha, path) {
                Ok(bytes) if blake3::hash(&bytes).to_hex().to_string() == *current_hash => {}
                Ok(_) => changed_path = true,
                Err(_) => changed_path = true,
            }
        }
        let retention_status = if !retained_path {
            "removed"
        } else if changed_path || removed_path {
            "changed"
        } else {
            "retained"
        };
        let id_input = format!(
            "flopeek-historical-candidate-v1\0{}\0{}\0{}",
            context.id, current_basis.graph_id, commit.sha
        );
        candidates.push(HistoricalCandidate {
            schema_version: HISTORICAL_DIAGNOSIS_SCHEMA.to_string(),
            id: format!("candidate_{}", blake3::hash(id_input.as_bytes()).to_hex()),
            project_id: graph.project_id.clone(),
            context_id: context.id.clone(),
            current_graph_basis: current_basis.clone(),
            last_known_good_revision: last_revision.clone(),
            commit: commit.sha.clone(),
            parents: commit.parents.clone(),
            summary: commit.summary.clone(),
            changed_paths,
            changed_paths_truncated,
            relevance_reasons: reasons,
            score,
            retention_status: retention_status.to_string(),
        });
    }

    candidates.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.commit.cmp(&right.commit))
    });
    let mut truncated =
        truncated_commits || path_bound_zero || snapshot_bound_zero || focus_limit_truncated;
    if truncated_commits {
        omissions.push(format!("history commits capped at {}", limits.max_commits));
    }
    if candidates.len() > limits.max_candidates {
        candidates.truncate(limits.max_candidates);
        truncated = true;
        omissions.push(format!(
            "historical candidates capped at {}",
            limits.max_candidates
        ));
    }
    let omission_limit = limits.max_paths.max(1);
    if omissions.len() > omission_limit {
        omissions.truncate(omission_limit);
        truncated = true;
    }
    let diagnosis = HistoricalDiagnosis {
        schema_version: HISTORICAL_DIAGNOSIS_SCHEMA.to_string(),
        context_id: context.id,
        current_graph_basis: current_basis,
        last_known_good_basis: Some(GitBasis {
            revision: last_revision,
        }),
        range: Some(range),
        commits_inspected: inspected.len(),
        candidates,
        truncated,
        omissions,
        limitations,
    };
    store::persist_historical_candidates(root, &diagnosis)?;
    Ok(diagnosis)
}

pub fn build_packet(
    root: &Path,
    context_id: &str,
    limits: DiagnosticLimits,
) -> Result<DiagnosticPacket, String> {
    let context = store::get_diagnostic_context(root, context_id)?;
    let graph = store::current_graph(root)?.ok_or_else(|| {
        "A current graph is required before a diagnostic packet can be built.".to_string()
    })?;
    let current_basis = GraphBasis {
        project_id: graph.project_id.clone(),
        graph_id: graph.graph_id.clone(),
        graph_version: graph.graph_version,
        source_revision: graph.source_revision.clone(),
        observation_id: graph.observation_id.clone(),
    };
    let mut focus_context_refs = Vec::new();
    let mut focus_nodes = Vec::new();
    let mut omissions = Vec::new();
    for (index, uri) in context.focus_context_refs.iter().enumerate() {
        if index >= limits.max_context_refs {
            omissions.push(format!(
                "focus Context Refs capped at {}",
                limits.max_context_refs
            ));
            break;
        }
        let resolved = store::resolve_context(root, uri)?;
        if let Some(node) = graph.nodes.iter().find(|node| node.id == resolved.node_id) {
            if !focus_nodes
                .iter()
                .any(|existing: &GraphNode| existing.id == node.id)
            {
                focus_nodes.push(node.clone());
            }
        } else {
            omissions.push(format!("focus node unavailable for {uri}"));
        }
        focus_context_refs.push(resolved);
    }
    let mut assertions = store::list_diagnostic_assertions(root, context_id)?;
    let assertion_total = assertions.len();
    if assertion_total > limits.max_assertions {
        assertions.truncate(limits.max_assertions);
        omissions.push(format!("assertions capped at {}", limits.max_assertions));
    }
    let historical = diagnose_history(root, context_id, limits.clone())?;
    if historical.truncated {
        omissions
            .push("historical diagnosis was truncated by one or more declared bounds".to_string());
    }
    omissions.extend(
        historical
            .omissions
            .iter()
            .take(8)
            .map(|omission| format!("historical: {omission}")),
    );
    let mut limitations = historical.limitations.clone();
    limitations.push("Assertions retain their declared evidence class and attribution; they are not parser facts.".to_string());
    let packet_truncated = historical.truncated
        || focus_context_refs.len() < context.focus_context_refs.len()
        || assertions.len() < assertion_total;
    let mut packet = DiagnosticPacket {
        schema_version: DIAGNOSTIC_PACKET_SCHEMA.to_string(),
        current_graph_basis: current_basis.clone(),
        last_known_good_basis: context.last_known_good_basis.clone(),
        focus_context_refs,
        focus_nodes,
        assertions,
        historical,
        context,
        limitations,
        omissions,
        truncated: packet_truncated,
    };
    trim_packet(&mut packet, limits.max_packet_bytes)?;
    Ok(packet)
}

fn trim_packet(packet: &mut DiagnosticPacket, max_bytes: usize) -> Result<(), String> {
    if max_bytes == 0 {
        return Err("max_packet_bytes must be greater than zero.".to_string());
    }
    let serialized =
        |packet: &DiagnosticPacket| serde_json::to_vec(packet).map_err(|error| error.to_string());
    if serialized(packet)?.len() <= max_bytes {
        return Ok(());
    }
    packet.truncated = true;
    packet
        .omissions
        .push("diagnostic packet exceeded max_packet_bytes".to_string());
    while serialized(packet)?.len() > max_bytes {
        if packet.historical.candidates.pop().is_some() {
            packet
                .omissions
                .push("historical candidates omitted by packet bound".to_string());
        } else if packet.assertions.pop().is_some() {
            packet
                .omissions
                .push("assertions omitted by packet bound".to_string());
        } else if packet.focus_nodes.pop().is_some() {
            packet
                .omissions
                .push("focus node cards omitted by packet bound".to_string());
        } else if packet.focus_context_refs.pop().is_some() {
            packet
                .omissions
                .push("focus Context Refs omitted by packet bound".to_string());
        } else {
            return Err(
                "diagnostic packet envelope exceeds max_packet_bytes even after bounded omissions."
                    .to_string(),
            );
        }
    }
    Ok(())
}

#[derive(Debug, Clone)]
struct CommitRecord {
    sha: String,
    parents: Vec<String>,
    summary: String,
}

type FocusPathSets = (BTreeSet<String>, BTreeSet<String>, Vec<String>);

fn historical_delta_reasons(
    root: &Path,
    commit: &CommitRecord,
    focus_paths: &BTreeSet<String>,
    cone_paths: &BTreeSet<String>,
    limits: &DiagnosticLimits,
    cache: &mut BTreeMap<String, HistoricalSnapshot>,
) -> Result<(Vec<String>, u32, Vec<String>), String> {
    let current = load_or_build_historical_snapshot(root, &commit.sha, limits, cache)?;
    let parent = load_or_build_historical_snapshot(root, &commit.parents[0], limits, cache)?;
    let mut reasons = Vec::new();
    let mut score = 0;
    let mut notes = Vec::new();
    let current_files = current
        .files
        .iter()
        .map(|file| (file.path.as_str(), file.hash.as_str()))
        .collect::<BTreeMap<_, _>>();
    let parent_files = parent
        .files
        .iter()
        .map(|file| (file.path.as_str(), file.hash.as_str()))
        .collect::<BTreeMap<_, _>>();
    let mut changed_files = BTreeSet::new();
    changed_files.extend(current_files.keys().copied());
    changed_files.extend(parent_files.keys().copied());
    for path in changed_files {
        if current_files.get(path) != parent_files.get(path) {
            if focus_paths.contains(path) {
                reasons.push("focused-node-changed".to_string());
                score += 35;
            } else if cone_paths.contains(path) {
                reasons.push("dependency-cone-node-changed".to_string());
                score += 20;
            }
        }
    }
    let current_edges = current
        .edges
        .iter()
        .map(|edge| (edge.from.as_str(), edge.to.as_str(), edge.kind.as_str()))
        .collect::<BTreeSet<_>>();
    let parent_edges = parent
        .edges
        .iter()
        .map(|edge| (edge.from.as_str(), edge.to.as_str(), edge.kind.as_str()))
        .collect::<BTreeSet<_>>();
    let current_node_paths = current
        .nodes
        .iter()
        .filter_map(|node| {
            node.path
                .as_ref()
                .map(|path| (node.id.as_str(), path.as_str()))
        })
        .collect::<BTreeMap<_, _>>();
    let parent_node_paths = parent
        .nodes
        .iter()
        .filter_map(|node| {
            node.path
                .as_ref()
                .map(|path| (node.id.as_str(), path.as_str()))
        })
        .collect::<BTreeMap<_, _>>();
    let mut changed_edges = current_edges.clone();
    changed_edges.extend(parent_edges.iter().copied());
    for (from, to, _) in changed_edges {
        if current_edges.contains(&(from, to, "calls"))
            != parent_edges.contains(&(from, to, "calls"))
            || current_edges.contains(&(from, to, "imports"))
                != parent_edges.contains(&(from, to, "imports"))
        {
            let from_path = current_node_paths
                .get(from)
                .or_else(|| parent_node_paths.get(from));
            let to_path = current_node_paths
                .get(to)
                .or_else(|| parent_node_paths.get(to));
            if from_path.is_some_and(|path| focus_paths.contains(*path))
                || to_path.is_some_and(|path| focus_paths.contains(*path))
            {
                if !reasons
                    .iter()
                    .any(|reason| reason == "focused-edge-changed")
                {
                    reasons.push("focused-edge-changed".to_string());
                    score += 45;
                }
            } else if (from_path.is_some_and(|path| cone_paths.contains(*path))
                || to_path.is_some_and(|path| cone_paths.contains(*path)))
                && !reasons
                    .iter()
                    .any(|reason| reason == "dependency-cone-edge-changed")
            {
                reasons.push("dependency-cone-edge-changed".to_string());
                score += 25;
            }
        }
    }
    if current.truncated || parent.truncated {
        notes.push("historical graph snapshot was truncated by declared bounds".to_string());
    }
    Ok((reasons, score, notes))
}

fn load_or_build_historical_snapshot(
    root: &Path,
    revision: &str,
    limits: &DiagnosticLimits,
    cache: &mut BTreeMap<String, HistoricalSnapshot>,
) -> Result<HistoricalSnapshot, String> {
    let cache_key = format!(
        "{revision}\0{HISTORY_DERIVATION_ID}\0{PARSER_IDENTITY}\0{}\0{}",
        limits.max_paths, limits.max_snapshot_bytes
    );
    if let Some(snapshot) = cache.get(&cache_key) {
        return Ok(snapshot.clone());
    }
    if let Some(snapshot) = crate::history_store::load_with_key(
        root,
        revision,
        HISTORY_DERIVATION_ID,
        PARSER_IDENTITY,
        limits.max_paths,
        limits.max_snapshot_bytes,
    )? {
        cache.insert(cache_key, snapshot.clone());
        return Ok(snapshot);
    }
    let snapshot = build_historical_snapshot(root, revision, limits)?;
    crate::history_store::save_with_key(
        root,
        &snapshot,
        HISTORY_DERIVATION_ID,
        PARSER_IDENTITY,
        limits.max_paths,
        limits.max_snapshot_bytes,
    )?;
    cache.insert(cache_key, snapshot.clone());
    Ok(snapshot)
}

fn build_historical_snapshot(
    root: &Path,
    revision: &str,
    limits: &DiagnosticLimits,
) -> Result<HistoricalSnapshot, String> {
    let paths = git_tree_paths(root, revision)?;
    let temporary = std::env::temp_dir().join(format!(
        "flopeek-history-{}-{}",
        &revision[..revision.len().min(16)],
        std::process::id()
    ));
    if temporary.exists() {
        fs::remove_dir_all(&temporary)
            .map_err(|error| format!("Unable to replace historical snapshot workspace: {error}"))?;
    }
    fs::create_dir_all(&temporary)
        .map_err(|error| format!("Unable to create historical snapshot workspace: {error}"))?;
    let mut total_bytes = 0_usize;
    let mut truncated = false;
    let mut omissions = Vec::new();
    let mut included = 0_usize;
    for path in paths.into_iter().filter(|path| is_typescript_path(path)) {
        if included >= limits.max_paths {
            truncated = true;
            omissions.push(format!(
                "historical snapshot paths capped at {}",
                limits.max_paths
            ));
            break;
        }
        if !safe_relative_path(&path) {
            truncated = true;
            omissions.push(format!("unsafe historical path omitted: {path}"));
            continue;
        }
        let bytes = git_show_bytes(root, revision, &path)?;
        if total_bytes.saturating_add(bytes.len()) > limits.max_snapshot_bytes {
            truncated = true;
            omissions.push(format!(
                "historical snapshot bytes capped at {}",
                limits.max_snapshot_bytes
            ));
            break;
        }
        let destination = temporary.join(&path);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                format!("Unable to create historical source directory: {error}")
            })?;
        }
        fs::write(destination, &bytes).map_err(|error| {
            format!("Unable to materialize historical source evidence: {error}")
        })?;
        total_bytes += bytes.len();
        included += 1;
    }
    let built = crate::graph::build(&temporary);
    let _ = fs::remove_dir_all(&temporary);
    let (mut graph_snapshot, _) = built?;
    graph_snapshot.project_id = crate::graph::project_id(root);
    graph_snapshot.source_revision = revision.to_string();
    graph_snapshot.observation_id.clear();
    graph_snapshot.graph_version = 0;
    graph_snapshot.truncated |= truncated;
    graph_snapshot.omissions.extend(omissions);
    Ok(HistoricalSnapshot {
        schema_version: HISTORICAL_SNAPSHOT_SCHEMA.to_string(),
        project_id: graph_snapshot.project_id,
        source_revision: revision.to_string(),
        files: graph_snapshot.files,
        nodes: graph_snapshot.nodes,
        edges: graph_snapshot.edges,
        resolution_evidence: graph_snapshot.resolution_evidence,
        truncated: graph_snapshot.truncated,
        omissions: graph_snapshot.omissions,
    })
}

fn git_tree_paths(root: &Path, revision: &str) -> Result<Vec<String>, String> {
    let output = git_output(root, &["ls-tree", "-r", "--name-only", revision, "--"])?;
    Ok(output
        .lines()
        .map(|path| path.replace('\\', "/"))
        .filter(|path| !path.is_empty())
        .collect())
}

fn git_show_bytes(root: &Path, revision: &str, path: &str) -> Result<Vec<u8>, String> {
    let object = format!("{revision}:{path}");
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["show", "--format=", "--no-ext-diff", &object])
        .output()
        .map_err(|error| format!("Unable to execute historical source query: {error}"))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if detail.is_empty() {
            format!("Historical source query failed for {revision}:{path}.")
        } else {
            format!("Historical source query failed for {revision}:{path}: {detail}")
        });
    }
    Ok(output.stdout)
}

fn safe_relative_path(path: &str) -> bool {
    let candidate = Path::new(path);
    !candidate.is_absolute()
        && candidate
            .components()
            .all(|component| matches!(component, std::path::Component::Normal(_)))
}

fn focus_paths(
    root: &Path,
    context: &DiagnosticContext,
    graph_snapshot: &crate::model::GraphSnapshot,
    limits: &DiagnosticLimits,
) -> Result<FocusPathSets, String> {
    let mut focus = BTreeSet::new();
    let mut limitations = Vec::new();
    let mut starts = Vec::new();
    for uri in context
        .focus_context_refs
        .iter()
        .take(limits.max_context_refs)
    {
        let resolved = store::resolve_context(root, uri)?;
        starts.push(resolved.node_id.clone());
        if resolved.status != "current" {
            limitations.push(format!("focus Context Ref {uri} is {}.", resolved.status));
        }
        if let Some(node) = graph_snapshot
            .nodes
            .iter()
            .find(|node| node.id == resolved.node_id)
        {
            if let Some(path) = &node.path {
                focus.insert(path.clone());
            }
        } else {
            limitations.push(format!(
                "focus node {} is unavailable in the current graph.",
                resolved.node_id
            ));
        }
    }
    if context.focus_context_refs.len() > limits.max_context_refs {
        limitations.push(format!(
            "focus Context Refs capped at {}.",
            limits.max_context_refs
        ));
    }
    let mut cone = focus.clone();
    let mut queue = starts.into_iter().collect::<VecDeque<_>>();
    let mut visited = BTreeSet::new();
    while let Some(node_id) = queue.pop_front() {
        if !visited.insert(node_id.clone())
            || visited.len() > limits.max_paths.saturating_mul(8).max(1)
        {
            continue;
        }
        for edge in graph_snapshot
            .edges
            .iter()
            .filter(|edge| edge.from == node_id || edge.to == node_id)
        {
            let neighbour = if edge.from == node_id {
                &edge.to
            } else {
                &edge.from
            };
            queue.push_back(neighbour.clone());
            if let Some(node) = graph_snapshot
                .nodes
                .iter()
                .find(|node| node.id == *neighbour)
                && let Some(path) = &node.path
            {
                cone.insert(path.clone());
            }
        }
    }
    Ok((focus, cone, limitations))
}

pub(crate) fn validate_basis(basis: &GraphBasis) -> Result<(), String> {
    validate_id("graph project id", &basis.project_id)?;
    validate_id("graph id", &basis.graph_id)?;
    validate_id("observation id", &basis.observation_id)?;
    if basis.graph_version == 0 {
        return Err("currentGraphBasis.graphVersion must be greater than zero.".to_string());
    }
    validate_revision(&basis.source_revision)
}

fn validate_string_list(name: &str, values: &[String]) -> Result<(), String> {
    validate_list(name, values, MAX_LIST_ITEMS)?;
    for value in values {
        validate_text(name, value)?;
    }
    Ok(())
}

fn validate_list<T>(name: &str, values: &[T], max: usize) -> Result<(), String> {
    if values.len() > max {
        return Err(format!("{name} exceeds the bound of {max} items."));
    }
    Ok(())
}

fn validate_text(name: &str, value: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > MAX_TEXT_BYTES || value.contains('\0') {
        return Err(format!("{name} must be non-empty and bounded."));
    }
    Ok(())
}

fn validate_id(name: &str, value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > MAX_ID_BYTES
        || value
            .bytes()
            .any(|byte| !(byte.is_ascii_alphanumeric() || b"._:-".contains(&byte)))
    {
        return Err(format!("{name} must be a bounded stable identifier."));
    }
    Ok(())
}

fn validate_choice(name: &str, value: &str, allowed: &[&str]) -> Result<(), String> {
    if allowed.contains(&value) {
        Ok(())
    } else {
        Err(format!("Unsupported {name} {value:?}."))
    }
}

fn validate_revision(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 128
        || value.chars().any(char::is_whitespace)
        || value.contains([';', '|', '&', '`', '\0'])
    {
        return Err("Git revision must be a bounded single token.".to_string());
    }
    Ok(())
}

fn is_typescript_path(path: &str) -> bool {
    path.ends_with(".ts") || path.ends_with(".tsx")
}

fn is_test_path(path: &str) -> bool {
    path.ends_with(".test.ts")
        || path.ends_with(".test.tsx")
        || path.ends_with(".spec.ts")
        || path.ends_with(".spec.tsx")
        || path
            .split('/')
            .any(|part| part == "test" || part == "tests" || part == "__tests__")
}

fn current_head(root: &Path) -> Result<String, String> {
    git_output(root, &["rev-parse", "--verify", "HEAD"])
}

fn git_is_dirty(root: &Path) -> bool {
    let Ok(output) = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["status", "--porcelain", "--untracked-files=all"])
        .output()
    else {
        return true;
    };
    if !output.status.success() {
        return true;
    }
    String::from_utf8_lossy(&output.stdout).lines().any(|line| {
        let path = line.get(3..).unwrap_or_default().replace('\\', "/");
        !(path == ".flopeek" || path.starts_with(".flopeek/"))
    })
}

fn resolve_revision(root: &Path, revision: &str) -> Result<String, String> {
    validate_revision(revision)?;
    let expression = format!("{revision}^{{commit}}");
    git_output(root, &["rev-parse", "--verify", &expression])
}

fn git_log(
    root: &Path,
    last_known_good: &str,
    current: &str,
    max_count: usize,
) -> Result<Vec<CommitRecord>, String> {
    let range = format!("{last_known_good}..{current}");
    let max = max_count.to_string();
    let output = git_output(
        root,
        &[
            "log",
            "--first-parent",
            "--max-count",
            &max,
            "--format=%H%x00%P%x00%s",
            &range,
            "--",
        ],
    )?;
    let mut records = Vec::new();
    for line in output.lines() {
        let mut fields = line.splitn(3, '\0');
        let Some(sha) = fields.next() else { continue };
        let Some(parents) = fields.next() else {
            continue;
        };
        let Some(summary) = fields.next() else {
            continue;
        };
        if sha.is_empty() {
            continue;
        }
        records.push(CommitRecord {
            sha: sha.to_string(),
            parents: parents.split_whitespace().map(ToOwned::to_owned).collect(),
            summary: summary.chars().take(512).collect(),
        });
    }
    Ok(records)
}

fn git_changed_paths(
    root: &Path,
    commit: &str,
    first_parent: Option<&str>,
) -> Result<Vec<String>, String> {
    let output = if let Some(parent) = first_parent {
        git_output_bytes(
            root,
            &[
                "diff",
                "--name-status",
                "-z",
                "--find-renames",
                "--find-copies",
                "--diff-filter=ACDMRT",
                parent,
                commit,
                "--",
            ],
        )?
    } else {
        git_output_bytes(
            root,
            &[
                "diff-tree",
                "--no-commit-id",
                "--name-status",
                "-z",
                "--find-renames",
                "--find-copies",
                "--root",
                "-r",
                "--diff-filter=ACDMRT",
                commit,
                "--",
            ],
        )?
    };
    let fields = output
        .split(|byte| *byte == 0)
        .filter(|field| !field.is_empty())
        .map(|field| String::from_utf8_lossy(field).replace('\\', "/"))
        .collect::<Vec<_>>();
    let mut paths = Vec::new();
    let mut index = 0;
    while index < fields.len() {
        let status = &fields[index];
        index += 1;
        let Some(path) = fields.get(index) else { break };
        index += 1;
        paths.push(path.clone());
        if (status.starts_with('R') || status.starts_with('C'))
            && let Some(new_path) = fields.get(index)
        {
            paths.push(new_path.clone());
            index += 1;
        }
    }
    Ok(paths)
}

fn git_output(root: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .map_err(|error| format!("Unable to execute bounded Git query: {error}"))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if detail.is_empty() {
            "Bounded Git query failed.".to_string()
        } else {
            format!("Bounded Git query failed: {detail}")
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn git_output_bytes(root: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .map_err(|error| format!("Unable to execute bounded Git query: {error}"))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if detail.is_empty() {
            "Bounded Git query failed.".to_string()
        } else {
            format!("Bounded Git query failed: {detail}")
        });
    }
    Ok(output.stdout)
}

pub fn graph_basis(graph_snapshot: &crate::model::GraphSnapshot) -> GraphBasis {
    GraphBasis {
        project_id: graph_snapshot.project_id.clone(),
        graph_id: graph_snapshot.graph_id.clone(),
        graph_version: graph_snapshot.graph_version,
        source_revision: graph_snapshot.source_revision.clone(),
        observation_id: graph_snapshot.observation_id.clone(),
    }
}

pub fn context_ref_json(reference: &ContextRef) -> Result<Value, String> {
    serde_json::to_value(reference).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
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
        let fixture =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/typescript/history");
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
        let repeat = diagnose_history(&root, &context.id, DiagnosticLimits::default())
            .expect("repeat history");
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
        for entry in
            fs::read_dir(root.join(".flopeek/diagnostics/history")).expect("history entries")
        {
            let entry = entry.expect("history entry");
            let snapshot = fs::read_to_string(entry.path()).expect("snapshot");
            assert!(!snapshot.contains("export async function"));
        }
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
}
