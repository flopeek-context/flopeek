//! Store behavior tests.

use super::migrations::{migration_v1, migration_v2, migration_v3, migration_v4, migration_v5};
use super::*;
use crate::graph;
use std::fs;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn fixture_root() -> PathBuf {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("flopeek-store-{suffix}"));
    fs::create_dir_all(root.join("src")).expect("mkdir");
    fs::write(root.join("src/main.ts"), "export const main = 1;").expect("write");
    root
}

fn flow_fixture_root() -> PathBuf {
    let root = fixture_root();
    fs::write(
        root.join("package.json"),
        r#"{
            "scripts": {
                "start": "tsx src/main.ts",
                "unsupported": "tsx src/main.ts && echo credential-sentinel"
            },
            "bin": {"checkout": "src/main.ts"},
            "main": "src/main",
            "module": "src/main.ts"
        }"#,
    )
    .expect("package manifest");
    fs::write(
        root.join("src/main.ts"),
        "export function main() { helper(); }\nfunction helper() { return 'source-body-sentinel'; }\nmain();\n",
    )
    .expect("main source");
    fs::create_dir_all(root.join("tests")).expect("tests");
    fs::write(
        root.join("tests/main.test.ts"),
        "import { main } from '../src/main'; main();\n",
    )
    .expect("test source");
    root
}

fn schema_snapshot(connection: &rusqlite::Connection) -> Vec<(String, String, String)> {
    connection
        .prepare(
            "SELECT type, name, COALESCE(sql, '') FROM sqlite_master
             WHERE name NOT LIKE 'sqlite_%' AND type IN ('table', 'index')
             ORDER BY type, name",
        )
        .expect("schema query")
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .expect("schema rows")
        .collect::<Result<Vec<_>, _>>()
        .expect("schema snapshot")
}

fn initialize_v5_database(root: &Path) {
    fs::create_dir_all(root.join(STORE_DIRECTORY)).expect("store directory");
    let mut connection = rusqlite::Connection::open(database_path(root)).expect("sqlite");
    connection
        .execute_batch("PRAGMA foreign_keys = ON;")
        .expect("foreign keys");
    for target in 1..=5 {
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .expect("migration transaction");
        match target {
            1 => migration_v1(&transaction).expect("v1"),
            2 => migration_v2(&transaction).expect("v2"),
            3 => migration_v3(&transaction).expect("v3"),
            4 => migration_v4(&transaction).expect("v4"),
            5 => migration_v5(&transaction).expect("v5"),
            _ => unreachable!(),
        }
        transaction
            .execute_batch(&format!("PRAGMA user_version = {target};"))
            .expect("version");
        transaction.commit().expect("migration commit");
    }
}

fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .expect("git");
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

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

#[test]
fn failed_v3_to_v4_migration_preserves_user_version_and_existing_rows() {
    let root = fixture_root();
    let connection = open(&root).expect("fresh database");
    connection
        .execute(
            "INSERT INTO graph_versions(
                graph_version, graph_id, project_id, source_revision,
                created_at, truncated, omissions_json
             ) VALUES(1, 'graph-old', 'project-old', 'revision-old', 1, 0, '[]')",
            [],
        )
        .expect("old graph");
    connection
        .execute_batch(
            "PRAGMA user_version = 3;
             CREATE TRIGGER fail_observation_migration
             BEFORE INSERT ON graph_observations
             BEGIN SELECT RAISE(ABORT, 'forced migration failure'); END;",
        )
        .expect("prepare failed migration");
    drop(connection);
    assert!(open(&root).is_err());
    let connection = rusqlite::Connection::open(database_path(&root)).expect("inspect");
    assert_eq!(
        connection
            .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .expect("version"),
        3
    );
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM graph_versions", [], |row| row
                .get::<_, i64>(0))
            .expect("graph row"),
        1
    );
    drop(connection);
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn legacy_context_ref_is_unresolved_without_evidence_and_uses_file_fallback_with_hash() {
    let root = fixture_root();
    let (snapshot, facts) = graph::build(&root).expect("build");
    let result = persist_scan(&root, snapshot, &facts).expect("persist");
    let reference = &result.context_refs[0];
    let connection = open(&root).expect("open");
    connection
        .execute(
            "UPDATE context_refs
             SET origin_observation_id = '', origin_fingerprint = '', fingerprint_scope = 'legacy-file-v1'
             WHERE uri = ?1",
            params![reference.uri],
        )
        .expect("legacy unresolved");
    drop(connection);
    assert_eq!(
        resolve_context(&root, &reference.uri)
            .expect("resolve unresolved")
            .status,
        "unresolved"
    );
    let (snapshot, facts) = graph::build(&root).expect("rebuild legacy graph");
    let rescanned = persist_scan(&root, snapshot, &facts).expect("persist unresolved legacy");
    assert_eq!(
        rescanned
            .context_refs
            .iter()
            .find(|candidate| candidate.uri == reference.uri)
            .expect("legacy Context Ref after rescan")
            .status,
        "unresolved"
    );
    let connection = open(&root).expect("reopen");
    let (observation_id, hash) = connection
        .query_row(
            "SELECT observation.observation_id, source.hash
             FROM graph_observations observation
             JOIN source_files source ON source.graph_version = observation.graph_version
             JOIN graph_nodes node ON node.graph_version = source.graph_version
                 AND node.path = source.path
             WHERE observation.graph_version = ?1 AND node.node_id = ?2 LIMIT 1",
            params![result.graph.graph_version as i64, reference.node_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .expect("legacy fallback evidence");
    connection
        .execute(
            "UPDATE context_refs
             SET origin_observation_id = ?1, origin_fingerprint = ?2, fingerprint_scope = 'legacy-file-v1'
             WHERE uri = ?3",
            params![observation_id, hash, reference.uri],
        )
        .expect("legacy evidence");
    drop(connection);
    assert_eq!(
        resolve_context(&root, &reference.uri)
            .expect("resolve legacy current")
            .status,
        "current"
    );
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn legacy_v4_facts_are_readable_but_resolution_evidence_is_unavailable() {
    let root = fixture_root();
    let (snapshot, facts) = graph::build(&root).expect("build");
    persist_scan(&root, snapshot, &facts).expect("persist");
    let connection = open(&root).expect("open");
    let facts_json = connection
        .query_row("SELECT facts_json FROM source_files LIMIT 1", [], |row| {
            row.get::<_, String>(0)
        })
        .expect("facts");
    let mut legacy: serde_json::Value = serde_json::from_str(&facts_json).expect("json");
    let object = legacy.as_object_mut().expect("facts object");
    object.remove("schema_version");
    object.remove("resolution_records");
    connection
        .execute(
            "UPDATE source_files SET facts_json = ?1",
            params![serde_json::to_string(&legacy).expect("legacy facts")],
        )
        .expect("write legacy facts");
    drop(connection);

    let current = current_graph(&root)
        .expect("read legacy graph")
        .expect("current graph");
    assert_eq!(current.schema_version, crate::model::GRAPH_SCHEMA);
    assert_eq!(current.resolution_evidence.status, "unavailable");
    assert!(
        current
            .resolution_evidence
            .omissions
            .iter()
            .any(|omission| omission == "legacy-facts-without-resolution-evidence")
    );
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn corrupted_graph_rows_are_rebuilt_transactionally() {
    let root = fixture_root();
    let (snapshot, facts) = graph::build(&root).expect("build");
    let first = persist_scan(&root, snapshot, &facts).expect("persist");
    let connection = open(&root).expect("open");
    connection
        .execute(
            "DELETE FROM graph_nodes WHERE graph_version = ?1",
            params![first.graph.graph_version],
        )
        .expect("corrupt rows");
    drop(connection);
    let (snapshot, facts) = graph::build(&root).expect("build again");
    let recovered = persist_scan(&root, snapshot, &facts).expect("recover");
    assert_eq!(recovered.graph.graph_version, first.graph.graph_version);
    assert_eq!(
        status(&root).expect("status").node_count,
        recovered.graph.nodes.len() as u64
    );
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn context_ref_failure_rolls_back_graph_rows_observation_and_current_state() {
    let root = fixture_root();
    let (snapshot, facts) = graph::build(&root).expect("build");
    let first = persist_scan(&root, snapshot, &facts).expect("first persist");
    fs::write(root.join("src/other.ts"), "export const other = 2;\n").expect("change");
    let (snapshot, facts) = graph::build(&root).expect("changed build");
    let connection = open(&root).expect("open");
    connection
        .execute_batch(
            "CREATE TRIGGER fail_context_ref BEFORE INSERT ON context_refs
             WHEN NEW.graph_version > 1
             BEGIN SELECT RAISE(ABORT, 'forced Context Ref failure'); END;",
        )
        .expect("trigger");
    drop(connection);
    assert!(persist_scan(&root, snapshot, &facts).is_err());
    let connection = open(&root).expect("reopen");
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM graph_versions", [], |row| row
                .get::<_, i64>(0))
            .expect("graphs"),
        1
    );
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM graph_observations", [], |row| row
                .get::<_, i64>(0))
            .expect("observations"),
        1
    );
    assert_eq!(
        status(&root).expect("status").current_graph_version,
        Some(first.graph.graph_version)
    );
    drop(connection);
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn flow_ref_round_trip_is_canonical_and_origin_is_immutable() {
    let root = flow_fixture_root();
    let (snapshot, facts) = graph::build(&root).expect("build");
    let first = persist_scan(&root, snapshot, &facts).expect("first persist");
    assert!(!first.flow_refs.is_empty());
    let first_ref = first.flow_refs[0].clone();
    assert_eq!(first_ref.status, "current");
    assert_eq!(first_ref.freshness_reason, "origin-observation-current");
    assert_eq!(first_ref.origin_observation_id, first.graph.observation_id);
    assert_eq!(
        first_ref
            .current_basis
            .as_ref()
            .map(|basis| &basis.observation_id),
        Some(&first.graph.observation_id)
    );
    assert_eq!(
        resolve_flow(&root, &first_ref.uri).expect("resolve"),
        first_ref
    );

    fs::write(
        root.join("package.json"),
        "{\n  \"scripts\": {\"start\": \"tsx src/main.ts\", \"unsupported\": \"tsx src/main.ts && echo credential-sentinel\"},\n  \"bin\": {\"checkout\": \"src/main.ts\"},\n  \"main\": \"src/main\",\n  \"module\": \"src/main.ts\"\n}\n",
    )
    .expect("format package");
    let (snapshot, facts) = graph::build(&root).expect("second build");
    let second = persist_scan(&root, snapshot, &facts).expect("second persist");
    assert_eq!(second.graph.graph_id, first.graph.graph_id);
    assert_eq!(second.graph.graph_version, first.graph.graph_version);
    assert_ne!(second.graph.observation_id, first.graph.observation_id);
    let second_ref = second
        .flow_refs
        .iter()
        .find(|reference| reference.uri == first_ref.uri)
        .expect("same flow ref");
    assert_eq!(
        second_ref.origin_observation_id,
        first_ref.origin_observation_id
    );
    assert_eq!(second_ref.status, "current");
    assert_eq!(second_ref.freshness_reason, "flow-fingerprint-match");
    assert_eq!(
        second_ref
            .current_basis
            .as_ref()
            .map(|basis| &basis.observation_id),
        Some(&second.graph.observation_id)
    );
    assert_eq!(
        resolve_flow(&root, &first_ref.uri).expect("resolve second"),
        *second_ref
    );

    fs::write(
        root.join("package.json"),
        "{\"scripts\":{\"start\":\"tsx src/other.ts\",\"unsupported\":\"tsx src/main.ts && echo credential-sentinel\"},\"bin\":{\"checkout\":\"src/main.ts\"},\"main\":\"src/main\",\"module\":\"src/main.ts\"}",
    )
    .expect("changed entry target");
    fs::write(
        root.join("src/other.ts"),
        "export function other() { return 'other'; }\n",
    )
    .expect("other target");
    let (snapshot, facts) = graph::build(&root).expect("third build");
    let third = persist_scan(&root, snapshot, &facts).expect("third persist");
    let stale = resolve_flow(&root, &first_ref.uri).expect("stale flow");
    assert_eq!(stale.status, "stale");
    assert_eq!(stale.freshness_reason, "flow-fingerprint-changed");
    assert_eq!(stale.origin_observation_id, first_ref.origin_observation_id);
    assert_eq!(
        stale
            .current_basis
            .as_ref()
            .map(|basis| &basis.observation_id),
        Some(&third.graph.observation_id)
    );
    let connection = open(&root).expect("open");
    let payloads = connection
        .prepare("SELECT payload_json FROM graph_flows UNION ALL SELECT entry_json FROM graph_flow_evidence")
        .expect("flow payloads")
        .query_map([], |row| row.get::<_, String>(0))
        .expect("flow rows")
        .collect::<Result<Vec<_>, _>>()
        .expect("flow payload values")
        .join("\n");
    assert!(!payloads.contains("credential-sentinel"));
    assert!(!payloads.contains("source-body-sentinel"));
    assert!(!payloads.contains(root.to_string_lossy().as_ref()));
    drop(connection);
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn flow_ref_reports_missing_identity_and_wrong_project_explicitly() {
    let root = flow_fixture_root();
    let (snapshot, facts) = graph::build(&root).expect("build");
    let first = persist_scan(&root, snapshot, &facts).expect("persist");
    let reference = first.flow_refs.first().expect("flow ref").clone();
    let connection = open(&root).expect("open");
    let wrong_uri = flow_ref::uri("different-project", &reference.graph_id, &reference.flow_id);
    let wrong =
        flow_ref::resolve(&connection, &wrong_uri, &first.project_id).expect("wrong project");
    assert_eq!(wrong.status, "wrong-project");
    drop(connection);
    let connection = open(&root).expect("open again");
    connection
        .execute(
            "DELETE FROM graph_flows WHERE graph_version = ?1 AND flow_id = ?2",
            params![first.graph.graph_version as i64, reference.flow_id],
        )
        .expect("remove current flow");
    drop(connection);
    let stale = resolve_flow(&root, &reference.uri).expect("stale resolve");
    assert_eq!(stale.status, "stale");
    assert_eq!(stale.freshness_reason, "flow-identity-missing");
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn flow_ref_remains_current_for_unrelated_source_but_tracks_related_test_changes() {
    let root = flow_fixture_root();
    let (snapshot, facts) = graph::build(&root).expect("build");
    let first = persist_scan(&root, snapshot, &facts).expect("persist");
    let reference = first.flow_refs.first().expect("flow ref").clone();
    assert!(!first.graph.flows[0].related_tests.is_empty());

    fs::write(
        root.join("src/unrelated.ts"),
        "export const unrelated = 'unrelated';\n",
    )
    .expect("unrelated source");
    let (snapshot, facts) = graph::build(&root).expect("unrelated build");
    let unrelated = persist_scan(&root, snapshot, &facts).expect("unrelated persist");
    let current = resolve_flow(&root, &reference.uri).expect("current flow");
    assert_eq!(current.status, "current");
    assert_eq!(current.freshness_reason, "flow-fingerprint-match");
    assert_eq!(
        current.origin_observation_id,
        reference.origin_observation_id
    );
    assert_ne!(unrelated.graph.observation_id, first.graph.observation_id);

    fs::write(
        root.join("tests/main.test.ts"),
        "export const unrelatedTest = true;\n",
    )
    .expect("related test change");
    let (snapshot, facts) = graph::build(&root).expect("test change build");
    let changed = persist_scan(&root, snapshot, &facts).expect("test change persist");
    let stale = resolve_flow(&root, &reference.uri).expect("related test stale");
    assert_eq!(stale.status, "stale");
    assert_eq!(stale.freshness_reason, "flow-fingerprint-changed");
    assert_eq!(stale.origin_observation_id, reference.origin_observation_id);
    assert_eq!(
        stale
            .current_basis
            .as_ref()
            .map(|basis| &basis.observation_id),
        Some(&changed.graph.observation_id)
    );
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn corrupted_flow_ref_metadata_rolls_back_the_entire_scan() {
    let root = flow_fixture_root();
    let (snapshot, facts) = graph::build(&root).expect("build");
    let first = persist_scan(&root, snapshot, &facts).expect("persist");
    let reference = first.flow_refs.first().expect("flow ref").clone();
    let connection = open(&root).expect("open");
    connection
        .execute(
            "UPDATE flow_refs SET origin_fingerprint = 'corrupted' WHERE uri = ?1",
            params![reference.uri],
        )
        .expect("corrupt flow ref");
    drop(connection);
    fs::write(
        root.join("package.json"),
        "{\n  \"scripts\": {\"start\": \"tsx src/main.ts\", \"unsupported\": \"tsx src/main.ts && echo credential-sentinel\"},\n  \"bin\": {\"checkout\": \"src/main.ts\"},\n  \"main\": \"src/main\",\n  \"module\": \"src/main.ts\"\n}\n",
    )
    .expect("format package");
    let (snapshot, facts) = graph::build(&root).expect("second build");
    assert!(persist_scan(&root, snapshot, &facts).is_err());
    let connection = open(&root).expect("reopen");
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM graph_observations", [], |row| row
                .get::<_, i64>(0))
            .expect("observations"),
        1
    );
    assert_eq!(
        status(&root).expect("status").current_graph_version,
        Some(first.graph.graph_version)
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT origin_fingerprint FROM flow_refs WHERE uri = ?1",
                params![reference.uri],
                |row| row.get::<_, String>(0),
            )
            .expect("flow fingerprint"),
        "corrupted"
    );
    drop(connection);
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn fresh_and_upgraded_v6_schema_match_and_migration_failure_rolls_back() {
    let fresh_root = fixture_root();
    let fresh = open(&fresh_root).expect("fresh schema");
    let fresh_schema = schema_snapshot(&fresh);
    assert_eq!(
        fresh
            .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .expect("fresh version"),
        CURRENT_USER_VERSION
    );
    drop(fresh);

    let upgraded_root = fixture_root();
    initialize_v5_database(&upgraded_root);
    let project_id = graph::project_id(&upgraded_root);
    let connection = rusqlite::Connection::open(database_path(&upgraded_root)).expect("v5");
    connection
        .execute(
            "INSERT INTO graph_versions(graph_version, graph_id, project_id, source_revision, created_at, truncated, omissions_json)
             VALUES(1, 'graph-v5', ?1, 'revision-v5', 1, 0, '[]')",
            params![project_id],
        )
        .expect("graph row");
    connection
        .execute(
            "INSERT INTO diagnostic_contexts(id, project_id, revision, payload_json, created_at)
             VALUES('context-v5', ?1, 1, '{\"contextSentinel\":\"keep\"}', 1)",
            params![project_id],
        )
        .expect("context row");
    connection
        .execute(
            "INSERT INTO diagnostic_assertions(id, context_id, revision, kind, status, actor, payload_json, created_at)
             VALUES('assertion-v5', 'context-v5', 1, 'observation', 'proposed', 'test', '{\"assertionSentinel\":\"keep\"}', 1)",
            [],
        )
        .expect("assertion row");
    drop(connection);
    let upgraded = open(&upgraded_root).expect("upgrade schema");
    assert_eq!(schema_snapshot(&upgraded), fresh_schema);
    assert_eq!(
        upgraded
            .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .expect("upgraded version"),
        CURRENT_USER_VERSION
    );
    let migrated_context = upgraded
        .query_row(
            "SELECT payload_json FROM diagnostic_contexts WHERE id='context-v5'",
            [],
            |row| row.get::<_, String>(0),
        )
        .expect("context payload");
    assert!(migrated_context.contains("contextSentinel"));
    assert!(migrated_context.contains(crate::model::DIAGNOSTIC_CONTEXT_SCHEMA));
    assert!(migrated_context.contains("focusFlowRefs"));
    assert_eq!(
        upgraded
            .query_row(
                "SELECT payload_json FROM diagnostic_assertions WHERE id='assertion-v5'",
                [],
                |row| row.get::<_, String>(0)
            )
            .expect("assertion payload"),
        "{\"assertionSentinel\":\"keep\"}"
    );
    drop(upgraded);
    fs::remove_dir_all(fresh_root).expect("cleanup fresh");
    fs::remove_dir_all(upgraded_root).expect("cleanup upgraded");

    let failed_root = fixture_root();
    initialize_v5_database(&failed_root);
    let connection = rusqlite::Connection::open(database_path(&failed_root)).expect("failed v5");
    connection
        .execute(
            "INSERT INTO diagnostic_contexts(id, project_id, revision, payload_json, created_at)
             VALUES('context-fail', ?1, 1, '{}', 1)",
            params![graph::project_id(&failed_root)],
        )
        .expect("failure context");
    connection
        .execute_batch(
            "CREATE TRIGGER fail_v6_context_update BEFORE UPDATE ON diagnostic_contexts
             BEGIN SELECT RAISE(ABORT, 'forced v6 migration failure'); END;",
        )
        .expect("failure trigger");
    drop(connection);
    assert!(open(&failed_root).is_err());
    let connection =
        rusqlite::Connection::open(database_path(&failed_root)).expect("reopen failed");
    assert_eq!(
        connection
            .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .expect("failed version"),
        5
    );
    let columns = table_columns_from_connection(&connection, "graph_observations");
    assert!(
        !columns
            .iter()
            .any(|column| column == "entry_manifest_status")
    );
    assert!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='flow_refs'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .expect("flow table check")
            == 0
    );
    drop(connection);
    fs::remove_dir_all(failed_root).expect("cleanup failed");
}

fn table_columns_from_connection(connection: &rusqlite::Connection, table: &str) -> Vec<String> {
    connection
        .prepare(&format!("PRAGMA table_info({table})"))
        .expect("table columns")
        .query_map([], |row| row.get::<_, String>(1))
        .expect("table column rows")
        .collect::<Result<Vec<_>, _>>()
        .expect("table column values")
}

#[test]
fn migrates_graph_and_historical_columns_without_losing_the_database() {
    let root = fixture_root();
    fs::create_dir_all(root.join(STORE_DIRECTORY)).expect("store directory");
    let connection = rusqlite::Connection::open(database_path(&root)).expect("old sqlite");
    connection
        .execute_batch(
            "CREATE TABLE graph_versions (
                graph_version INTEGER PRIMARY KEY NOT NULL,
                graph_id TEXT NOT NULL UNIQUE,
                project_id TEXT NOT NULL,
                source_revision TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                truncated INTEGER NOT NULL
            );
            CREATE TABLE historical_candidates (
                id TEXT PRIMARY KEY NOT NULL,
                project_id TEXT NOT NULL,
                graph_version INTEGER NOT NULL,
                payload_json TEXT NOT NULL,
                created_at INTEGER NOT NULL
            );",
        )
        .expect("old schema");
    drop(connection);
    let connection = open(&root).expect("migrate");
    let graph_columns = connection
        .prepare("PRAGMA table_info(graph_versions)")
        .expect("graph columns")
        .query_map([], |row| row.get::<_, String>(1))
        .expect("graph query")
        .collect::<Result<Vec<_>, _>>()
        .expect("graph names");
    let history_columns = connection
        .prepare("PRAGMA table_info(historical_candidates)")
        .expect("history columns")
        .query_map([], |row| row.get::<_, String>(1))
        .expect("history query")
        .collect::<Result<Vec<_>, _>>()
        .expect("history names");
    assert!(
        graph_columns
            .iter()
            .any(|column| column == "omissions_json")
    );
    assert!(history_columns.iter().any(|column| column == "context_id"));
    let observation_columns = connection
        .prepare("PRAGMA table_info(graph_observations)")
        .expect("observation columns")
        .query_map([], |row| row.get::<_, String>(1))
        .expect("observation query")
        .collect::<Result<Vec<_>, _>>()
        .expect("observation names");
    assert!(
        observation_columns
            .iter()
            .any(|column| column == "module_resolution_fingerprint")
    );
    assert_eq!(
        connection
            .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .expect("version"),
        CURRENT_USER_VERSION
    );
    drop(connection);
    fs::remove_dir_all(root).expect("cleanup");
}
