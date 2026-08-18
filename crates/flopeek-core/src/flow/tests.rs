use super::related_tests::derive_related_tests;
use super::*;
use crate::model::{GraphEdge, GraphNode, SourceFile};
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_root() -> PathBuf {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    std::env::temp_dir().join(format!("flopeek-flow-{suffix}"))
}

fn source(path: &str) -> SourceFile {
    SourceFile {
        path: path.to_string(),
        language: if path.ends_with(".tsx") {
            "tsx".to_string()
        } else {
            "typescript".to_string()
        },
        bytes: 1,
        hash: format!("hash:{path}"),
    }
}

fn file_node(path: &str) -> GraphNode {
    GraphNode {
        id: node_id("file", path, ""),
        kind: "file".to_string(),
        path: Some(path.to_string()),
        name: None,
        language: Some(if path.ends_with(".tsx") {
            "tsx".to_string()
        } else {
            "typescript".to_string()
        }),
        evidence_fingerprint: format!("file-fp:{path}"),
    }
}

fn symbol_node(path: &str, name: &str) -> GraphNode {
    GraphNode {
        id: node_id("function", path, &format!("function:{name}")),
        kind: "function".to_string(),
        path: Some(path.to_string()),
        name: Some(name.to_string()),
        language: Some("typescript".to_string()),
        evidence_fingerprint: format!("symbol-fp:{path}:{name}"),
    }
}

#[test]
fn package_entries_and_static_bfs_are_deterministic_without_command_body() {
    let root = temp_root();
    fs::create_dir_all(root.join("src")).expect("mkdir");
    fs::write(
        root.join("package.json"),
        r#"{"scripts":{"start":"tsx src/main"},"main":"src/main.ts"}"#,
    )
    .expect("manifest");
    let files = vec![SourceFile {
        path: "src/main.ts".to_string(),
        language: "typescript".to_string(),
        bytes: 1,
        hash: "source".to_string(),
    }];
    let file = node_id("file", "src/main.ts", "");
    let callee = node_id("symbol", "src/main.ts", "function:main");
    let nodes = vec![
        GraphNode {
            id: file.clone(),
            kind: "file".to_string(),
            path: Some("src/main.ts".to_string()),
            name: None,
            language: Some("typescript".to_string()),
            evidence_fingerprint: "file-fp".to_string(),
        },
        GraphNode {
            id: callee.clone(),
            kind: "function".to_string(),
            path: Some("src/main.ts".to_string()),
            name: Some("main".to_string()),
            language: Some("typescript".to_string()),
            evidence_fingerprint: "callee-fp".to_string(),
        },
    ];
    let edges = vec![GraphEdge {
        from: file.clone(),
        to: callee.clone(),
        kind: "calls".to_string(),
        evidence: "direct".to_string(),
    }];
    let first = derive(&root, "project_test", &files, &nodes, &edges).expect("derive");
    let second = derive(&root, "project_test", &files, &nodes, &edges).expect("derive again");
    assert_eq!(
        first.entry_evidence.effective_fingerprint,
        second.entry_evidence.effective_fingerprint
    );
    assert_eq!(first.flows, second.flows);
    assert!(
        first
            .entry_evidence
            .records
            .iter()
            .all(|record| record.reason != "script-command-body-stored")
    );
    assert!(first.flows.iter().all(|flow| {
        flow.traversed_edges
            .iter()
            .all(|edge| edge.kind == "calls" || edge.kind == "constructs")
    }));
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn unsupported_manifest_is_explicitly_unavailable() {
    let root = temp_root();
    fs::create_dir_all(&root).expect("mkdir");
    fs::write(root.join("package.json"), "{ invalid").expect("manifest");
    let result = derive(&root, "project_test", &[], &[], &[]).expect("derive");
    assert_eq!(result.entry_evidence.status, "unavailable");
    assert!(
        result
            .entry_evidence
            .omissions
            .iter()
            .any(|reason| reason == "package-json-invalid")
    );
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn all_entry_forms_and_abstentions_are_explicit_and_body_free() {
    let root = temp_root();
    fs::create_dir_all(root.join("src")).expect("mkdir");
    fs::write(
        root.join("package.json"),
        r#"{
                "scripts": {
                    "start": "tsx src/main",
                    "tsx": "ts-node src/tsx.tsx",
                    "node": "node src/node.ts",
                    "bun": "bun src/main.ts",
                    "bun-run": "bun run src/main.ts",
                    "deno": "deno run src/index",
                    "shell": "npm run start && echo credential-sentinel",
                    "flags": "tsx --loader src/main.ts",
                    "escape": "tsx ../outside.ts",
                    "javascript": "node dist/main.js",
                    "declaration": "tsx types/index.d.ts",
                    "missing": "tsx src/missing"
                },
                "bin": {
                    "checkout": "src/bin.tsx",
                    "bad": "src/nope.ts"
                },
                "main": "src/main",
                "module": "src/index"
            }"#,
    )
    .expect("manifest");
    let files = [
        source("src/main.ts"),
        source("src/tsx.tsx"),
        source("src/node.ts"),
        source("src/index.ts"),
        source("src/bin.tsx"),
    ];
    let nodes = files
        .iter()
        .map(|file| file_node(&file.path))
        .collect::<Vec<_>>();
    let evidence = derive(&root, "project_test", &files, &nodes, &[])
        .expect("derive")
        .entry_evidence;
    let record = |kind: &str, key: &str| {
        evidence
            .records
            .iter()
            .find(|record| record.kind == kind && record.key == key)
            .unwrap_or_else(|| panic!("missing entry {kind}:{key}"))
    };
    for key in ["start", "tsx", "node", "bun", "bun-run", "deno"] {
        assert_eq!(record("script", key).status, "resolved", "{key}");
    }
    assert_eq!(record("bin", "checkout").status, "resolved");
    assert_eq!(record("main", "main").status, "resolved");
    assert_eq!(record("module", "module").status, "resolved");
    assert_eq!(
        record("script", "shell").reason,
        "script-command-complex-or-shell-composed"
    );
    assert_eq!(
        record("script", "flags").reason,
        "script-command-flags-or-quoting-unsupported"
    );
    assert_eq!(
        record("script", "escape").reason,
        "entry-target-escapes-repository"
    );
    assert_eq!(
        record("script", "javascript").reason,
        "entry-target-javascript-output-unsupported"
    );
    assert_eq!(
        record("script", "declaration").reason,
        "entry-target-declaration-file-unsupported"
    );
    assert_eq!(
        record("script", "missing").reason,
        "entry-target-missing-or-not-typescript"
    );
    assert_eq!(
        record("bin", "bad").reason,
        "entry-target-missing-or-not-typescript"
    );
    let serialized = serde_json::to_string(&evidence).expect("evidence json");
    assert!(!serialized.contains("credential-sentinel"));
    assert!(!serialized.contains(root.to_string_lossy().as_ref()));
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn entry_and_flow_record_caps_are_bounded_and_reported() {
    let root = temp_root();
    fs::create_dir_all(&root).expect("mkdir");
    let mut scripts = serde_json::Map::new();
    for index in 0..=MAX_ENTRY_RECORDS {
        scripts.insert(
            format!("entry-{index}"),
            Value::String("tsx src/main.ts".to_string()),
        );
    }
    fs::write(
        root.join("package.json"),
        serde_json::to_vec(&serde_json::json!({"scripts": scripts})).expect("manifest json"),
    )
    .expect("manifest");
    let files = vec![source("src/main.ts")];
    let nodes = vec![file_node("src/main.ts")];
    let result = derive(&root, "project_test", &files, &nodes, &[]).expect("derive");
    assert_eq!(result.entry_evidence.records.len(), MAX_ENTRY_RECORDS);
    assert_eq!(result.flows.len(), MAX_FLOWS);
    assert!(result.entry_evidence.truncated);
    assert!(result.truncated);
    assert!(
        result
            .omissions
            .iter()
            .any(|reason| reason.contains("entry records capped"))
    );
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn missing_and_oversized_manifests_have_explicit_bounded_evidence() {
    let missing_root = temp_root();
    fs::create_dir_all(&missing_root).expect("missing root");
    let missing = derive(
        &missing_root,
        "project_test",
        &[source("src/main.ts")],
        &[file_node("src/main.ts")],
        &[],
    )
    .expect("missing manifest");
    assert_eq!(missing.entry_evidence.status, "complete");
    assert!(missing.entry_evidence.manifest.is_none());
    assert!(
        missing
            .entry_evidence
            .limitations
            .iter()
            .any(|limitation| limitation.contains("absent"))
    );
    fs::remove_dir_all(missing_root).expect("cleanup missing");

    let oversized_root = temp_root();
    fs::create_dir_all(&oversized_root).expect("oversized root");
    let padding = "x".repeat(MAX_MANIFEST_BYTES + 1);
    fs::write(
        oversized_root.join("package.json"),
        format!(r#"{{"padding":"{padding}"}}"#),
    )
    .expect("oversized manifest");
    let oversized = derive(
        &oversized_root,
        "project_test",
        &[source("src/main.ts")],
        &[file_node("src/main.ts")],
        &[],
    )
    .expect("oversized manifest evidence");
    assert_eq!(oversized.entry_evidence.status, "truncated");
    assert!(oversized.entry_evidence.truncated);
    assert!(oversized.truncated);
    assert!(
        oversized
            .omissions
            .iter()
            .any(|omission| omission.contains("package.json exceeds"))
    );
    assert!(
        !serde_json::to_string(&oversized.entry_evidence)
            .expect("oversized evidence json")
            .contains(&padding)
    );
    fs::remove_dir_all(oversized_root).expect("cleanup oversized");
}

#[test]
fn flow_caps_and_cycles_are_deterministic_with_categorical_omissions() {
    let root = temp_root();
    fs::create_dir_all(&root).expect("mkdir");
    fs::write(
        root.join("package.json"),
        r#"{"scripts":{"start":"tsx src/main.ts"}}"#,
    )
    .expect("manifest");
    let main = file_node("src/main.ts");

    let mut chain_files = vec![source("src/main.ts")];
    let mut chain_nodes = vec![main.clone()];
    let mut chain_edges = Vec::new();
    let mut previous = main.id.clone();
    for index in 0..70 {
        let path = format!("src/chain-{index}.ts");
        let node = symbol_node(&path, &format!("chain{index}"));
        chain_files.push(source(&path));
        chain_edges.push(GraphEdge {
            from: previous,
            to: node.id.clone(),
            kind: "calls".to_string(),
            evidence: "direct-call".to_string(),
        });
        previous = node.id.clone();
        chain_nodes.push(node);
    }
    let chain = derive(
        &root,
        "project_test",
        &chain_files,
        &chain_nodes,
        &chain_edges,
    )
    .expect("chain");
    let chain_flow = chain.flows.first().expect("chain flow");
    assert!(chain_flow.truncated);
    assert!(
        chain_flow
            .omissions
            .iter()
            .any(|reason| reason.contains("depth capped"))
    );

    let mut fan_files = vec![source("src/main.ts")];
    let mut fan_nodes = vec![main.clone()];
    let mut fan_edges = Vec::new();
    for index in 0..300 {
        let path = format!("src/fan-{index}.ts");
        let node = symbol_node(&path, &format!("fan{index}"));
        fan_files.push(source(&path));
        fan_edges.push(GraphEdge {
            from: main.id.clone(),
            to: node.id.clone(),
            kind: "calls".to_string(),
            evidence: "direct-call".to_string(),
        });
        fan_nodes.push(node);
    }
    let fan = derive(&root, "project_test", &fan_files, &fan_nodes, &fan_edges).expect("fan");
    let fan_flow = fan.flows.first().expect("fan flow");
    assert!(fan_flow.truncated);
    assert!(
        fan_flow
            .omissions
            .iter()
            .any(|reason| reason.contains("steps capped"))
    );

    let mut edge_files = vec![source("src/main.ts")];
    let mut edge_nodes = vec![main.clone()];
    let mut edge_edges = Vec::new();
    for index in 0..600 {
        let path = format!("src/edge-{index}.ts");
        let node = symbol_node(&path, &format!("edge{index}"));
        edge_files.push(source(&path));
        edge_edges.push(GraphEdge {
            from: main.id.clone(),
            to: node.id.clone(),
            kind: "calls".to_string(),
            evidence: "direct-call".to_string(),
        });
        edge_nodes.push(node);
    }
    let edge = derive(&root, "project_test", &edge_files, &edge_nodes, &edge_edges).expect("edge");
    let edge_flow = edge.flows.first().expect("edge flow");
    assert!(edge_flow.truncated);
    assert!(
        edge_flow
            .omissions
            .iter()
            .any(|reason| reason.contains("edges capped"))
    );

    let cycle_a = symbol_node("src/cycle-a.ts", "cycleA");
    let cycle_b = symbol_node("src/cycle-b.ts", "cycleB");
    let cycle_edges = vec![
        GraphEdge {
            from: main.id.clone(),
            to: cycle_a.id.clone(),
            kind: "calls".to_string(),
            evidence: "direct-call".to_string(),
        },
        GraphEdge {
            from: cycle_a.id.clone(),
            to: cycle_b.id.clone(),
            kind: "calls".to_string(),
            evidence: "direct-call".to_string(),
        },
        GraphEdge {
            from: cycle_b.id.clone(),
            to: cycle_a.id.clone(),
            kind: "calls".to_string(),
            evidence: "direct-call".to_string(),
        },
    ];
    let cycle_nodes = vec![main.clone(), cycle_a, cycle_b];
    let cycle_files = vec![
        source("src/main.ts"),
        source("src/cycle-a.ts"),
        source("src/cycle-b.ts"),
    ];
    let first = derive(
        &root,
        "project_test",
        &cycle_files,
        &cycle_nodes,
        &cycle_edges,
    )
    .expect("cycle");
    let second = derive(
        &root,
        "project_test",
        &cycle_files,
        &cycle_nodes,
        &cycle_edges,
    )
    .expect("cycle again");
    assert_eq!(first.flows, second.flows);
    let ids = first.flows[0]
        .steps
        .iter()
        .map(|step| step.node_id.clone())
        .collect::<BTreeSet<_>>();
    assert_eq!(ids.len(), first.flows[0].steps.len());
    assert!(
        first.flows[0]
            .traversed_edges
            .iter()
            .all(|edge| matches!(edge.kind.as_str(), "calls" | "constructs"))
    );
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn related_tests_keep_only_proven_relations_and_strengths() {
    let nodes = vec![
        GraphNode {
            id: "test-calls".to_string(),
            kind: "function".to_string(),
            path: Some("tests/calls.test.ts".to_string()),
            name: Some("calls".to_string()),
            language: Some("typescript".to_string()),
            evidence_fingerprint: "test-calls".to_string(),
        },
        GraphNode {
            id: "test-import".to_string(),
            kind: "file".to_string(),
            path: Some("tests/import.spec.ts".to_string()),
            name: None,
            language: Some("typescript".to_string()),
            evidence_fingerprint: "test-import".to_string(),
        },
        GraphNode {
            id: "test-type".to_string(),
            kind: "file".to_string(),
            path: Some("tests/type.test.ts".to_string()),
            name: None,
            language: Some("typescript".to_string()),
            evidence_fingerprint: "test-type".to_string(),
        },
        GraphNode {
            id: "test-to-test".to_string(),
            kind: "file".to_string(),
            path: Some("tests/other.test.ts".to_string()),
            name: None,
            language: Some("typescript".to_string()),
            evidence_fingerprint: "test-to-test".to_string(),
        },
        GraphNode {
            id: "prod".to_string(),
            kind: "function".to_string(),
            path: Some("src/payment.ts".to_string()),
            name: Some("charge".to_string()),
            language: Some("typescript".to_string()),
            evidence_fingerprint: "prod".to_string(),
        },
        GraphNode {
            id: "prod-2".to_string(),
            kind: "function".to_string(),
            path: Some("src/checkout.ts".to_string()),
            name: Some("checkout".to_string()),
            language: Some("typescript".to_string()),
            evidence_fingerprint: "prod-2".to_string(),
        },
    ];
    let edges = vec![
        GraphEdge {
            from: "test-calls".to_string(),
            to: "prod".to_string(),
            kind: "calls".to_string(),
            evidence: "direct-call".to_string(),
        },
        GraphEdge {
            from: "test-import".to_string(),
            to: "prod".to_string(),
            kind: "imports".to_string(),
            evidence: "named-import".to_string(),
        },
        GraphEdge {
            from: "test-type".to_string(),
            to: "prod".to_string(),
            kind: "imports".to_string(),
            evidence: "type-only-import".to_string(),
        },
        GraphEdge {
            from: "test-to-test".to_string(),
            to: "test-calls".to_string(),
            kind: "imports".to_string(),
            evidence: "named-import".to_string(),
        },
        GraphEdge {
            from: "test-calls".to_string(),
            to: "prod-2".to_string(),
            kind: "constructs".to_string(),
            evidence: "direct-construct".to_string(),
        },
    ];
    let related = derive_related_tests(&nodes, &edges);
    assert_eq!(related.status, "complete");
    assert_eq!(related.records.len(), 3);
    assert!(
        related
            .records
            .iter()
            .any(|record| { record.relation == "direct-call" && record.strength == "strong" })
    );
    assert!(
        related
            .records
            .iter()
            .any(|record| { record.relation == "direct-construct" && record.strength == "strong" })
    );
    assert!(
        related
            .records
            .iter()
            .any(|record| { record.relation == "direct-import" && record.strength == "weak" })
    );
    assert!(
        related
            .records
            .iter()
            .all(|record| record.test_path != "tests/type.test.ts")
    );
    assert!(
        related
            .records
            .iter()
            .all(|record| record.test_path != "tests/other.test.ts")
    );
}
