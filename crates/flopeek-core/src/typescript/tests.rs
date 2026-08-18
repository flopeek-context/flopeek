//! TypeScript parser characterization tests.

use super::*;

#[test]
fn extracts_direct_import_bindings_and_call_owner() {
    let facts = parse(
        "src/checkout.ts",
        b"import { charge as debit, type Card } from './payments';\nimport payment, * as ns from './payment';\nexport function checkout() { return debit(); }\n",
        "hash-ts",
    )
    .expect("parse TypeScript");
    assert_eq!(facts.schema_version, TYPESCRIPT_FACTS_SCHEMA);
    assert_eq!(facts.parser, PARSER_IDENTITY);
    assert!(
        facts
            .imports
            .iter()
            .any(|item| item.local_name.as_deref() == Some("debit")
                && item.imported_name.as_deref() == Some("charge"))
    );
    assert!(facts.imports.iter().any(
        |item| item.kind == "namespace-import" && item.local_name.as_deref() == Some("ns")
    ));
    assert!(
        facts
            .imports
            .iter()
            .any(|item| item.type_only && item.local_name.as_deref() == Some("Card"))
    );
    assert_eq!(facts.calls[0].callee.as_deref(), Some("debit"));
    assert_eq!(facts.calls[0].caller.as_deref(), Some("function:checkout"));
    assert_eq!(facts.calls[0].callee_form, "identifier");
}

#[test]
fn extracts_class_members_heritage_and_method_call_owners() {
    let facts = parse(
        "src/payment.ts",
        br#"export interface Gateway extends BaseGateway { charge(): void; }
export abstract class BaseGateway { abstract charge(): void; }
export class Payment extends BaseGateway implements Gateway {
  private secret() { return 1; }
  #hidden() { return 2; }
  public visible() { this.secret(); this.#hidden(); this.visible(); }
  static create() { return new Payment(); }
  constructor() {}
  overloaded(value: string): void;
  overloaded(value: number): void;
  overloaded(value: unknown) {}
}
"#,
        "hash-classes",
    )
    .expect("parse classes");

    assert!(facts.declarations.iter().any(|declaration| {
        declaration.qualified_name == "method:Payment.secret"
            && declaration.visibility == "private"
            && declaration.owner.as_deref() == Some("class:Payment")
    }));
    assert!(facts.declarations.iter().any(|declaration| {
        declaration.qualified_name == "method:Payment.#hidden"
            && declaration.visibility == "private"
    }));
    assert!(facts.declarations.iter().any(|declaration| {
        declaration.qualified_name == "static_method:Payment.create" && declaration.static_member
    }));
    assert!(facts.declarations.iter().any(|declaration| {
        declaration.qualified_name == "constructor:Payment.constructor"
            && !declaration.declaration_only
    }));
    assert_eq!(
        facts
            .declarations
            .iter()
            .filter(|declaration| declaration.qualified_name == "method:Payment.overloaded")
            .count(),
        3
    );
    assert!(facts.declarations.iter().any(|declaration| {
        declaration.qualified_name == "method_signature:Gateway.charge"
            && declaration.owner.as_deref() == Some("interface:Gateway")
    }));
    assert!(facts.heritage.iter().any(|item| {
        item.owner == "class:Payment"
            && item.relation == "extends"
            && item.reference == "BaseGateway"
    }));
    assert!(facts.heritage.iter().any(|item| {
        item.owner == "class:Payment"
            && item.relation == "implements"
            && item.reference == "Gateway"
    }));
    assert!(facts.heritage.iter().any(|item| {
        item.owner == "interface:Gateway"
            && item.relation == "extends"
            && item.reference == "BaseGateway"
    }));
    assert!(facts.calls.iter().any(|call| {
        call.callee.as_deref() == Some("this.secret")
            && call.callee_form == "this-member"
            && call.caller.as_deref() == Some("method:Payment.visible")
            && call.enclosing_type.as_deref() == Some("Payment")
    }));
    assert!(facts.calls.iter().any(|call| {
        call.callee.as_deref() == Some("Payment")
            && call.callee_form == "constructor"
            && call.caller.as_deref() == Some("static_method:Payment.create")
    }));
}

#[test]
fn extracts_all_direct_import_and_export_forms_without_source_body() {
    let facts = parse(
        "src/entry.ts",
        br#"import defaultValue, { charge as debit, type Card } from './payment';
import * as payments from './payments';
import './side-effect';
import type { Receipt } from './types';
export { debit as charge };
 export { charge as reexported } from './payment';
 export * from './payments';
 export * as paymentNamespace from './payment';
 export default function () { return defaultValue(); }
"#,
        "hash-imports",
    )
    .expect("parse import forms");

    assert!(facts.imports.iter().any(|item| {
        item.kind == "default-import"
            && item.local_name.as_deref() == Some("defaultValue")
            && item.imported_name.as_deref() == Some("default")
    }));
    assert!(facts.imports.iter().any(|item| {
        item.kind == "named-import"
            && item.local_name.as_deref() == Some("debit")
            && item.imported_name.as_deref() == Some("charge")
    }));
    assert!(facts.imports.iter().any(|item| {
        item.kind == "namespace-import" && item.local_name.as_deref() == Some("payments")
    }));
    assert!(
        facts
            .imports
            .iter()
            .any(|item| item.kind == "side-effect-import" && item.specifier == "./side-effect")
    );
    assert!(
        facts
            .imports
            .iter()
            .any(|item| { item.type_only && item.local_name.as_deref() == Some("Receipt") })
    );
    assert!(facts.exports.iter().any(|item| {
        item.kind == "local-export"
            && item.exported_name == "charge"
            && item.local_name.as_deref() == Some("debit")
    }));
    assert!(facts.exports.iter().any(|item| {
        item.kind == "re-export"
            && item.exported_name == "reexported"
            && item.source.as_deref() == Some("./payment")
    }));
    assert!(facts.exports.iter().any(|item| {
        item.kind == "re-export" && item.exported_name == "*" && item.source.is_some()
    }));
    assert!(facts.exports.iter().any(|item| {
        item.kind == "namespace-re-export"
            && item.exported_name == "paymentNamespace"
            && item.source.as_deref() == Some("./payment")
    }));
    assert!(
        facts
            .declarations
            .iter()
            .any(|item| item.qualified_name == "function:default")
    );
    let encoded = serde_json::to_string(&facts).expect("serialize facts");
    assert!(!encoded.contains("defaultValue();"));
}

#[test]
fn computed_and_dynamic_calls_are_not_reduced_to_direct_members() {
    let facts = parse(
        "src/entry.ts",
        b"declare const ns: { charge(): void }; ns.charge(); ns['charge'](); ns[method](); call?.();",
        "hash-calls",
    )
    .expect("parse calls");
    assert!(facts.calls.iter().any(
        |call| call.callee.as_deref() == Some("ns.charge") && call.callee_form == "member"
    ));
    assert!(facts.calls.iter().any(|call| call.callee_form == "dynamic"));
    assert!(
        facts
            .calls
            .iter()
            .filter(|call| call.callee_form == "member")
            .all(|call| call.callee.as_deref() == Some("ns.charge"))
    );
}

#[test]
fn extracts_default_and_local_exports_without_source_body() {
    let facts = parse(
        "src/payment.ts",
        b"export function charge() {}\nconst retry = () => charge();\nexport { retry as retryPayment };\nexport default charge;\n",
        "hash-ts",
    )
    .expect("parse TypeScript");
    assert!(
        facts
            .exports
            .iter()
            .any(|item| item.exported_name == "retryPayment"
                && item.local_name.as_deref() == Some("retry"))
    );
    assert!(facts.exports.iter().any(
        |item| item.exported_name == "default" && item.local_name.as_deref() == Some("charge")
    ));
    let encoded = serde_json::to_string(&facts).expect("serialize facts");
    assert!(!encoded.contains("charge();"));
}

#[test]
fn extracts_anonymous_default_and_tsx() {
    let default = parse(
        "src/payment.ts",
        b"export default function () { return 1; }\n",
        "hash-default",
    )
    .expect("parse default");
    assert!(
        default
            .declarations
            .iter()
            .any(|item| item.name == "default")
    );
    assert!(
        default
            .exports
            .iter()
            .any(|item| item.exported_name == "default")
    );

    let default_arrow = parse("src/arrow.ts", b"export default () => 1;\n", "hash-arrow")
        .expect("parse default arrow");
    assert!(
        default_arrow
            .declarations
            .iter()
            .any(|item| item.qualified_name == "function:default")
    );

    let tsx = parse(
        "src/Checkout.tsx",
        b"export function Checkout() { return <button />; }\n",
        "hash-tsx",
    )
    .expect("parse TSX");
    assert_eq!(tsx.language, "tsx");
    assert!(tsx.declarations.iter().any(|item| item.name == "Checkout"));
}

#[test]
fn rejects_javascript_and_non_typescript_inputs() {
    assert!(parse("src/legacy.js", b"export const x = 1;", "hash").is_err());
    assert!(parse("src/service.py", b"def service(): pass", "hash").is_err());
}
