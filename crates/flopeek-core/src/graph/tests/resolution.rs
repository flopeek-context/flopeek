use super::*;

#[test]
fn resolves_default_namespace_and_reports_conservative_import_outcomes() {
    let root = temp_root("resolution-forms");
    fs::create_dir_all(root.join("src")).expect("mkdir");
    fs::write(
        root.join("src/payment.ts"),
        "export function charge() { return 1; }\nconst settle = () => 2;\nexport default settle;",
    )
    .expect("payment");
    fs::write(
        root.join("src/other-payment.ts"),
        "export function charge() { return 3; }",
    )
    .expect("other payment");
    fs::write(
        root.join("src/types.d.ts"),
        "export declare function declared(): void;",
    )
    .expect("declaration file");
    fs::write(
            root.join("src/entry.ts"),
            "import settle, * as payment from './payment';\nimport { missing } from './payment';\nimport { charge as localCharge } from './payment';\nimport { charge as duplicate } from './payment';\nimport { charge as duplicate } from './other-payment';\nimport type { charge as TypeCharge } from './payment';\nimport { charge as aliasCharge } from '@/payment';\nimport { declared } from './types';\nimport external, { thing } from 'external-package';\nsettle();\nexport function run(localCharge: unknown) {\n  settle();\n  payment.charge();\n  localCharge();\n  duplicate();\n  TypeCharge();\n  aliasCharge();\n  declared();\n  missing();\n  external();\n  thing[method]();\n  dynamic();\n}\n",
        )
        .expect("entry");
    fs::write(
        root.join("src/barrel.ts"),
        "export { charge } from './payment';\n",
    )
    .expect("barrel");
    fs::write(
        root.join("src/barrel-local.ts"),
        "import { charge } from './payment'; export { charge };",
    )
    .expect("local barrel");
    fs::write(
            root.join("src/reexport-consumer.ts"),
            "import { charge } from './barrel'; import { charge as forwarded } from './barrel-local'; export function run() { charge(); forwarded(); }",
        )
        .expect("reexport consumer");
    fs::write(
        root.join("src/namespace-barrel.ts"),
        "export * as payments from './payment';",
    )
    .expect("namespace barrel");
    fs::write(
        root.join("src/namespace-local-barrel.ts"),
        "import * as payments from './payment'; export { payments as forwarded };",
    )
    .expect("namespace local barrel");
    fs::write(
            root.join("src/namespace-consumer.ts"),
            "import { payments } from './namespace-barrel'; import { forwarded } from './namespace-local-barrel'; export function run() { payments.charge(); forwarded.charge(); }",
        )
        .expect("namespace consumer");

    let (graph, facts) = build(&root).expect("graph");
    let entry = facts
        .iter()
        .find(|fact| fact.path == "src/entry.ts")
        .expect("entry facts");
    assert!(
        entry
            .resolution_records
            .iter()
            .any(|record| record.status == "resolved" && record.reason == "default-import-binding")
    );
    assert!(entry.resolution_records.iter().any(|record| {
        record.status == "resolved" && record.reason == "namespace-import-binding"
    }));
    assert!(entry.resolution_records.iter().any(|record| {
        record.status == "unresolved" && record.reason == "local-binding-shadowed-import"
    }));
    assert!(
        entry
            .resolution_records
            .iter()
            .any(|record| record.status == "unresolved" && record.reason == "type-only-binding")
    );
    assert!(entry.resolution_records.iter().any(|record| {
        record.status == "unresolved" && record.reason == "non-relative-path-alias"
    }));
    assert!(
        entry.resolution_records.iter().any(|record| {
            record.status == "resolved" && record.reason == "named-import-binding"
        })
    );
    assert!(entry.resolution_records.iter().any(|record| {
        record.status == "ambiguous" && record.reason == "ambiguous-import-binding"
    }));
    let entry_file_id = graph
        .nodes
        .iter()
        .find(|node| node.kind == "file" && node.path.as_deref() == Some("src/entry.ts"))
        .expect("entry file")
        .id
        .clone();
    assert!(
        entry.resolution_records.iter().any(|record| {
            record.reference == "settle" && record.caller_node_id == entry_file_id
        })
    );
    assert!(
        entry
            .resolution_records
            .iter()
            .any(|record| record.status == "unresolved" && record.reason == "missing-export")
    );
    assert!(
        entry
            .resolution_records
            .iter()
            .any(|record| record.status == "external" && record.reason == "external-module")
    );
    assert!(
        entry
            .resolution_records
            .iter()
            .any(|record| record.status == "unresolved" && record.reason == "dynamic-callee")
    );
    let reexport = facts
        .iter()
        .find(|fact| fact.path == "src/reexport-consumer.ts")
        .expect("reexport facts");
    assert!(
        reexport
            .resolution_records
            .iter()
            .filter(|record| {
                record.status == "resolved" && record.reason == "named-import-through-reexport"
            })
            .count()
            >= 2
    );
    let namespace_consumer = facts
        .iter()
        .find(|fact| fact.path == "src/namespace-consumer.ts")
        .expect("namespace consumer facts");
    assert!(namespace_consumer.resolution_records.iter().any(|record| {
        record.status == "resolved" && record.reason == "namespace-member-binding"
    }));
    assert!(reexport.resolution_records.iter().any(|record| {
        record.status == "resolved" && record.reason == "named-import-through-reexport"
    }));
    assert_eq!(graph.resolution_evidence.status, "complete");
    assert!(
        graph
            .resolution_evidence
            .records
            .iter()
            .all(|record| !record.path.starts_with('/') && !record.path.contains('\\'))
    );
    assert!(
        graph
            .edges
            .iter()
            .any(|edge| { edge.kind == "imports-external" && edge.evidence == "default-import" })
    );
    assert!(graph.edges.iter().any(|edge| {
        edge.kind == "imports-unresolved" && edge.evidence == "non-relative-path-alias"
    }));
    assert!(!graph.edges.iter().any(|edge| edge.kind == "calls" && {
        graph
            .nodes
            .iter()
            .find(|node| node.id == edge.to)
            .is_some_and(|node| node.kind == "external-module")
    }));
    let encoded_graph = serde_json::to_string(&graph).expect("encode graph");
    assert!(!encoded_graph.contains("import settle"));
    assert!(!encoded_graph.contains("payment.charge()"));
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn resolution_evidence_is_bounded_and_reports_categorical_omission() {
    let records = (0..=MAX_RESOLUTION_RECORDS)
        .map(|index| SymbolResolution {
            path: "src/entry.ts".to_string(),
            caller_node_id: "node-caller".to_string(),
            reference: format!("call{index}"),
            form: "identifier".to_string(),
            status: "unresolved".to_string(),
            reason: "dynamic-callee".to_string(),
            candidate_node_ids: Vec::new(),
            occurrence_count: 1,
        })
        .collect();
    let facts = vec![TypeScriptFacts {
        schema_version: crate::model::TYPESCRIPT_FACTS_SCHEMA.to_string(),
        path: "src/entry.ts".to_string(),
        language: "ts".to_string(),
        source_hash: "hash".to_string(),
        parser: typescript::PARSER_IDENTITY.to_string(),
        parse_status: "parsed".to_string(),
        imports: Vec::new(),
        declarations: Vec::new(),
        exports: Vec::new(),
        calls: Vec::new(),
        unsupported: Vec::new(),
        resolution_records: records,
        canonical_fingerprint: "fingerprint".to_string(),
        heritage: Vec::new(),
    }];
    let evidence = resolution_evidence(&facts, false);
    assert_eq!(evidence.status, "truncated");
    assert_eq!(evidence.records.len(), MAX_RESOLUTION_RECORDS);
    assert!(evidence.truncated);
    assert!(
        evidence
            .omissions
            .iter()
            .any(|omission| omission.contains("resolution records capped"))
    );
}

#[test]
fn structural_graph_ignores_comments_and_whitespace_but_tracks_exact_source() {
    let root = temp_root("freshness");
    fs::create_dir_all(root.join("src")).expect("mkdir");
    fs::write(root.join("src/a.ts"), "export const a = 'value';\n").expect("write");
    let first = build(&root).expect("first graph").0;
    fs::write(
        root.join("src/a.ts"),
        "// comment\n\n export   const a = \"value\";\n",
    )
    .expect("format");
    let second = build(&root).expect("second graph").0;
    assert_eq!(first.graph_id, second.graph_id);
    assert_ne!(first.source_fingerprint, second.source_fingerprint);
    assert_eq!(first.nodes, second.nodes);
    fs::remove_dir_all(root).expect("cleanup");
}
