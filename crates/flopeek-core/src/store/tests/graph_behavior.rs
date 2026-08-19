use super::*;

#[test]
fn persists_graph_atomically_and_reuses_version() {
    let root = fixture_root();
    let (snapshot, facts) = graph::build(&root).expect("build");
    let first = persist_scan(&root, snapshot, &facts).expect("persist");
    let (snapshot, facts) = graph::build(&root).expect("build again");
    let second = persist_scan(&root, snapshot, &facts).expect("persist again");
    assert_eq!(first.graph.graph_version, 1);
    assert_eq!(second.graph.graph_version, 1);
    assert_eq!(status(&root).expect("status").graph_count, 1);
    assert!(database_path(&root).is_file());
    let connection = open(&root).expect("open");
    let facts_json = connection
        .query_row("SELECT facts_json FROM source_files LIMIT 1", [], |row| {
            row.get::<_, String>(0)
        })
        .expect("facts");
    assert!(!facts_json.contains("export const main = 1"));
    assert!(!facts_json.contains("Promise"));
    drop(connection);
    let current = current_graph(&root).expect("current graph").expect("graph");
    let node = current
        .nodes
        .iter()
        .find(|node| node.kind == "variable")
        .expect("variable node");
    let details = node_details(&root, &node.id).expect("node details");
    assert_eq!(details["evidenceClass"], "static");
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn repository_manifest_is_persisted_without_checkout_path() {
    let root = fixture_root();
    fs::write(
        root.join(crate::identity::MANIFEST_PATH),
        r#"{"schemaVersion":"flopeek-repository-identity/v1","repositoryId":"repo_123e4567-e89b-12d3-a456-426614174000"}"#,
    )
    .expect("identity manifest");
    let (snapshot, facts) = graph::build(&root).expect("build");
    let result = persist_scan(&root, snapshot, &facts).expect("persist");
    assert_eq!(result.identity_basis.status, "available");
    assert_eq!(
        result.identity_basis.repository_id.as_deref(),
        Some("repo_123e4567-e89b-12d3-a456-426614174000")
    );
    let encoded = serde_json::to_string(&result).expect("encoded scan");
    assert!(!encoded.contains(&root.to_string_lossy().to_string()));
    let current = current_graph(&root).expect("current graph").expect("graph");
    assert_eq!(current.identity_basis.status, "available");
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn context_ref_becomes_stale_after_graph_changes() {
    let root = fixture_root();
    let (snapshot, facts) = graph::build(&root).expect("build");
    let first = persist_scan(&root, snapshot, &facts).expect("persist");
    let uri = first.context_refs[0].uri.clone();
    fs::write(root.join("src/other.ts"), "export const other = 2;").expect("write");
    let (snapshot, facts) = graph::build(&root).expect("build changed");
    let second = persist_scan(&root, snapshot, &facts).expect("persist changed");
    assert_eq!(second.graph.graph_version, 2);
    assert_eq!(
        resolve_context(&root, &uri).expect("resolve").status,
        "current"
    );
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn focused_symbol_and_direct_edge_changes_are_stale_but_unrelated_nodes_stay_current() {
    let root = fixture_root();
    fs::write(root.join("src/helper.ts"), "export const helper = 1;\n").expect("helper");
    let (snapshot, facts) = graph::build(&root).expect("build");
    let first = persist_scan(&root, snapshot, &facts).expect("persist");
    let focused = first
        .context_refs
        .iter()
        .find(|reference| {
            first.graph.nodes.iter().any(|node| {
                node.id == reference.node_id
                    && node.path.as_deref() == Some("src/main.ts")
                    && node.kind == "file"
            })
        })
        .expect("focused file")
        .uri
        .clone();
    fs::write(
        root.join("src/unrelated.ts"),
        "export const unrelated = 1;\n",
    )
    .expect("unrelated");
    let (snapshot, facts) = graph::build(&root).expect("unrelated graph");
    persist_scan(&root, snapshot, &facts).expect("persist unrelated");
    assert_eq!(
        resolve_context(&root, &focused)
            .expect("resolve unrelated")
            .status,
        "current"
    );
    fs::write(root.join("src/main.ts"), "export const main = 2;\n").expect("focused change");
    let (snapshot, facts) = graph::build(&root).expect("focused graph");
    persist_scan(&root, snapshot, &facts).expect("persist focused");
    let resolved = resolve_context(&root, &focused).expect("resolve focused");
    assert_eq!(resolved.status, "stale");
    assert!(resolved.freshness_reason.contains("fingerprint"));
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn direct_import_target_change_stales_caller_and_target_refs() {
    let root = fixture_root();
    fs::write(
        root.join("src/target.ts"),
        "export function target() { return 1; }\n",
    )
    .expect("target");
    fs::write(
        root.join("src/caller.ts"),
        "import { target } from './target'; export function run() { target(); }\n",
    )
    .expect("caller");
    let (snapshot, facts) = graph::build(&root).expect("build initial");
    let first = persist_scan(&root, snapshot, &facts).expect("persist initial");
    let caller_ref = first
        .context_refs
        .iter()
        .find(|candidate| {
            first.graph.nodes.iter().any(|node| {
                node.id == candidate.node_id
                    && node.path.as_deref() == Some("src/caller.ts")
                    && node.name.as_deref() == Some("run")
            })
        })
        .expect("caller ref")
        .uri
        .clone();
    let target_ref = first
        .context_refs
        .iter()
        .find(|candidate| {
            first.graph.nodes.iter().any(|node| {
                node.id == candidate.node_id
                    && node.path.as_deref() == Some("src/target.ts")
                    && node.name.as_deref() == Some("target")
            })
        })
        .expect("target ref")
        .uri
        .clone();
    fs::write(
        root.join("src/other.ts"),
        "export function other() { return 2; }\n",
    )
    .expect("other");
    fs::write(
        root.join("src/caller.ts"),
        "import { other } from './other'; export function run() { other(); }\n",
    )
    .expect("changed caller");
    let (snapshot, facts) = graph::build(&root).expect("build changed");
    persist_scan(&root, snapshot, &facts).expect("persist changed");
    let caller = resolve_context(&root, &caller_ref).expect("resolve caller");
    let target = resolve_context(&root, &target_ref).expect("resolve target");
    assert_eq!(caller.status, "stale");
    assert_eq!(target.status, "stale");
    assert!(caller.freshness_reason.contains("fingerprint"));
    assert!(target.freshness_reason.contains("fingerprint"));
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn method_body_change_stales_only_method_and_member_addition_stales_class() {
    let root = fixture_root();
    fs::write(
        root.join("src/payment.ts"),
        "export class Payment { private secret() { return 1; } sibling() { return 2; } }\n",
    )
    .expect("class");
    let (snapshot, facts) = graph::build(&root).expect("build initial");
    let first = persist_scan(&root, snapshot, &facts).expect("persist initial");
    let ref_for = |name: &str| {
        first
            .context_refs
            .iter()
            .find(|candidate| {
                first.graph.nodes.iter().any(|node| {
                    node.id == candidate.node_id
                        && node.path.as_deref() == Some("src/payment.ts")
                        && node.name.as_deref() == Some(name)
                })
            })
            .expect("context ref")
            .uri
            .clone()
    };
    let class_ref = ref_for("Payment");
    let secret_ref = ref_for("method:Payment.secret");
    let sibling_ref = ref_for("method:Payment.sibling");

    fs::write(
        root.join("src/payment.ts"),
        "// formatting only\nexport class Payment { private secret() { return 3; } sibling() { return 2; } }\n",
    )
    .expect("method body");
    let (snapshot, facts) = graph::build(&root).expect("build changed");
    persist_scan(&root, snapshot, &facts).expect("persist changed");
    assert_eq!(
        resolve_context(&root, &secret_ref).expect("secret").status,
        "stale"
    );
    assert_eq!(
        resolve_context(&root, &sibling_ref)
            .expect("sibling")
            .status,
        "current"
    );
    assert_eq!(
        resolve_context(&root, &class_ref).expect("class").status,
        "current"
    );

    fs::write(
        root.join("src/payment.ts"),
        "export class Payment { private secret() { return 3; } sibling() { return 2; } added() {} }\n",
    )
    .expect("member addition");
    let (snapshot, facts) = graph::build(&root).expect("build member addition");
    persist_scan(&root, snapshot, &facts).expect("persist member addition");
    assert_eq!(
        resolve_context(&root, &class_ref)
            .expect("class after addition")
            .status,
        "stale"
    );
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn identical_structure_across_revision_gets_new_observation_and_reuses_graph_version() {
    let root = fixture_root();
    git(&root, &["init"]);
    git(
        &root,
        &["config", "user.email", "flopeek-test@example.invalid"],
    );
    git(&root, &["config", "user.name", "Flopeek Test"]);
    git(&root, &["add", "."]);
    git(&root, &["commit", "-m", "source A"]);
    let (snapshot, facts) = graph::build(&root).expect("build A");
    let first = persist_scan(&root, snapshot, &facts).expect("scan A");
    let first_ref = first.context_refs[0].clone();
    let first_resolved = resolve_context(&root, &first_ref.uri).expect("resolve first ref");
    assert_eq!(first_ref, first_resolved);
    assert_eq!(first_ref.origin_observation_id, first.graph.observation_id);
    assert_eq!(
        first_ref
            .current_basis
            .as_ref()
            .expect("first current basis")
            .observation_id,
        first.graph.observation_id
    );
    fs::write(root.join("README.md"), "documentation-only change\n").expect("README");
    git(&root, &["add", "README.md"]);
    git(&root, &["commit", "-m", "README only"]);
    let (snapshot, facts) = graph::build(&root).expect("build README");
    let second = persist_scan(&root, snapshot, &facts).expect("scan README");
    assert_eq!(first.graph.graph_id, second.graph.graph_id);
    assert_eq!(first.graph.graph_version, second.graph.graph_version);
    assert_ne!(first.graph.observation_id, second.graph.observation_id);
    let delta = get_observation_delta(&root, None, crate::temporal::DeltaLimits::default())
        .expect("README observation delta");
    assert_eq!(delta.status, "complete");
    assert_eq!(delta.graph_relation, "same-structural-graph");
    assert_eq!(
        delta.counts,
        crate::model::ObservationDeltaCounts::default()
    );
    let second_ref = second
        .context_refs
        .iter()
        .find(|reference| reference.uri == first_ref.uri)
        .expect("same Context Ref URI");
    assert_eq!(second_ref.origin_observation_id, first.graph.observation_id);
    assert_eq!(
        second_ref
            .current_basis
            .as_ref()
            .expect("second current basis")
            .observation_id,
        second.graph.observation_id
    );
    assert_eq!(second_ref.status, "current");
    assert_eq!(
        second_ref.freshness_reason,
        "node AST and direct-edge fingerprint matches"
    );
    assert_eq!(
        second_ref,
        &resolve_context(&root, &second_ref.uri).expect("resolve canonical second ref")
    );
    let connection = open(&root).expect("open");
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM graph_observations", [], |row| row
                .get::<_, i64>(0))
            .expect("observations"),
        2
    );
    let manifest = connection
        .query_row(
            "SELECT source_manifest_json FROM graph_observations ORDER BY observed_at LIMIT 1",
            [],
            |row| row.get::<_, String>(0),
        )
        .expect("manifest");
    assert!(!manifest.contains("export const main"));
    assert_eq!(
        resolve_context(&root, &first.context_refs[0].uri)
            .expect("resolve old ref")
            .status,
        "current"
    );
    drop(connection);
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn comment_only_tsconfig_change_creates_observation_but_keeps_context_current() {
    let root = fixture_root();
    fs::write(
        root.join("tsconfig.json"),
        r#"{"compilerOptions":{"baseUrl":".","paths":{"@app/*":["src/*"]}}}"#,
    )
    .expect("config");
    let (snapshot, facts) = graph::build(&root).expect("build first");
    let first = persist_scan(&root, snapshot, &facts).expect("persist first");
    let reference = first.context_refs[0].uri.clone();
    fs::write(
        root.join("tsconfig.json"),
        "// documentation-only config comment\n{\n  \"compilerOptions\": { \"baseUrl\": \".\", \"paths\": { \"@app/*\": [\"src/*\"] } }\n}\n",
    )
    .expect("comment-only config");
    let (snapshot, facts) = graph::build(&root).expect("build second");
    let second = persist_scan(&root, snapshot, &facts).expect("persist second");
    assert_eq!(first.graph.graph_id, second.graph.graph_id);
    assert_eq!(first.graph.graph_version, second.graph.graph_version);
    assert_ne!(first.graph.observation_id, second.graph.observation_id);
    assert_eq!(second.graph.module_resolution.status, "complete");
    assert_eq!(
        resolve_context(&root, &reference)
            .expect("resolve current reference")
            .status,
        "current"
    );
    let connection = open(&root).expect("open");
    assert_eq!(
        connection
            .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .expect("version"),
        CURRENT_USER_VERSION
    );
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM graph_observations", [], |row| row
                .get::<_, i64>(0))
            .expect("observations"),
        2
    );
    drop(connection);
    fs::remove_dir_all(root).expect("cleanup");
}
