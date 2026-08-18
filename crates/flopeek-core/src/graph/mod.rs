//! Deterministic graph assembly for TypeScript/TSX evidence.

use crate::discovery::{discover, read_source};

use crate::model::{
    GraphEdge, GraphNode, GraphSnapshot, PRODUCT_IDENTITY, ResolutionEvidence, SourceFile,
    SymbolResolution, TYPESCRIPT_RESOLUTION_SCHEMA, TypeScriptFacts,
};
use crate::typescript;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path};
use std::process::Command;

pub const MAX_SOURCE_FILES: usize = 10_000;
pub const MAX_GRAPH_NODES: usize = 50_000;
pub const MAX_GRAPH_EDGES: usize = 100_000;
pub const MAX_RESOLUTION_RECORDS: usize = 100_000;
pub const GRAPH_DERIVATION_ID: &str = "typescript-structural-evidence-v3";

type ModuleExports = BTreeMap<String, BTreeMap<String, Vec<String>>>;
type DeferredExports = BTreeSet<(String, String)>;

pub fn build(root: &Path) -> Result<(GraphSnapshot, Vec<TypeScriptFacts>), String> {
    let mut files = discover(root)?;
    let mut truncated = false;
    let mut omissions = Vec::new();
    if files.len() > MAX_SOURCE_FILES {
        files.truncate(MAX_SOURCE_FILES);
        truncated = true;
        omissions.push(format!("source files capped at {MAX_SOURCE_FILES}"));
    }
    let mut facts = Vec::with_capacity(files.len());
    for file in &files {
        let source = read_source(root, &file.path)?;
        facts.push(typescript::parse(&file.path, &source, &file.hash)?);
    }

    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    let mut file_ids = BTreeMap::new();
    let mut symbols_by_path = BTreeMap::<String, BTreeMap<String, Vec<String>>>::new();
    let mut symbol_kinds = BTreeMap::<String, String>::new();
    let known_paths = files
        .iter()
        .map(|file| file.path.clone())
        .collect::<BTreeSet<_>>();

    for file in &files {
        let id = node_id("file", &file.path, "");
        file_ids.insert(file.path.clone(), id.clone());
        nodes.push(GraphNode {
            id,
            kind: "file".to_string(),
            path: Some(file.path.clone()),
            name: None,
            language: Some(file.language.clone()),
            evidence_fingerprint: String::new(),
        });
    }
    for fact in &facts {
        let file_id = file_ids
            .get(&fact.path)
            .ok_or_else(|| format!("Missing file node for {}", fact.path))?;
        for declaration in &fact.declarations {
            let normalized_kind = normalize_symbol_kind(&declaration.kind);
            let qualified_name = if declaration.qualified_name.is_empty() {
                format!("{normalized_kind}:{}", declaration.name)
            } else {
                declaration.qualified_name.clone()
            };
            let id = node_id("symbol", &fact.path, &qualified_name);
            if !nodes.iter().any(|node| node.id == id) {
                nodes.push(GraphNode {
                    id: id.clone(),
                    kind: normalized_kind.clone(),
                    path: Some(fact.path.clone()),
                    name: Some(declaration.name.clone()),
                    language: Some(fact.language.clone()),
                    evidence_fingerprint: String::new(),
                });
            }
            symbol_kinds.insert(id.clone(), normalized_kind);
            symbols_by_path
                .entry(fact.path.clone())
                .or_default()
                .entry(declaration.name.clone())
                .or_default()
                .push(id.clone());
            edges.push(GraphEdge {
                from: file_id.clone(),
                to: id,
                kind: "declares".to_string(),
                evidence: "top-level-typescript-declaration".to_string(),
            });
        }
    }
    for symbols in symbols_by_path.values_mut() {
        for ids in symbols.values_mut() {
            ids.sort();
            ids.dedup();
        }
    }

    let mut external_ids = BTreeMap::new();
    for fact in &facts {
        let file_id = file_ids
            .get(&fact.path)
            .ok_or_else(|| format!("Missing file node for {}", fact.path))?;
        for import in &fact.imports {
            if let Some(target_path) = resolve_relative(&fact.path, &import.specifier, &known_paths)
            {
                let target_id = file_ids
                    .get(&target_path)
                    .ok_or_else(|| format!("Missing imported file node for {target_path}"))?;
                edges.push(GraphEdge {
                    from: file_id.clone(),
                    to: target_id.clone(),
                    kind: "imports".to_string(),
                    evidence: import.kind.clone(),
                });
            } else if import.specifier.starts_with('.') {
                let target_id = node_id("unresolved-module", &import.specifier, "");
                if !nodes.iter().any(|node| node.id == target_id) {
                    nodes.push(GraphNode {
                        id: target_id.clone(),
                        kind: "unresolved-module".to_string(),
                        path: None,
                        name: Some(import.specifier.clone()),
                        language: None,
                        evidence_fingerprint: String::new(),
                    });
                }
                edges.push(GraphEdge {
                    from: file_id.clone(),
                    to: target_id,
                    kind: "imports-unresolved".to_string(),
                    evidence: "missing-relative-module".to_string(),
                });
            } else if is_non_relative_alias(&import.specifier, &known_paths) {
                let target_id = node_id("unresolved-module", &import.specifier, "");
                if !nodes.iter().any(|node| node.id == target_id) {
                    nodes.push(GraphNode {
                        id: target_id.clone(),
                        kind: "unresolved-module".to_string(),
                        path: None,
                        name: Some(import.specifier.clone()),
                        language: None,
                        evidence_fingerprint: String::new(),
                    });
                }
                edges.push(GraphEdge {
                    from: file_id.clone(),
                    to: target_id,
                    kind: "imports-unresolved".to_string(),
                    evidence: "non-relative-path-alias".to_string(),
                });
            } else {
                let target_id = external_ids
                    .entry(import.specifier.clone())
                    .or_insert_with(|| node_id("external-module", &import.specifier, ""))
                    .clone();
                if !nodes.iter().any(|node| node.id == target_id) {
                    nodes.push(GraphNode {
                        id: target_id.clone(),
                        kind: "external-module".to_string(),
                        path: None,
                        name: Some(import.specifier.clone()),
                        language: None,
                        evidence_fingerprint: String::new(),
                    });
                }
                edges.push(GraphEdge {
                    from: file_id.clone(),
                    to: target_id,
                    kind: "imports-external".to_string(),
                    evidence: import.kind.clone(),
                });
            }
        }
    }

    let (module_exports, deferred_exports) = build_module_exports(&facts, &symbols_by_path);
    let resolution = ResolutionContext {
        file_ids: &file_ids,
        symbols_by_path: &symbols_by_path,
        symbol_kinds: &symbol_kinds,
        module_exports: &module_exports,
        deferred_exports: &deferred_exports,
        known_paths: &known_paths,
        external_ids: &external_ids,
    };
    resolve_calls(&mut facts, &resolution, &mut edges);

    nodes.sort_by(|left, right| left.id.cmp(&right.id));
    edges.sort_by(|left, right| {
        left.from
            .cmp(&right.from)
            .then_with(|| left.to.cmp(&right.to))
            .then_with(|| left.kind.cmp(&right.kind))
            .then_with(|| left.evidence.cmp(&right.evidence))
    });
    edges.dedup();

    let resolution_evidence = resolution_evidence(&facts, false);
    if resolution_evidence.truncated {
        truncated = true;
        omissions.extend(resolution_evidence.omissions.clone());
    }
    assign_node_fingerprints(&mut nodes, &edges, &facts);
    if nodes.len() > MAX_GRAPH_NODES {
        nodes.truncate(MAX_GRAPH_NODES);
        truncated = true;
        omissions.push(format!("graph nodes capped at {MAX_GRAPH_NODES}"));
    }
    if edges.len() > MAX_GRAPH_EDGES {
        edges.truncate(MAX_GRAPH_EDGES);
        truncated = true;
        omissions.push(format!("graph edges capped at {MAX_GRAPH_EDGES}"));
    }
    omissions.sort();
    omissions.dedup();

    let project_id = project_id(root);
    let source_revision = source_revision(root);
    let source_fingerprint = exact_source_fingerprint(&files)?;
    let identity_files = files
        .iter()
        .map(|file| (&file.path, &file.language))
        .collect::<Vec<_>>();
    let identity = GraphIdentity {
        project_id: &project_id,
        derivation_id: GRAPH_DERIVATION_ID,
        files: &identity_files,
        nodes: &nodes,
        edges: &edges,
        resolution_evidence: &resolution_evidence,
    };
    let graph_id = blake3::hash(&serde_json::to_vec(&identity).map_err(|error| error.to_string())?)
        .to_hex()
        .to_string();
    Ok((
        GraphSnapshot {
            schema_version: crate::model::GRAPH_SCHEMA.to_string(),
            product: PRODUCT_IDENTITY.to_string(),
            project_id,
            graph_id,
            graph_version: 0,
            source_revision,
            source_fingerprint,
            observation_id: String::new(),
            files,
            nodes,
            edges,
            resolution_evidence,
            truncated,
            omissions,
        },
        facts,
    ))
}

#[derive(Serialize)]
struct GraphIdentity<'a> {
    project_id: &'a str,
    derivation_id: &'a str,
    files: &'a Vec<(&'a String, &'a String)>,
    nodes: &'a [GraphNode],
    edges: &'a [GraphEdge],
    resolution_evidence: &'a ResolutionEvidence,
}

fn build_module_exports(
    facts: &[TypeScriptFacts],
    symbols_by_path: &BTreeMap<String, BTreeMap<String, Vec<String>>>,
) -> (ModuleExports, DeferredExports) {
    let mut module_exports = ModuleExports::new();
    let mut deferred_exports = BTreeSet::new();
    for fact in facts {
        let local_symbols = symbols_by_path.get(&fact.path);
        let exports = module_exports.entry(fact.path.clone()).or_default();
        for export in &fact.exports {
            if export.source.is_some() {
                deferred_exports.insert((fact.path.clone(), export.exported_name.clone()));
                continue;
            }
            if export.type_only {
                continue;
            }
            let Some(local_name) = export.local_name.as_deref() else {
                continue;
            };
            let imported_binding = fact.imports.iter().any(|import| {
                import.local_name.as_deref() == Some(local_name)
                    && import.kind != "side-effect-import"
            });
            let Some(local_symbols) = local_symbols else {
                if imported_binding {
                    deferred_exports.insert((fact.path.clone(), export.exported_name.clone()));
                }
                continue;
            };
            let Some(targets) = local_symbols.get(local_name) else {
                if imported_binding {
                    deferred_exports.insert((fact.path.clone(), export.exported_name.clone()));
                }
                continue;
            };
            let entry = exports.entry(export.exported_name.clone()).or_default();
            entry.extend(targets.iter().cloned());
        }
        for targets in exports.values_mut() {
            targets.sort();
            targets.dedup();
        }
    }
    (module_exports, deferred_exports)
}

struct ResolutionContext<'a> {
    file_ids: &'a BTreeMap<String, String>,
    symbols_by_path: &'a BTreeMap<String, BTreeMap<String, Vec<String>>>,
    symbol_kinds: &'a BTreeMap<String, String>,
    module_exports: &'a ModuleExports,
    deferred_exports: &'a DeferredExports,
    known_paths: &'a BTreeSet<String>,
    external_ids: &'a BTreeMap<String, String>,
}

fn resolve_calls(
    facts: &mut [TypeScriptFacts],
    resolution: &ResolutionContext<'_>,
    edges: &mut Vec<GraphEdge>,
) {
    for fact in facts {
        let Some(file_id) = resolution.file_ids.get(&fact.path) else {
            continue;
        };
        let imports = fact.imports.clone();
        let calls = fact.calls.clone();
        fact.resolution_records.clear();
        for call in calls {
            let caller_id = caller_node_id(
                &fact.path,
                call.caller.as_deref(),
                file_id,
                resolution.symbols_by_path,
            );
            let outcome = resolve_call(&fact.path, &call, &imports, resolution);
            if outcome.status == "resolved"
                && let Some(target) = outcome.candidates.first()
            {
                edges.push(GraphEdge {
                    from: caller_id.clone(),
                    to: target.clone(),
                    kind: "calls".to_string(),
                    evidence: outcome.reason.clone(),
                });
            }
            fact.resolution_records.push(SymbolResolution {
                path: fact.path.clone(),
                caller_node_id: caller_id,
                reference: call.callee.unwrap_or_else(|| "<dynamic>".to_string()),
                form: call.callee_form,
                status: outcome.status,
                reason: outcome.reason,
                candidate_node_ids: outcome.candidates,
                occurrence_count: 1,
            });
        }
        coalesce_resolutions(&mut fact.resolution_records);
    }
}

fn caller_node_id(
    path: &str,
    caller: Option<&str>,
    file_id: &str,
    symbols_by_path: &BTreeMap<String, BTreeMap<String, Vec<String>>>,
) -> String {
    let Some(caller) = caller else {
        return file_id.to_string();
    };
    let Some(symbols) = symbols_by_path.get(path) else {
        return file_id.to_string();
    };
    let Some((kind, name)) = caller.split_once(':') else {
        return file_id.to_string();
    };
    symbols
        .get(name)
        .and_then(|ids| {
            ids.iter()
                .find(|id| **id == node_id("symbol", path, &format!("{kind}:{name}")))
        })
        .cloned()
        .or_else(|| symbols.get(name).and_then(|ids| ids.first().cloned()))
        .unwrap_or_else(|| file_id.to_string())
}

#[derive(Debug, Clone)]
struct ResolutionOutcome {
    status: String,
    reason: String,
    candidates: Vec<String>,
}

fn resolve_call(
    path: &str,
    call: &crate::model::TypeScriptCall,
    imports: &[crate::model::TypeScriptImport],
    resolution: &ResolutionContext<'_>,
) -> ResolutionOutcome {
    if call.dynamic {
        return unresolved("dynamic-callee");
    }
    let Some(callee) = call.callee.as_deref() else {
        return unresolved("dynamic-callee");
    };
    if call.callee_form == "identifier" {
        let matching_imports = imports
            .iter()
            .filter(|import| {
                import.local_name.as_deref() == Some(callee)
                    && import.kind != "re-export"
                    && import.kind != "side-effect-import"
            })
            .collect::<Vec<_>>();
        if matching_imports.len() > 1 {
            return unresolved_with_status("ambiguous-import-binding", "ambiguous");
        }
        if let Some(import) = matching_imports.first().copied() {
            if call.shadowed {
                return unresolved("local-binding-shadowed-import");
            }
            if import.type_only {
                return unresolved("type-only-binding");
            }
            if import.kind == "namespace-import" {
                return unresolved("namespace-called-as-function");
            }
            return resolve_import_target(
                path,
                import,
                import.imported_name.as_deref().unwrap_or("default"),
                resolution,
            );
        }
        let candidates = callable_candidates(
            resolution
                .symbols_by_path
                .get(path)
                .and_then(|symbols| symbols.get(callee))
                .cloned()
                .unwrap_or_default(),
            resolution.symbol_kinds,
        );
        return unique_or_unresolved(candidates, "same-module-declaration");
    }

    if call.callee_form == "member" {
        let Some(receiver) = call.receiver.as_deref() else {
            return unresolved("unsupported-member-call");
        };
        let Some(property) = callee
            .strip_prefix(receiver)
            .and_then(|value| value.strip_prefix('.'))
        else {
            return unresolved("unsupported-member-call");
        };
        let matching_imports = imports
            .iter()
            .filter(|import| {
                import.local_name.as_deref() == Some(receiver)
                    && import.kind == "namespace-import"
                    && import.kind != "re-export"
            })
            .collect::<Vec<_>>();
        if matching_imports.len() > 1 {
            return unresolved_with_status("ambiguous-import-binding", "ambiguous");
        }
        let Some(import) = matching_imports.first().copied() else {
            return unresolved("unsupported-member-call");
        };
        if call.shadowed {
            return unresolved("local-binding-shadowed-import");
        }
        if import.type_only {
            return unresolved("type-only-binding");
        }
        return resolve_import_target(path, import, property, resolution);
    }
    unresolved("unsupported-callee-form")
}

fn resolve_import_target(
    current_path: &str,
    import: &crate::model::TypeScriptImport,
    imported_name: &str,
    resolution: &ResolutionContext<'_>,
) -> ResolutionOutcome {
    if is_non_relative_alias(&import.specifier, resolution.known_paths) {
        return unresolved("non-relative-path-alias");
    }
    if !import.specifier.starts_with('.') {
        return ResolutionOutcome {
            status: "external".to_string(),
            reason: "external-module".to_string(),
            candidates: resolution
                .external_ids
                .get(&import.specifier)
                .cloned()
                .into_iter()
                .collect(),
        };
    }
    let Some(target_path) =
        resolve_relative(current_path, &import.specifier, resolution.known_paths)
    else {
        return unresolved("missing-relative-module");
    };
    let Some(exports) = resolution.module_exports.get(&target_path) else {
        return unresolved("missing-relative-module");
    };
    let candidates = callable_candidates(
        exports.get(imported_name).cloned().unwrap_or_default(),
        resolution.symbol_kinds,
    );
    if candidates.is_empty() {
        let deferred = resolution
            .deferred_exports
            .contains(&(target_path.clone(), imported_name.to_string()))
            || resolution
                .deferred_exports
                .contains(&(target_path, "*".to_string()));
        return unresolved(if deferred {
            "re-export-resolution-deferred"
        } else {
            "missing-export"
        });
    }
    unique_or_unresolved(
        candidates,
        match import.kind.as_str() {
            "named-import" => "named-import-binding",
            "default-import" => "default-import-binding",
            "namespace-import" => "namespace-import-binding",
            _ => "direct-import-binding",
        },
    )
}

fn callable_candidates(
    candidates: Vec<String>,
    symbol_kinds: &BTreeMap<String, String>,
) -> Vec<String> {
    let mut candidates = candidates
        .into_iter()
        .filter(|candidate| {
            symbol_kinds.get(candidate).is_some_and(|kind| {
                matches!(
                    kind.as_str(),
                    "function"
                        | "function_signature"
                        | "class"
                        | "class_signature"
                        | "variable"
                        | "enum"
                )
            })
        })
        .collect::<Vec<_>>();
    candidates.sort();
    candidates.dedup();
    candidates
}

fn unique_or_unresolved(candidates: Vec<String>, reason: &str) -> ResolutionOutcome {
    match candidates.as_slice() {
        [candidate] => ResolutionOutcome {
            status: "resolved".to_string(),
            reason: reason.to_string(),
            candidates: vec![candidate.clone()],
        },
        [] => unresolved("unresolved-identifier"),
        _ => ResolutionOutcome {
            status: "ambiguous".to_string(),
            reason: "ambiguous-symbol".to_string(),
            candidates,
        },
    }
}

fn unresolved(reason: &str) -> ResolutionOutcome {
    unresolved_with_status(reason, "unresolved")
}

fn unresolved_with_status(reason: &str, status: &str) -> ResolutionOutcome {
    ResolutionOutcome {
        status: status.to_string(),
        reason: reason.to_string(),
        candidates: Vec::new(),
    }
}

fn coalesce_resolutions(records: &mut Vec<SymbolResolution>) {
    records.sort_by(resolution_ordering);
    let mut coalesced = Vec::with_capacity(records.len());
    for record in records.drain(..) {
        if let Some(existing) = coalesced.last_mut()
            && same_resolution_key(existing, &record)
        {
            existing.occurrence_count = existing
                .occurrence_count
                .saturating_add(record.occurrence_count);
        } else {
            coalesced.push(record);
        }
    }
    *records = coalesced;
}

fn same_resolution_key(left: &SymbolResolution, right: &SymbolResolution) -> bool {
    left.path == right.path
        && left.caller_node_id == right.caller_node_id
        && left.reference == right.reference
        && left.form == right.form
        && left.status == right.status
        && left.reason == right.reason
        && left.candidate_node_ids == right.candidate_node_ids
}

fn resolution_ordering(left: &SymbolResolution, right: &SymbolResolution) -> std::cmp::Ordering {
    left.path
        .cmp(&right.path)
        .then_with(|| left.caller_node_id.cmp(&right.caller_node_id))
        .then_with(|| left.reference.cmp(&right.reference))
        .then_with(|| left.form.cmp(&right.form))
        .then_with(|| left.status.cmp(&right.status))
        .then_with(|| left.reason.cmp(&right.reason))
        .then_with(|| left.candidate_node_ids.cmp(&right.candidate_node_ids))
}

pub fn resolution_evidence(facts: &[TypeScriptFacts], legacy: bool) -> ResolutionEvidence {
    if legacy
        || facts.iter().any(|fact| {
            fact.schema_version != crate::model::TYPESCRIPT_FACTS_SCHEMA
                || fact.parser != crate::typescript::PARSER_IDENTITY
        })
    {
        return ResolutionEvidence {
            schema_version: TYPESCRIPT_RESOLUTION_SCHEMA.to_string(),
            status: "unavailable".to_string(),
            records: Vec::new(),
            truncated: false,
            omissions: vec!["legacy-facts-without-resolution-evidence".to_string()],
        };
    }
    let mut records = Vec::new();
    for fact in facts {
        records.extend(fact.resolution_records.iter().cloned());
    }
    coalesce_resolutions(&mut records);
    let mut truncated = false;
    let mut omissions = Vec::new();
    if records.len() > MAX_RESOLUTION_RECORDS {
        records.truncate(MAX_RESOLUTION_RECORDS);
        truncated = true;
        omissions.push(format!(
            "resolution records capped at {MAX_RESOLUTION_RECORDS}"
        ));
    }
    ResolutionEvidence {
        schema_version: TYPESCRIPT_RESOLUTION_SCHEMA.to_string(),
        status: if truncated { "truncated" } else { "complete" }.to_string(),
        records,
        truncated,
        omissions,
    }
}

fn assign_node_fingerprints(
    nodes: &mut [GraphNode],
    edges: &[GraphEdge],
    facts: &[TypeScriptFacts],
) {
    let mut intrinsic = BTreeMap::<String, String>::new();
    for node in nodes.iter() {
        let value = if node.kind == "file" {
            node.path
                .as_deref()
                .and_then(|path| facts.iter().find(|fact| fact.path == path))
                .map(canonical_fact)
                .unwrap_or_default()
        } else {
            let declaration_fingerprints = node
                .path
                .as_deref()
                .and_then(|path| facts.iter().find(|fact| fact.path == path))
                .map(|fact| {
                    fact.declarations
                        .iter()
                        .filter(|declaration| {
                            declaration.qualified_name
                                == format!(
                                    "{}:{}",
                                    node.kind,
                                    node.name.as_deref().unwrap_or_default()
                                )
                                || (declaration.qualified_name.is_empty()
                                    && declaration.name == node.name.as_deref().unwrap_or_default()
                                    && normalize_symbol_kind(&declaration.kind) == node.kind)
                        })
                        .map(|declaration| {
                            format!("{}:{}", declaration.ast_fingerprint, declaration.exported)
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let exported = node
                .path
                .as_deref()
                .and_then(|path| facts.iter().find(|fact| fact.path == path))
                .is_some_and(|fact| {
                    fact.declarations.iter().any(|declaration| {
                        declaration.name == node.name.as_deref().unwrap_or_default()
                            && normalize_symbol_kind(&declaration.kind) == node.kind
                            && declaration.exported
                    })
                });
            let mut declaration_fingerprints = declaration_fingerprints;
            declaration_fingerprints.sort();
            format!(
                "symbol\0{}\0{}\0{}\0exported={exported}\0ast={}",
                node.kind,
                node.path.as_deref().unwrap_or_default(),
                node.name.as_deref().unwrap_or_default(),
                declaration_fingerprints.join("|")
            )
        };
        intrinsic.insert(node.id.clone(), value);
    }
    for node in nodes {
        let mut canonical = intrinsic.remove(&node.id).unwrap_or_default();
        if let Some(path) = node.path.as_deref()
            && let Some(fact) = facts.iter().find(|fact| fact.path == path)
        {
            for record in &fact.resolution_records {
                if node.kind == "file" || record.caller_node_id == node.id {
                    canonical.push('\0');
                    canonical.push_str(&serde_json::to_string(record).unwrap_or_default());
                }
            }
        }
        let mut signatures = edges
            .iter()
            .filter_map(|edge| {
                if edge.from == node.id {
                    Some(format!(
                        "out\0{}\0{}\0{}",
                        edge.kind, edge.evidence, edge.to
                    ))
                } else if edge.to == node.id {
                    Some(format!(
                        "in\0{}\0{}\0{}",
                        edge.kind, edge.evidence, edge.from
                    ))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        signatures.sort();
        for signature in signatures {
            canonical.push('\0');
            canonical.push_str(&signature);
        }
        node.evidence_fingerprint = blake3::hash(canonical.as_bytes()).to_hex().to_string();
    }
}

fn canonical_fact(fact: &TypeScriptFacts) -> String {
    let resolutions = fact
        .resolution_records
        .iter()
        .map(|record| serde_json::to_string(record).unwrap_or_default())
        .collect::<Vec<_>>();
    format!(
        "{}\0{}\0{}\0{}\0{}",
        fact.language,
        fact.parser,
        fact.parse_status,
        fact.canonical_fingerprint,
        resolutions.join("|")
    )
}

pub fn node_id(kind: &str, path: &str, name: &str) -> String {
    let input = format!("flopeek-node-v1\0{kind}\0{path}\0{name}");
    format!("node_{}", blake3::hash(input.as_bytes()).to_hex())
}

pub fn project_id(root: &Path) -> String {
    let identity = root
        .canonicalize()
        .ok()
        .and_then(|path| path.to_str().map(ToOwned::to_owned))
        .unwrap_or_else(|| root.to_string_lossy().into_owned());
    format!(
        "project_{}",
        blake3::hash(format!("flopeek-project-v1\0{identity}").as_bytes()).to_hex()
    )
}

pub fn source_revision(root: &Path) -> String {
    let head = Command::new("git")
        .args([
            "-C",
            &root.to_string_lossy(),
            "rev-parse",
            "--verify",
            "HEAD",
        ])
        .output();
    let Ok(head) = head else {
        return "unavailable".to_string();
    };
    if !head.status.success() {
        return "unavailable".to_string();
    }
    let revision = String::from_utf8_lossy(&head.stdout).trim().to_string();
    if revision.is_empty() {
        return "unavailable".to_string();
    }
    let dirty = Command::new("git")
        .args([
            "-C",
            &root.to_string_lossy(),
            "status",
            "--porcelain",
            "--untracked-files=all",
        ])
        .output()
        .is_ok_and(|output| {
            output.status.success()
                && String::from_utf8_lossy(&output.stdout).lines().any(|line| {
                    let path = line.get(3..).unwrap_or_default().replace('\\', "/");
                    !(path == ".flopeek" || path.starts_with(".flopeek/"))
                })
        });
    if dirty {
        format!("{revision}+dirty")
    } else {
        revision
    }
}

pub fn resolve_relative(
    current: &str,
    specifier: &str,
    known_paths: &BTreeSet<String>,
) -> Option<String> {
    if !specifier.starts_with('.') {
        return None;
    }
    let current_parent = Path::new(current).parent().unwrap_or_else(|| Path::new(""));
    let joined = current_parent.join(specifier);
    let mut base = Vec::new();
    for component in joined.components() {
        match component {
            Component::Normal(value) => base.push(value.to_str()?.to_string()),
            Component::CurDir => {}
            Component::ParentDir => {
                base.pop()?;
            }
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    let base = base.join("/");
    let candidates = [
        base.clone(),
        format!("{base}.ts"),
        format!("{base}.tsx"),
        format!("{base}.d.ts"),
        format!("{base}/index.ts"),
        format!("{base}/index.tsx"),
        format!("{base}/index.d.ts"),
    ];
    candidates
        .iter()
        .find(|candidate| known_paths.contains(*candidate))
        .cloned()
}

fn is_non_relative_alias(specifier: &str, known_paths: &BTreeSet<String>) -> bool {
    if specifier.starts_with("@/")
        || specifier.starts_with("~/")
        || specifier.starts_with('#')
        || specifier.starts_with('/')
        || specifier.contains('\\')
    {
        return true;
    }
    let candidates = [
        specifier.to_string(),
        format!("{specifier}.ts"),
        format!("{specifier}.tsx"),
        format!("{specifier}.d.ts"),
        format!("{specifier}/index.ts"),
        format!("{specifier}/index.tsx"),
        format!("{specifier}/index.d.ts"),
    ];
    candidates
        .iter()
        .any(|candidate| known_paths.contains(candidate))
}

fn exact_source_fingerprint(files: &[SourceFile]) -> Result<String, String> {
    let canonical = files
        .iter()
        .map(|file| (&file.path, &file.language, file.bytes, &file.hash))
        .collect::<Vec<_>>();
    serde_json::to_vec(&canonical)
        .map(|bytes| blake3::hash(&bytes).to_hex().to_string())
        .map_err(|error| format!("Unable to derive source fingerprint: {error}"))
}

fn normalize_symbol_kind(kind: &str) -> String {
    kind.trim().to_ascii_lowercase().replace(['-', ' '], "_")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(prefix: &str) -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!("flopeek-{prefix}-{suffix}"))
    }

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
                node.path.as_deref() == Some("src/payment.ts")
                    && node.name.as_deref() == Some("charge")
            })
            .expect("charge symbol");
        assert!(graph.edges.iter().any(|edge| {
            edge.from == checkout.id && edge.to == payment.id && edge.kind == "calls"
        }));
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

        let (graph, facts) = build(&root).expect("graph");
        let entry = facts
            .iter()
            .find(|fact| fact.path == "src/entry.ts")
            .expect("entry facts");
        assert!(
            entry
                .resolution_records
                .iter()
                .any(|record| record.status == "resolved"
                    && record.reason == "default-import-binding")
        );
        assert!(entry.resolution_records.iter().any(|record| {
            record.status == "resolved" && record.reason == "namespace-import-binding"
        }));
        assert!(entry.resolution_records.iter().any(|record| {
            record.status == "unresolved" && record.reason == "local-binding-shadowed-import"
        }));
        assert!(entry
            .resolution_records
            .iter()
            .any(|record| record.status == "unresolved" && record.reason == "type-only-binding"));
        assert!(entry.resolution_records.iter().any(|record| {
            record.status == "unresolved" && record.reason == "non-relative-path-alias"
        }));
        assert!(entry.resolution_records.iter().any(|record| {
            record.status == "resolved" && record.reason == "named-import-binding"
        }));
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
        assert!(entry.resolution_records.iter().any(|record| {
            record.reference == "settle" && record.caller_node_id == entry_file_id
        }));
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
        assert!(reexport.resolution_records.iter().any(|record| {
            record.status == "unresolved" && record.reason == "re-export-resolution-deferred"
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
            graph.edges.iter().any(|edge| {
                edge.kind == "imports-external" && edge.evidence == "default-import"
            })
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
}
