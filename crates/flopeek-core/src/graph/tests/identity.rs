use super::*;

#[test]
fn resolves_relative_modules_in_deterministic_order() {
    let paths = [
        "src/a.ts",
        "src/lib.tsx",
        "shared.ts",
        "src/index.d.ts",
        "src/types.d.ts",
        "src/nested/index.ts",
    ]
    .into_iter()
    .map(str::to_string)
    .collect();
    assert_eq!(
        resolve_relative("src/a.ts", "./lib", &paths),
        Some("src/lib.tsx".to_string())
    );
    assert_eq!(
        resolve_relative("src/a.ts", "../shared", &paths),
        Some("shared.ts".to_string())
    );
    assert_eq!(
        resolve_relative("src/a.ts", "./types", &paths),
        Some("src/types.d.ts".to_string())
    );
    assert_eq!(
        resolve_relative("src/a.ts", "./nested", &paths),
        Some("src/nested/index.ts".to_string())
    );
    assert_eq!(resolve_relative("src/a.ts", "../../outside", &paths), None);
    assert_eq!(resolve_relative("src/a.ts", "react", &paths), None);
}

#[test]
fn graph_identity_is_reproducible() {
    let root = temp_root("graph");
    fs::create_dir_all(root.join("src")).expect("mkdir");
    fs::write(root.join("src/a.ts"), "export const a = 1;").expect("write");
    let (first, _) = build(&root).expect("first graph");
    let (second, _) = build(&root).expect("second graph");
    assert_eq!(first.graph_id, second.graph_id);
    assert_eq!(first.nodes, second.nodes);
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn import_binding_resolves_caller_to_exported_symbol() {
    let root = temp_root("imports");
    fs::create_dir_all(root.join("src")).expect("mkdir");
    fs::write(
        root.join("src/payment.ts"),
        "export function charge() { return 1; }",
    )
    .expect("payment");
    fs::write(
            root.join("src/checkout.ts"),
            "import { charge as debit } from './payment'; export function checkout() { return debit(); }",
        )
        .expect("checkout");
    let (graph, facts) = build(&root).expect("graph");
    let checkout = graph
        .nodes
        .iter()
        .find(|node| {
            node.path.as_deref() == Some("src/checkout.ts")
                && node.name.as_deref() == Some("checkout")
        })
        .expect("checkout symbol");
    let payment = graph
        .nodes
        .iter()
        .find(|node| {
            node.path.as_deref() == Some("src/payment.ts") && node.name.as_deref() == Some("charge")
        })
        .expect("charge symbol");
    assert!(
        graph.edges.iter().any(|edge| {
            edge.from == checkout.id && edge.to == payment.id && edge.kind == "calls"
        })
    );
    assert!(
        facts
            .iter()
            .find(|fact| fact.path == "src/checkout.ts")
            .is_some_and(|fact| {
                fact.resolution_records.iter().any(|record| {
                    record.status == "resolved" && record.reason == "named-import-binding"
                })
            })
    );
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn class_semantics_resolve_private_static_constructor_and_heritage_edges() {
    let root = temp_root("class-semantics");
    fs::create_dir_all(root.join("src")).expect("mkdir");
    fs::write(
        root.join("src/payment.ts"),
        r#"export interface Gateway { charge(): void; }
export class Base { protected base() {} }
export class Payment extends Base implements Gateway {
  private secret() { return 1; }
  charge() {}
  visible() { this.secret(); this.visible(); }
  static create() { return new Payment(); }
  constructor() {}
}
"#,
    )
    .expect("payment");
    fs::write(
            root.join("src/entry.ts"),
            "import { Payment as Alias } from './payment'; export function run() { new Alias(); Alias.create(); Alias.visible(); }",
        )
        .expect("entry");

    let (graph, facts) = build(&root).expect("graph");
    let payment_class = graph
        .nodes
        .iter()
        .find(|node| {
            node.path.as_deref() == Some("src/payment.ts")
                && node.kind == "class"
                && node.name.as_deref() == Some("Payment")
        })
        .expect("payment class");
    let base_class = graph
        .nodes
        .iter()
        .find(|node| {
            node.path.as_deref() == Some("src/payment.ts")
                && node.kind == "class"
                && node.name.as_deref() == Some("Base")
        })
        .expect("base class");
    let gateway = graph
        .nodes
        .iter()
        .find(|node| {
            node.path.as_deref() == Some("src/payment.ts")
                && node.kind == "interface"
                && node.name.as_deref() == Some("Gateway")
        })
        .expect("gateway interface");
    let visible = graph
        .nodes
        .iter()
        .find(|node| {
            node.path.as_deref() == Some("src/payment.ts")
                && node.name.as_deref() == Some("method:Payment.visible")
        })
        .expect("visible method");
    let secret = graph
        .nodes
        .iter()
        .find(|node| {
            node.path.as_deref() == Some("src/payment.ts")
                && node.name.as_deref() == Some("method:Payment.secret")
        })
        .expect("secret method");
    let create = graph
        .nodes
        .iter()
        .find(|node| {
            node.path.as_deref() == Some("src/payment.ts")
                && node.name.as_deref() == Some("static_method:Payment.create")
        })
        .expect("create method");
    let constructor = graph
        .nodes
        .iter()
        .find(|node| {
            node.path.as_deref() == Some("src/payment.ts")
                && node.name.as_deref() == Some("constructor:Payment.constructor")
        })
        .expect("constructor");
    let run = graph
        .nodes
        .iter()
        .find(|node| {
            node.path.as_deref() == Some("src/entry.ts") && node.name.as_deref() == Some("run")
        })
        .expect("run");

    assert!(graph.edges.iter().any(|edge| {
        edge.from == payment_class.id && edge.to == base_class.id && edge.kind == "extends"
    }));
    assert!(graph.edges.iter().any(|edge| {
        edge.from == payment_class.id && edge.to == gateway.id && edge.kind == "implements"
    }));
    assert!(
        graph.edges.iter().any(|edge| {
            edge.from == visible.id && edge.to == secret.id && edge.kind == "calls"
        })
    );
    assert!(graph.edges.iter().any(|edge| {
        edge.from == create.id && edge.to == constructor.id && edge.kind == "constructs"
    }));
    assert!(graph.edges.iter().any(|edge| {
        edge.from == run.id && edge.to == constructor.id && edge.kind == "constructs"
    }));
    assert!(graph.edges.iter().any(|edge| {
        edge.from == run.id
            && edge.to == create.id
            && edge.kind == "calls"
            && edge.evidence == "static-class-method"
    }));

    let entry = facts
        .iter()
        .find(|fact| fact.path == "src/entry.ts")
        .expect("entry facts");
    assert!(entry.resolution_records.iter().any(|record| {
        record.reference == "Alias.visible"
            && record.status == "unresolved"
            && record.reason == "static-member-not-found"
    }));
    let payment = facts
        .iter()
        .find(|fact| fact.path == "src/payment.ts")
        .expect("payment facts");
    assert!(payment.resolution_records.iter().any(|record| {
        record.reference == "this.visible"
            && record.status == "unresolved"
            && record.reason == "potentially-polymorphic-this-call"
    }));
    assert!(payment.resolution_records.iter().any(|record| {
        record.reference == "Base"
            && record.form == "heritage-extends-identifier"
            && record.status == "resolved"
    }));
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn class_imports_follow_default_named_barrel_and_tsconfig_resolution() {
    let root = temp_root("class-import-resolution");
    fs::create_dir_all(root.join("src")).expect("mkdir");
    fs::write(
        root.join("tsconfig.json"),
        r#"{
              "compilerOptions": { "baseUrl": ".", "paths": { "@lib/*": ["src/*"] } }
            }"#,
    )
    .expect("config");
    fs::write(
        root.join("src/base.ts"),
        "export class Payment { static create() {} constructor() {} }",
    )
    .expect("base");
    fs::write(
        root.join("src/barrel.ts"),
        "export { Payment as default, Payment as NamedPayment } from './base';",
    )
    .expect("barrel");
    fs::write(
            root.join("src/entry.ts"),
            "import DefaultPayment, { NamedPayment as Alias } from '@lib/barrel'; export function run() { new DefaultPayment(); Alias.create(); new Alias(); }",
        )
        .expect("entry");

    let (graph, facts) = build(&root).expect("graph");
    let entry = facts
        .iter()
        .find(|fact| fact.path == "src/entry.ts")
        .expect("entry facts");
    assert_eq!(graph.module_resolution.status, "complete");
    assert!(entry.resolution_records.iter().any(|record| {
        record.reference == "DefaultPayment"
            && record.form == "constructor"
            && record.status == "resolved"
            && record.reason == "explicit-constructor"
    }));
    assert!(entry.resolution_records.iter().any(|record| {
        record.reference == "Alias.create"
            && record.status == "resolved"
            && record.reason == "static-class-method"
    }));
    assert_eq!(
        entry
            .resolution_records
            .iter()
            .filter(|record| record.reference == "Alias" && record.form == "constructor")
            .count(),
        1
    );
    assert!(!graph.edges.iter().any(|edge| {
        edge.kind == "calls"
            && graph
                .nodes
                .iter()
                .find(|node| node.id == edge.to)
                .is_some_and(|node| node.kind == "external-module")
    }));
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn tsconfig_paths_resolve_alias_imports_without_global_name_fallback() {
    let root = temp_root("tsconfig-paths");
    fs::create_dir_all(root.join("src/payments")).expect("mkdir");
    fs::write(
        root.join("tsconfig.json"),
        r#"{
              // JSONC config is supported.
              "compilerOptions": {
                "baseUrl": ".",
                "paths": { "@payments/*": ["src/payments/*"] },
              },
            }"#,
    )
    .expect("config");
    fs::write(
        root.join("src/payments/charge.ts"),
        "export function charge() { return 1; }",
    )
    .expect("charge");
    fs::write(
        root.join("src/checkout.ts"),
        "import { charge } from '@payments/charge'; export function checkout() { charge(); }",
    )
    .expect("checkout");
    let (graph, facts) = build(&root).expect("graph");
    assert_eq!(graph.module_resolution.status, "complete");
    assert_eq!(
        graph.module_resolution.root_config.as_deref(),
        Some("tsconfig.json")
    );
    assert!(
        graph
            .module_resolution
            .config_files
            .iter()
            .all(|file| !file.path.contains('\\') && !file.path.starts_with('/'))
    );
    let checkout = graph
        .nodes
        .iter()
        .find(|node| {
            node.path.as_deref() == Some("src/checkout.ts")
                && node.name.as_deref() == Some("checkout")
        })
        .expect("checkout node");
    let charge = graph
        .nodes
        .iter()
        .find(|node| {
            node.path.as_deref() == Some("src/payments/charge.ts")
                && node.name.as_deref() == Some("charge")
        })
        .expect("charge node");
    assert!(
        graph.edges.iter().any(|edge| {
            edge.from == checkout.id && edge.to == charge.id && edge.kind == "calls"
        })
    );
    assert!(
        graph
            .edges
            .iter()
            .any(|edge| { edge.kind == "imports" && edge.evidence == "tsconfig-path-alias" })
    );
    assert!(
        facts
            .iter()
            .find(|fact| fact.path == "src/checkout.ts")
            .is_some_and(|fact| {
                fact.resolution_records.iter().any(|record| {
                    record.status == "resolved" && record.reason == "named-import-binding"
                })
            })
    );
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn reexport_cycles_and_star_collisions_are_explicitly_ambiguous() {
    let root = temp_root("reexport-ambiguity");
    fs::create_dir_all(root.join("src")).expect("mkdir");
    fs::write(root.join("src/one.ts"), "export function charge() {}").expect("one");
    fs::write(root.join("src/two.ts"), "export function charge() {}").expect("two");
    fs::write(
        root.join("src/ambiguous.ts"),
        "export * from './one'; export * from './two';",
    )
    .expect("ambiguous barrel");
    fs::write(
        root.join("src/consumer.ts"),
        "import { charge } from './ambiguous'; export function run() { charge(); }",
    )
    .expect("consumer");
    fs::write(
        root.join("src/cycle-a.ts"),
        "export { charge } from './cycle-b';",
    )
    .expect("cycle a");
    fs::write(
        root.join("src/cycle-b.ts"),
        "export { charge } from './cycle-a';",
    )
    .expect("cycle b");
    fs::write(
        root.join("src/cycle-consumer.ts"),
        "import { charge } from './cycle-a'; export function run() { charge(); }",
    )
    .expect("cycle consumer");
    let (graph, facts) = build(&root).expect("graph");
    let consumer = facts
        .iter()
        .find(|fact| fact.path == "src/consumer.ts")
        .expect("consumer facts");
    assert!(consumer.resolution_records.iter().any(|record| {
        record.status == "ambiguous" && record.reason == "ambiguous-star-reexport"
    }));
    let cycle_consumer = facts
        .iter()
        .find(|fact| fact.path == "src/cycle-consumer.ts")
        .expect("cycle consumer facts");
    assert!(
        cycle_consumer
            .resolution_records
            .iter()
            .any(|record| { record.status == "unresolved" && record.reason == "re-export-cycle" })
    );
    assert!(!graph.edges.iter().any(|edge| edge.kind == "calls" && {
        graph
            .nodes
            .iter()
            .any(|node| node.id == edge.from && node.path.as_deref() == Some("src/consumer.ts"))
    }));
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn reexport_chain_depth_is_bounded_without_guessing() {
    let root = temp_root("reexport-depth");
    fs::create_dir_all(root.join("src")).expect("mkdir");
    fs::write(root.join("src/leaf.ts"), "export function charge() {}").expect("leaf");
    let mut next = "leaf".to_string();
    for index in (0..MAX_REEXPORT_DEPTH + 2).rev() {
        let current = format!("barrel{index}");
        fs::write(
            root.join(format!("src/{current}.ts")),
            format!("export {{ charge }} from './{next}';"),
        )
        .expect("barrel");
        next = current;
    }
    fs::write(
        root.join("src/consumer.ts"),
        format!("import {{ charge }} from './{next}'; export function run() {{ charge(); }}"),
    )
    .expect("consumer");
    let (_, facts) = build(&root).expect("graph");
    let consumer = facts
        .iter()
        .find(|fact| fact.path == "src/consumer.ts")
        .expect("consumer facts");
    assert!(consumer.resolution_records.iter().any(|record| {
        record.status == "unresolved" && record.reason == "re-export-depth-capped"
    }));
    fs::remove_dir_all(root).expect("cleanup");
}
