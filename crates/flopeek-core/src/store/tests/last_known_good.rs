use super::*;
use crate::model::{
    DIAGNOSTIC_CONTEXT_SCHEMA, DiagnosticContext, LAST_KNOWN_GOOD_SCHEMA, LastKnownGoodBinding,
};
use std::fs;
use std::path::Path;

fn git(root: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .expect("git");
    assert!(output.status.success(), "git {args:?}");
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

#[test]
fn last_known_good_is_append_only_and_human_confirmation_is_required() {
    let root = fixture_root();
    fs::write(
        root.join(crate::identity::MANIFEST_PATH),
        r#"{"schemaVersion":"flopeek-repository-identity/v1","repositoryId":"repo_123e4567-e89b-12d3-a456-426614174000"}"#,
    )
    .expect("manifest");
    git(&root, &["init"]);
    git(
        &root,
        &["config", "user.email", "flopeek-test@example.invalid"],
    );
    git(&root, &["config", "user.name", "Flopeek Test"]);
    git(&root, &["add", "."]);
    git(&root, &["commit", "-m", "baseline"]);
    let revision = std::process::Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["rev-parse", "HEAD"])
        .output()
        .expect("revision");
    let revision = String::from_utf8_lossy(&revision.stdout).trim().to_string();
    let (snapshot, facts) = graph::build(&root).expect("build");
    let scan = persist_scan(&root, snapshot, &facts).expect("scan");
    let context = create_diagnostic_context(
        &root,
        DiagnosticContext {
            schema_version: DIAGNOSTIC_CONTEXT_SCHEMA.to_string(),
            id: "lkg-context".to_string(),
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
            constraints: vec![],
            acceptance_criteria: vec![],
            unresolved_questions: vec![],
            actor: "agent".to_string(),
            created_at: 0,
            status: "open".to_string(),
            supersedes: None,
        },
    )
    .expect("context");
    let repository_id = crate::identity::resolve(&root)
        .expect("identity")
        .repository_id
        .expect("repository id");
    let binding = |id: &str, actor: &str, actor_kind: &str, status: &str| LastKnownGoodBinding {
        schema_version: LAST_KNOWN_GOOD_SCHEMA.to_string(),
        binding_id: id.to_string(),
        repository_id: repository_id.clone(),
        project_id: scan.project_id.clone(),
        context_id: context.id.clone(),
        git_revision: revision.clone(),
        observation_id: None,
        event_id: None,
        graph_basis: None,
        actor: actor.to_string(),
        actor_kind: actor_kind.to_string(),
        evidence: Vec::new(),
        status: status.to_string(),
        predecessor_binding_id: None,
        superseded_binding_id: None,
        created_at: 0,
        validation: Default::default(),
    };
    let proposed =
        create_last_known_good_binding(&root, binding("proposed", "agent", "agent", "proposed"))
            .expect("proposed");
    assert_eq!(proposed.validation.status, "valid");
    assert!(
        create_last_known_good_binding(
            &root,
            binding("agent-confirm", "agent", "agent", "confirmed")
        )
        .is_err()
    );
    let mut confirmed = binding("confirmed", "human", "human", "confirmed");
    confirmed.predecessor_binding_id = Some(proposed.binding_id.clone());
    let confirmed = create_last_known_good_binding(&root, confirmed).expect("confirmed");
    assert_eq!(confirmed.validation.status, "valid");
    assert_eq!(
        get_last_known_good(&root, &context.id).expect("get").status,
        "confirmed"
    );
    assert_eq!(
        list_last_known_good_history(&root, &context.id)
            .expect("history")
            .len(),
        2
    );
    assert_eq!(
        validate_last_known_good(&root, &context.id, &confirmed.binding_id)
            .expect("validate")
            .validation
            .status,
        "valid"
    );
    let mut revoked = binding("revoked", "human", "human", "revoked");
    revoked.predecessor_binding_id = Some(confirmed.binding_id.clone());
    create_last_known_good_binding(&root, revoked).expect("revoked");
    let resolution = get_last_known_good(&root, &context.id).expect("revoked resolution");
    assert_eq!(resolution.status, "revoked");
    assert_eq!(
        resolution
            .binding
            .as_ref()
            .map(|value| value.binding_id.as_str()),
        Some("revoked")
    );
    assert!(
        confirmed_last_known_good(&root, &context.id)
            .expect("effective confirmed")
            .is_none()
    );
    assert!(
        get_diagnostic_context(&root, &context.id)
            .expect("context after revocation")
            .last_known_good_binding_id
            .is_none()
    );
    let diagnosis = crate::diagnostic::diagnose_history(
        &root,
        &context.id,
        crate::model::DiagnosticLimits::default(),
    )
    .expect("diagnosis after revocation");
    assert!(diagnosis.last_known_good_binding.is_none());
    assert!(diagnosis.candidates.is_empty());

    let mut reconfirmed = binding("reconfirmed", "human", "human", "confirmed");
    reconfirmed.predecessor_binding_id = Some("revoked".to_string());
    let reconfirmed =
        create_last_known_good_binding(&root, reconfirmed).expect("reconfirmed binding");
    let mut superseded = binding("superseded", "human", "human", "superseded");
    superseded.predecessor_binding_id = Some(reconfirmed.binding_id.clone());
    superseded.superseded_binding_id = Some(reconfirmed.binding_id);
    create_last_known_good_binding(&root, superseded).expect("superseded");
    assert_eq!(
        get_last_known_good(&root, &context.id)
            .expect("superseded resolution")
            .status,
        "superseded"
    );
    assert!(
        confirmed_last_known_good(&root, &context.id)
            .expect("superseded effective confirmed")
            .is_none()
    );
    assert!(
        get_diagnostic_context(&root, &context.id)
            .expect("context after supersession")
            .last_known_good_binding_id
            .is_none()
    );
    assert_eq!(
        list_last_known_good_history(&root, &context.id)
            .expect("ordered history")
            .iter()
            .map(|value| value.binding_id.as_str())
            .collect::<Vec<_>>(),
        vec![
            "proposed",
            "confirmed",
            "revoked",
            "reconfirmed",
            "superseded"
        ]
    );
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn last_known_good_requires_current_first_parent_lineage() {
    let root = fixture_root();
    fs::write(
        root.join(crate::identity::MANIFEST_PATH),
        r#"{"schemaVersion":"flopeek-repository-identity/v1","repositoryId":"repo_123e4567-e89b-12d3-a456-426614174000"}"#,
    )
    .expect("manifest");
    git(&root, &["init"]);
    git(
        &root,
        &["config", "user.email", "flopeek-test@example.invalid"],
    );
    git(&root, &["config", "user.name", "Flopeek Test"]);
    fs::write(root.join("main.ts"), "export const main = 1;\n").expect("baseline");
    git(&root, &["add", "."]);
    git(&root, &["commit", "-m", "baseline"]);
    let main_branch = git(&root, &["branch", "--show-current"]);
    git(&root, &["switch", "-c", "side"]);
    fs::write(root.join("side.ts"), "export const side = 1;\n").expect("side source");
    git(&root, &["add", "."]);
    git(&root, &["commit", "-m", "side"]);
    let side_revision = git(&root, &["rev-parse", "HEAD"]);
    git(&root, &["switch", &main_branch]);
    fs::write(root.join("main.ts"), "export const main = 2;\n").expect("main current");
    git(&root, &["add", "."]);
    git(&root, &["commit", "-m", "main current"]);

    let (snapshot, facts) = graph::build(&root).expect("build");
    let scan = persist_scan(&root, snapshot, &facts).expect("scan");
    let context = create_diagnostic_context(
        &root,
        DiagnosticContext {
            schema_version: DIAGNOSTIC_CONTEXT_SCHEMA.to_string(),
            id: "side-lineage-context".to_string(),
            project_id: scan.project_id.clone(),
            revision: 0,
            intent: "diagnose".to_string(),
            symptom: "side lineage".to_string(),
            expected_behavior: "first-parent only".to_string(),
            focus_context_refs: vec![scan.context_refs[0].uri.clone()],
            focus_flow_refs: Vec::new(),
            current_graph_basis: crate::diagnostic::graph_basis(&scan.graph),
            last_known_good_basis: None,
            last_known_good_binding_id: None,
            constraints: vec![],
            acceptance_criteria: vec![],
            unresolved_questions: vec![],
            actor: "agent".to_string(),
            created_at: 0,
            status: "open".to_string(),
            supersedes: None,
        },
    )
    .expect("context");
    let repository_id = crate::identity::resolve(&root)
        .expect("identity")
        .repository_id
        .expect("repository id");
    let binding = LastKnownGoodBinding {
        schema_version: LAST_KNOWN_GOOD_SCHEMA.to_string(),
        binding_id: "side-proposal".to_string(),
        repository_id,
        project_id: scan.project_id,
        context_id: context.id.clone(),
        git_revision: side_revision,
        observation_id: None,
        event_id: None,
        graph_basis: None,
        actor: "agent".to_string(),
        actor_kind: "agent".to_string(),
        evidence: Vec::new(),
        status: "proposed".to_string(),
        predecessor_binding_id: None,
        superseded_binding_id: None,
        created_at: 0,
        validation: Default::default(),
    };
    let proposed = create_last_known_good_binding(&root, binding).expect("side proposal");
    assert!(proposed.validation.revision_available);
    assert!(!proposed.validation.first_parent_range_available);
    assert_eq!(proposed.validation.status, "invalid");
    assert!(
        proposed
            .validation
            .limitations
            .iter()
            .any(|value| value == "git-revision-not-on-current-first-parent-lineage")
    );
    let mut confirmation = proposed;
    confirmation.binding_id = "side-confirmation".to_string();
    confirmation.actor = "human".to_string();
    confirmation.actor_kind = "human".to_string();
    confirmation.status = "confirmed".to_string();
    confirmation.predecessor_binding_id = Some("side-proposal".to_string());
    confirmation.validation = Default::default();
    assert!(create_last_known_good_binding(&root, confirmation).is_err());
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn fresh_and_upgraded_v10_schema_match_and_migration_rolls_back() {
    let fresh_root = fixture_root();
    let fresh = open(&fresh_root).expect("fresh v10");
    let fresh_schema = schema_snapshot(&fresh);
    assert_eq!(
        fresh
            .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .expect("fresh version"),
        CURRENT_USER_VERSION
    );
    drop(fresh);

    let upgraded_root = fixture_root();
    initialize_v9_database(&upgraded_root);
    let project_id = graph::project_id(&upgraded_root);
    let connection = rusqlite::Connection::open(database_path(&upgraded_root)).expect("v9");
    connection
        .execute(
            "INSERT INTO diagnostic_contexts(id, project_id, revision, payload_json, created_at)
             VALUES('legacy-context', ?1, 1, '{\"legacyMemory\":\"keep\"}', 1)",
            params![project_id],
        )
        .expect("legacy context");
    drop(connection);
    let upgraded = open(&upgraded_root).expect("upgrade v10");
    assert_eq!(schema_snapshot(&upgraded), fresh_schema);
    assert_eq!(
        upgraded
            .query_row(
                "SELECT payload_json FROM diagnostic_contexts WHERE id = 'legacy-context'",
                [],
                |row| row.get::<_, String>(0),
            )
            .expect("legacy memory"),
        "{\"legacyMemory\":\"keep\"}"
    );
    drop(upgraded);
    fs::remove_dir_all(fresh_root).expect("cleanup fresh");
    fs::remove_dir_all(upgraded_root).expect("cleanup upgraded");

    let failed_root = fixture_root();
    initialize_v9_database(&failed_root);
    let connection = rusqlite::Connection::open(database_path(&failed_root)).expect("failed v9");
    connection
        .execute_batch(
            "CREATE TABLE last_known_good_bindings (binding_id TEXT PRIMARY KEY NOT NULL);",
        )
        .expect("conflicting table");
    drop(connection);
    assert!(open(&failed_root).is_err());
    let connection = rusqlite::Connection::open(database_path(&failed_root)).expect("reopen");
    assert_eq!(
        connection
            .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .expect("failed version"),
        9
    );
    drop(connection);
    fs::remove_dir_all(failed_root).expect("cleanup failed");
}
