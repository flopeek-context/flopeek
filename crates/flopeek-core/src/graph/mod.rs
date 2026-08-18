//! Deterministic graph assembly for TypeScript/TSX evidence.

use crate::discovery::{discover, read_source};
use crate::module_resolution::{
    ModuleResolver, NonRelativeResolution, resolve_relative as resolve_module_relative,
};

use crate::model::{
    GraphEdge, GraphNode, GraphSnapshot, PRODUCT_IDENTITY, ResolutionEvidence, SourceFile,
    SymbolResolution, TYPESCRIPT_RESOLUTION_SCHEMA, TypeScriptDeclaration, TypeScriptFacts,
};
use crate::typescript;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;

pub const MAX_SOURCE_FILES: usize = 10_000;
pub const MAX_GRAPH_NODES: usize = 50_000;
pub const MAX_GRAPH_EDGES: usize = 100_000;
pub const MAX_RESOLUTION_RECORDS: usize = 100_000;
pub const MAX_REEXPORT_DEPTH: usize = 32;
pub const GRAPH_DERIVATION_ID: &str = "typescript-structural-evidence-v5";

#[derive(Debug, Clone, PartialEq, Eq)]
enum ExportBinding {
    Local(Vec<String>),
    Forward {
        source: String,
        name: String,
        type_only: bool,
    },
    Star {
        source: String,
        type_only: bool,
    },
    Namespace {
        source: String,
        type_only: bool,
    },
    TypeOnly {
        ids: Vec<String>,
    },
}

type ModuleExports = BTreeMap<String, BTreeMap<String, Vec<ExportBinding>>>;

pub fn build(root: &Path) -> Result<(GraphSnapshot, Vec<TypeScriptFacts>), String> {
    let module_resolver = ModuleResolver::load(root);
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
    let mut members_by_owner = BTreeMap::<String, BTreeMap<String, Vec<String>>>::new();
    let mut declarations_by_id = BTreeMap::<String, TypeScriptDeclaration>::new();
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
                    name: Some(if declaration.owner.is_some() {
                        qualified_name.clone()
                    } else {
                        declaration.name.clone()
                    }),
                    language: Some(fact.language.clone()),
                    evidence_fingerprint: String::new(),
                });
            }
            symbol_kinds.insert(id.clone(), normalized_kind);
            declarations_by_id
                .entry(id.clone())
                .or_insert_with(|| declaration.clone());
            if let Some(owner) = declaration.owner.as_deref() {
                let owner_id = node_id("symbol", &fact.path, owner);
                members_by_owner
                    .entry(owner_id.clone())
                    .or_default()
                    .entry(declaration.name.clone())
                    .or_default()
                    .push(id.clone());
                edges.push(GraphEdge {
                    from: owner_id,
                    to: id,
                    kind: "declares-member".to_string(),
                    evidence: "typescript-class-member".to_string(),
                });
            } else {
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
    }
    for symbols in symbols_by_path.values_mut() {
        for ids in symbols.values_mut() {
            ids.sort();
            ids.dedup();
        }
    }
    for members in members_by_owner.values_mut() {
        for ids in members.values_mut() {
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
            } else {
                let non_relative =
                    module_resolver.resolve_non_relative(&import.specifier, &known_paths);
                match non_relative {
                    NonRelativeResolution::Local(target_path) => {
                        let target_id = file_ids.get(&target_path).ok_or_else(|| {
                            format!("Missing aliased file node for {target_path}")
                        })?;
                        edges.push(GraphEdge {
                            from: file_id.clone(),
                            to: target_id.clone(),
                            kind: "imports".to_string(),
                            evidence: "tsconfig-path-alias".to_string(),
                        });
                    }
                    NonRelativeResolution::Unresolved(reason)
                        if is_non_relative_alias(&import.specifier, &known_paths)
                            || !reason.starts_with("tsconfig-resolution-") =>
                    {
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
                            evidence: reason,
                        });
                    }
                    NonRelativeResolution::Unresolved(_) | NonRelativeResolution::External
                        if module_resolver.basis.root_config.is_none()
                            && is_non_relative_alias(&import.specifier, &known_paths) =>
                    {
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
                    }
                    NonRelativeResolution::Unresolved(_) | NonRelativeResolution::External => {
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
        }
    }

    let module_exports = build_module_exports(&facts, &symbols_by_path);
    let resolution = ResolutionContext {
        file_ids: &file_ids,
        symbols_by_path: &symbols_by_path,
        members_by_owner: &members_by_owner,
        declarations_by_id: &declarations_by_id,
        symbol_kinds: &symbol_kinds,
        module_exports: &module_exports,
        known_paths: &known_paths,
        external_ids: &external_ids,
        module_resolver: &module_resolver,
    };
    resolve_calls(&mut facts, &resolution, &mut edges);
    resolve_heritage(&mut facts, &resolution, &mut edges);

    nodes.sort_by(|left, right| left.id.cmp(&right.id));
    edges.sort_by(|left, right| {
        left.from
            .cmp(&right.from)
            .then_with(|| left.to.cmp(&right.to))
            .then_with(|| left.kind.cmp(&right.kind))
            .then_with(|| left.evidence.cmp(&right.evidence))
    });
    edges.dedup();

    let mut resolution_evidence = resolution_evidence(&facts, false);
    if resolution_evidence.truncated {
        truncated = true;
        omissions.extend(resolution_evidence.omissions.clone());
    }
    if module_resolver.basis.status == "truncated" {
        truncated = true;
        resolution_evidence.status = "truncated".to_string();
        resolution_evidence.truncated = true;
        resolution_evidence
            .omissions
            .extend(module_resolver.basis.limitations.clone());
        omissions.extend(module_resolver.basis.limitations.clone());
    }
    if module_resolver.basis.status == "unavailable" {
        resolution_evidence.status = "unavailable".to_string();
        resolution_evidence
            .omissions
            .push("module-resolution-basis-unavailable".to_string());
        resolution_evidence
            .omissions
            .extend(module_resolver.basis.limitations.clone());
        omissions.extend(module_resolver.basis.limitations.clone());
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
    let module_resolution = module_resolver.basis.clone();
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
            module_resolution,
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
) -> ModuleExports {
    let mut module_exports = ModuleExports::new();
    for fact in facts {
        let local_symbols = symbols_by_path.get(&fact.path);
        let exports = module_exports.entry(fact.path.clone()).or_default();
        for export in &fact.exports {
            if let Some(source) = export.source.as_deref() {
                let binding = if export.kind == "namespace-re-export" {
                    ExportBinding::Namespace {
                        source: source.to_string(),
                        type_only: export.type_only,
                    }
                } else if export.exported_name == "*" {
                    ExportBinding::Star {
                        source: source.to_string(),
                        type_only: export.type_only,
                    }
                } else {
                    ExportBinding::Forward {
                        source: source.to_string(),
                        name: export
                            .local_name
                            .as_deref()
                            .unwrap_or(&export.exported_name)
                            .to_string(),
                        type_only: export.type_only,
                    }
                };
                exports
                    .entry(export.exported_name.clone())
                    .or_default()
                    .push(binding);
                continue;
            }
            let Some(local_name) = export.local_name.as_deref() else {
                continue;
            };
            if let Some(targets) = local_symbols.and_then(|symbols| symbols.get(local_name)) {
                exports
                    .entry(export.exported_name.clone())
                    .or_default()
                    .push(if export.type_only {
                        ExportBinding::TypeOnly {
                            ids: targets.clone(),
                        }
                    } else {
                        ExportBinding::Local(targets.clone())
                    });
                continue;
            }
            if let Some(import) = fact.imports.iter().find(|import| {
                import.local_name.as_deref() == Some(local_name)
                    && import.kind != "side-effect-import"
                    && import.kind != "re-export"
                    && !import.type_only
            }) {
                let binding = if import.kind == "namespace-import" {
                    ExportBinding::Namespace {
                        source: import.specifier.clone(),
                        type_only: export.type_only || import.type_only,
                    }
                } else {
                    ExportBinding::Forward {
                        source: import.specifier.clone(),
                        name: import
                            .imported_name
                            .as_deref()
                            .unwrap_or("default")
                            .to_string(),
                        type_only: export.type_only || import.type_only,
                    }
                };
                exports
                    .entry(export.exported_name.clone())
                    .or_default()
                    .push(binding);
            }
        }
        for bindings in exports.values_mut() {
            bindings.sort_by(|left, right| format!("{left:?}").cmp(&format!("{right:?}")));
            bindings.dedup();
        }
    }
    module_exports
}

struct ResolutionContext<'a> {
    file_ids: &'a BTreeMap<String, String>,
    symbols_by_path: &'a BTreeMap<String, BTreeMap<String, Vec<String>>>,
    members_by_owner: &'a BTreeMap<String, BTreeMap<String, Vec<String>>>,
    declarations_by_id: &'a BTreeMap<String, TypeScriptDeclaration>,
    symbol_kinds: &'a BTreeMap<String, String>,
    module_exports: &'a ModuleExports,
    known_paths: &'a BTreeSet<String>,
    external_ids: &'a BTreeMap<String, String>,
    module_resolver: &'a ModuleResolver,
}

fn resolve_calls(
    facts: &mut [TypeScriptFacts],
    resolution: &ResolutionContext<'_>,
    edges: &mut Vec<GraphEdge>,
) {
    let mut export_cache = BTreeMap::<(String, String), ResolutionOutcome>::new();
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
                resolution.symbol_kinds,
            );
            let outcome = resolve_call(&fact.path, &call, &imports, resolution, &mut export_cache);
            if outcome.status == "resolved"
                && let Some(target) = outcome.candidates.first()
            {
                edges.push(GraphEdge {
                    from: caller_id.clone(),
                    to: target.clone(),
                    kind: if call.callee_form == "constructor" {
                        "constructs".to_string()
                    } else {
                        "calls".to_string()
                    },
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

fn resolve_heritage(
    facts: &mut [TypeScriptFacts],
    resolution: &ResolutionContext<'_>,
    edges: &mut Vec<GraphEdge>,
) {
    let mut export_cache = BTreeMap::<(String, String), ResolutionOutcome>::new();
    for fact in facts {
        let heritage_items = fact.heritage.clone();
        for heritage in heritage_items {
            let owner_id = node_id("symbol", &fact.path, &heritage.owner);
            let mut outcome = resolve_type_reference(
                &fact.path,
                &heritage,
                &fact.imports,
                resolution,
                &mut export_cache,
            );
            if outcome.status == "resolved" {
                let allowed = heritage_target_kinds(&heritage);
                outcome.candidates.retain(|candidate| {
                    resolution
                        .symbol_kinds
                        .get(candidate)
                        .is_some_and(|kind| allowed.contains(kind.as_str()))
                });
                outcome.candidates.sort();
                outcome.candidates.dedup();
                if outcome.candidates.len() > 1 {
                    outcome.status = "ambiguous".to_string();
                    outcome.reason = "ambiguous-heritage-target".to_string();
                } else if outcome.candidates.is_empty() {
                    outcome.status = "unresolved".to_string();
                    outcome.reason = "unsupported-heritage-target-kind".to_string();
                }
            }
            if outcome.status == "resolved"
                && let Some(target) = outcome.candidates.first()
            {
                edges.push(GraphEdge {
                    from: owner_id.clone(),
                    to: target.clone(),
                    kind: heritage.relation.clone(),
                    evidence: format!("typescript-heritage-{}", heritage.relation),
                });
            }
            fact.resolution_records.push(SymbolResolution {
                path: fact.path.clone(),
                caller_node_id: owner_id,
                reference: heritage.reference,
                form: format!("heritage-{}-{}", heritage.relation, heritage.form),
                status: outcome.status,
                reason: outcome.reason,
                candidate_node_ids: outcome.candidates,
                occurrence_count: 1,
            });
        }
        coalesce_resolutions(&mut fact.resolution_records);
    }
}

fn heritage_target_kinds(heritage: &crate::model::TypeScriptHeritage) -> BTreeSet<&'static str> {
    match (
        heritage.owner.split_once(':').map(|parts| parts.0),
        heritage.relation.as_str(),
    ) {
        (Some("class"), "extends") => BTreeSet::from(["class"]),
        (_, "implements") => BTreeSet::from(["class", "interface"]),
        (_, "extends") => BTreeSet::from(["class", "interface"]),
        _ => BTreeSet::new(),
    }
}

fn resolve_type_reference(
    path: &str,
    heritage: &crate::model::TypeScriptHeritage,
    imports: &[crate::model::TypeScriptImport],
    resolution: &ResolutionContext<'_>,
    export_cache: &mut BTreeMap<(String, String), ResolutionOutcome>,
) -> ResolutionOutcome {
    if heritage.form == "dynamic" {
        return unresolved("dynamic-heritage-unsupported");
    }
    let reference = heritage.reference.as_str();
    if heritage.form == "identifier" {
        let local = resolution
            .symbols_by_path
            .get(path)
            .and_then(|symbols| symbols.get(reference))
            .cloned()
            .unwrap_or_default();
        if !local.is_empty() {
            return unique_or_unresolved(local, "local-heritage-binding");
        }
        let matching_imports = imports
            .iter()
            .filter(|import| {
                import.local_name.as_deref() == Some(reference)
                    && import.kind != "re-export"
                    && import.kind != "side-effect-import"
            })
            .collect::<Vec<_>>();
        if matching_imports.len() > 1 {
            return unresolved_with_status("ambiguous-import-binding", "ambiguous");
        }
        let Some(import) = matching_imports.first().copied() else {
            return unresolved("missing-heritage-binding");
        };
        if import.kind == "namespace-import" {
            return unresolved("namespace-heritage-binding");
        }
        let mut result = resolve_import_raw(
            path,
            import,
            import.imported_name.as_deref().unwrap_or("default"),
            resolution,
            export_cache,
        );
        if result.type_only
            && heritage.owner.starts_with("class:")
            && heritage.relation == "extends"
        {
            result = unresolved("type-only-extends-binding");
        }
        return result;
    }
    let Some((receiver, property)) = reference.split_once('.') else {
        return unresolved("unsupported-heritage-reference");
    };
    let matching_imports = imports
        .iter()
        .filter(|import| {
            import.local_name.as_deref() == Some(receiver)
                && import.kind == "namespace-import"
                && !import.type_only
        })
        .collect::<Vec<_>>();
    if matching_imports.len() > 1 {
        return unresolved_with_status("ambiguous-import-binding", "ambiguous");
    }
    let Some(import) = matching_imports.first().copied() else {
        return unresolved("unsupported-heritage-reference");
    };
    resolve_import_raw(path, import, property, resolution, export_cache)
}

fn resolve_import_raw(
    current_path: &str,
    import: &crate::model::TypeScriptImport,
    imported_name: &str,
    resolution: &ResolutionContext<'_>,
    export_cache: &mut BTreeMap<(String, String), ResolutionOutcome>,
) -> ResolutionOutcome {
    if !import.specifier.starts_with('.')
        && resolution.module_resolver.basis.root_config.is_none()
        && is_non_relative_alias(&import.specifier, resolution.known_paths)
    {
        return unresolved("non-relative-path-alias");
    }
    let Some(target_path) = resolve_import_module(current_path, &import.specifier, resolution)
    else {
        return resolve_external_or_unresolved(&import.specifier, resolution);
    };
    let mut result = resolve_export(
        &target_path,
        imported_name,
        resolution,
        &mut Vec::new(),
        export_cache,
    );
    result.type_only |= import.type_only;
    result
}

fn caller_node_id(
    path: &str,
    caller: Option<&str>,
    file_id: &str,
    symbols_by_path: &BTreeMap<String, BTreeMap<String, Vec<String>>>,
    symbol_kinds: &BTreeMap<String, String>,
) -> String {
    let Some(caller) = caller else {
        return file_id.to_string();
    };
    let Some(symbols) = symbols_by_path.get(path) else {
        return file_id.to_string();
    };
    let direct_id = node_id("symbol", path, caller);
    if symbol_kinds.contains_key(&direct_id) {
        return direct_id;
    }
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
    namespace_modules: Vec<String>,
    type_only: bool,
}

fn resolve_call(
    path: &str,
    call: &crate::model::TypeScriptCall,
    imports: &[crate::model::TypeScriptImport],
    resolution: &ResolutionContext<'_>,
    export_cache: &mut BTreeMap<(String, String), ResolutionOutcome>,
) -> ResolutionOutcome {
    if call.dynamic {
        return unresolved(if call.callee_form == "dynamic-constructor" {
            "dynamic-constructor"
        } else {
            "dynamic-callee"
        });
    }
    let Some(callee) = call.callee.as_deref() else {
        return unresolved("dynamic-callee");
    };
    if call.callee_form == "constructor" {
        return resolve_constructor_call(path, callee, imports, call, resolution, export_cache);
    }
    if call.callee_form == "this-member" {
        let Some(receiver) = call.receiver.as_deref() else {
            return unresolved("unsupported-member-call");
        };
        let Some(property) = callee
            .strip_prefix(receiver)
            .and_then(|value| value.strip_prefix('.'))
        else {
            return unresolved("computed-member-unsupported");
        };
        if call.shadowed {
            return unresolved("local-binding-shadowed-this");
        }
        return resolve_private_this_member(path, property, call, resolution);
    }
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
                false,
                resolution,
                export_cache,
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
        if let Some(local_class) = local_class_candidates(path, receiver, resolution) {
            return resolve_static_member(&local_class, property, resolution);
        }
        let matching_imports = imports
            .iter()
            .filter(|import| {
                import.local_name.as_deref() == Some(receiver)
                    && import.kind != "re-export"
                    && import.kind != "side-effect-import"
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
        if import.kind == "namespace-import" {
            return resolve_import_target(path, import, property, true, resolution, export_cache);
        }
        let binding = resolve_import_raw(
            path,
            import,
            import.imported_name.as_deref().unwrap_or("default"),
            resolution,
            export_cache,
        );
        if !binding.namespace_modules.is_empty() {
            let mut outcomes = binding
                .namespace_modules
                .iter()
                .map(|module| {
                    resolve_export(module, property, resolution, &mut Vec::new(), export_cache)
                })
                .collect::<Vec<_>>();
            let mut namespace = combine_outcomes(&mut outcomes);
            namespace.candidates =
                callable_candidates(namespace.candidates, resolution.symbol_kinds);
            if namespace.status == "resolved" && namespace.candidates.is_empty() {
                return unresolved("non-callable-export");
            }
            return namespace;
        }
        if binding.status != "resolved" {
            return binding;
        }
        let class_candidates = binding
            .candidates
            .into_iter()
            .filter(|candidate| {
                resolution
                    .symbol_kinds
                    .get(candidate)
                    .is_some_and(|kind| kind == "class")
            })
            .collect::<Vec<_>>();
        if class_candidates.len() != 1 {
            return if class_candidates.is_empty() {
                unresolved("static-member-on-non-class")
            } else {
                unresolved_with_status("ambiguous-class-binding", "ambiguous")
            };
        }
        return resolve_static_member(&class_candidates, property, resolution);
    }
    unresolved("unsupported-callee-form")
}

fn resolve_private_this_member(
    path: &str,
    property: &str,
    call: &crate::model::TypeScriptCall,
    resolution: &ResolutionContext<'_>,
) -> ResolutionOutcome {
    let Some(owner_name) = call.enclosing_type.as_deref() else {
        return unresolved("this-member-outside-class");
    };
    let owner_id = node_id("symbol", path, &format!("class:{owner_name}"));
    let Some(members) = resolution.members_by_owner.get(&owner_id) else {
        return unresolved("missing-class-member");
    };
    let candidates = members
        .get(property)
        .into_iter()
        .flat_map(|ids| ids.iter())
        .filter(|id| {
            resolution
                .declarations_by_id
                .get(*id)
                .is_some_and(|declaration| {
                    !declaration.static_member
                        && declaration.visibility == "private"
                        && declaration.kind != "constructor"
                })
        })
        .cloned()
        .collect::<Vec<_>>();
    if candidates.len() == 1 {
        return ResolutionOutcome {
            status: "resolved".to_string(),
            reason: "same-class-private-method".to_string(),
            candidates,
            namespace_modules: Vec::new(),
            type_only: false,
        };
    }
    if candidates.len() > 1 {
        return unresolved_with_candidates("ambiguous-private-method", "ambiguous", candidates);
    }
    if members.contains_key(property) {
        unresolved("potentially-polymorphic-this-call")
    } else {
        unresolved("missing-class-member")
    }
}

fn local_class_candidates(
    path: &str,
    name: &str,
    resolution: &ResolutionContext<'_>,
) -> Option<Vec<String>> {
    let candidates = resolution
        .symbols_by_path
        .get(path)
        .and_then(|symbols| symbols.get(name))?
        .iter()
        .filter(|candidate| {
            resolution
                .symbol_kinds
                .get(*candidate)
                .is_some_and(|kind| kind == "class")
        })
        .cloned()
        .collect::<Vec<_>>();
    Some(candidates)
}

fn resolve_static_member(
    classes: &[String],
    property: &str,
    resolution: &ResolutionContext<'_>,
) -> ResolutionOutcome {
    let mut candidates = Vec::new();
    let mut saw_member = false;
    for class in classes {
        if let Some(members) = resolution.members_by_owner.get(class)
            && let Some(ids) = members.get(property)
        {
            saw_member = true;
            candidates.extend(
                ids.iter()
                    .filter(|id| {
                        resolution
                            .declarations_by_id
                            .get(*id)
                            .is_some_and(|declaration| declaration.static_member)
                    })
                    .cloned(),
            );
        }
    }
    candidates.sort();
    candidates.dedup();
    if candidates.len() == 1 {
        return ResolutionOutcome {
            status: "resolved".to_string(),
            reason: "static-class-method".to_string(),
            candidates,
            namespace_modules: Vec::new(),
            type_only: false,
        };
    }
    if candidates.len() > 1 {
        return unresolved_with_candidates("ambiguous-static-method", "ambiguous", candidates);
    }
    unresolved(if saw_member {
        "static-member-not-found"
    } else {
        "missing-class-member"
    })
}

fn resolve_constructor_call(
    path: &str,
    callee: &str,
    imports: &[crate::model::TypeScriptImport],
    call: &crate::model::TypeScriptCall,
    resolution: &ResolutionContext<'_>,
    export_cache: &mut BTreeMap<(String, String), ResolutionOutcome>,
) -> ResolutionOutcome {
    if call.shadowed {
        return unresolved("local-binding-shadowed-constructor");
    }
    let mut class_candidates = local_class_candidates(path, callee, resolution).unwrap_or_default();
    if class_candidates.len() > 1 {
        return unresolved_with_candidates(
            "ambiguous-class-binding",
            "ambiguous",
            class_candidates,
        );
    }
    if class_candidates.is_empty() {
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
        let Some(import) = matching_imports.first().copied() else {
            return unresolved("missing-class-binding");
        };
        if import.type_only {
            return unresolved("type-only-constructor-binding");
        }
        let target = resolve_import_target(
            path,
            import,
            import.imported_name.as_deref().unwrap_or("default"),
            false,
            resolution,
            export_cache,
        );
        if target.status != "resolved" {
            return target;
        }
        class_candidates = target
            .candidates
            .into_iter()
            .filter(|candidate| {
                resolution
                    .symbol_kinds
                    .get(candidate)
                    .is_some_and(|kind| kind == "class")
            })
            .collect();
    }
    if class_candidates.len() != 1 {
        return if class_candidates.is_empty() {
            unresolved("constructor-target-not-class")
        } else {
            unresolved_with_candidates("ambiguous-class-binding", "ambiguous", class_candidates)
        };
    }
    let class = &class_candidates[0];
    let Some(members) = resolution.members_by_owner.get(class) else {
        return ResolutionOutcome {
            status: "resolved".to_string(),
            reason: "implicit-default-constructor".to_string(),
            candidates: vec![class.clone()],
            namespace_modules: Vec::new(),
            type_only: false,
        };
    };
    let constructors = members
        .get("constructor")
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|candidate| {
            resolution
                .declarations_by_id
                .get(candidate)
                .is_some_and(|declaration| declaration.kind == "constructor")
        })
        .collect::<Vec<_>>();
    if constructors.len() == 1 {
        ResolutionOutcome {
            status: "resolved".to_string(),
            reason: "explicit-constructor".to_string(),
            candidates: constructors,
            namespace_modules: Vec::new(),
            type_only: false,
        }
    } else if constructors.len() > 1 {
        unresolved_with_candidates("ambiguous-constructor", "ambiguous", constructors)
    } else {
        ResolutionOutcome {
            status: "resolved".to_string(),
            reason: "implicit-default-constructor".to_string(),
            candidates: vec![class.clone()],
            namespace_modules: Vec::new(),
            type_only: false,
        }
    }
}

fn resolve_import_target(
    current_path: &str,
    import: &crate::model::TypeScriptImport,
    imported_name: &str,
    member_call: bool,
    resolution: &ResolutionContext<'_>,
    export_cache: &mut BTreeMap<(String, String), ResolutionOutcome>,
) -> ResolutionOutcome {
    if !import.specifier.starts_with('.')
        && resolution.module_resolver.basis.root_config.is_none()
        && is_non_relative_alias(&import.specifier, resolution.known_paths)
    {
        return unresolved("non-relative-path-alias");
    }
    let Some(target_path) = resolve_import_module(current_path, &import.specifier, resolution)
    else {
        return resolve_external_or_unresolved(&import.specifier, resolution);
    };
    let mut result = if !member_call || import.kind == "namespace-import" {
        resolve_export(
            &target_path,
            imported_name,
            resolution,
            &mut Vec::new(),
            export_cache,
        )
    } else {
        let binding = resolve_export(
            &target_path,
            import.imported_name.as_deref().unwrap_or("default"),
            resolution,
            &mut Vec::new(),
            export_cache,
        );
        if binding.namespace_modules.is_empty() {
            unresolved("unsupported-member-call")
        } else {
            let mut outcomes = binding
                .namespace_modules
                .iter()
                .map(|module| {
                    resolve_export(
                        module,
                        imported_name,
                        resolution,
                        &mut Vec::new(),
                        export_cache,
                    )
                })
                .collect::<Vec<_>>();
            combine_outcomes(&mut outcomes)
        }
    };
    if !result.namespace_modules.is_empty() && result.candidates.is_empty() {
        result = unresolved("namespace-called-as-function");
    }
    if result.type_only {
        return unresolved("type-only-binding");
    }
    result.candidates = callable_candidates(result.candidates, resolution.symbol_kinds);
    if result.status == "resolved" && result.candidates.is_empty() {
        result = unresolved("non-callable-export");
    } else if result.status == "ambiguous" && result.candidates.is_empty() {
        result = unresolved_with_status("non-callable-export", "ambiguous");
    }
    if result.status == "resolved"
        && matches!(
            result.reason.as_str(),
            "re-export-binding" | "local-export-binding"
        )
    {
        let through_reexport = result.reason == "re-export-binding";
        result.reason = match (import.kind.as_str(), through_reexport) {
            ("named-import", true) => "named-import-through-reexport".to_string(),
            ("default-import", true) => "default-import-through-reexport".to_string(),
            ("namespace-import", true) => "namespace-import-through-reexport".to_string(),
            ("named-import", false) => "named-import-binding".to_string(),
            ("default-import", false) => "default-import-binding".to_string(),
            ("namespace-import", false) => "namespace-import-binding".to_string(),
            (_, true) => "direct-import-through-reexport".to_string(),
            _ => "direct-import-binding".to_string(),
        };
    }
    result
}

fn resolve_import_module(
    current_path: &str,
    specifier: &str,
    resolution: &ResolutionContext<'_>,
) -> Option<String> {
    if specifier.starts_with('.') {
        return resolve_relative(current_path, specifier, resolution.known_paths);
    }
    match resolution
        .module_resolver
        .resolve_non_relative(specifier, resolution.known_paths)
    {
        NonRelativeResolution::Local(path) => Some(path),
        NonRelativeResolution::External | NonRelativeResolution::Unresolved(_) => None,
    }
}

fn resolve_external_or_unresolved(
    specifier: &str,
    resolution: &ResolutionContext<'_>,
) -> ResolutionOutcome {
    if specifier.starts_with('.') {
        return unresolved("missing-relative-module");
    }
    if resolution.module_resolver.basis.root_config.is_none()
        && is_non_relative_alias(specifier, resolution.known_paths)
    {
        return unresolved("non-relative-path-alias");
    }
    match resolution
        .module_resolver
        .resolve_non_relative(specifier, resolution.known_paths)
    {
        NonRelativeResolution::External => ResolutionOutcome {
            status: "external".to_string(),
            reason: "external-module".to_string(),
            candidates: resolution
                .external_ids
                .get(specifier)
                .cloned()
                .into_iter()
                .collect(),
            namespace_modules: Vec::new(),
            type_only: false,
        },
        NonRelativeResolution::Unresolved(reason)
            if is_non_relative_alias(specifier, resolution.known_paths)
                || !reason.starts_with("tsconfig-resolution-") =>
        {
            unresolved(&reason)
        }
        NonRelativeResolution::Unresolved(_) => ResolutionOutcome {
            status: "external".to_string(),
            reason: "external-module".to_string(),
            candidates: resolution
                .external_ids
                .get(specifier)
                .cloned()
                .into_iter()
                .collect(),
            namespace_modules: Vec::new(),
            type_only: false,
        },
        NonRelativeResolution::Local(_) => unresolved("module-resolution-inconsistent"),
    }
}

fn resolve_export(
    path: &str,
    name: &str,
    resolution: &ResolutionContext<'_>,
    stack: &mut Vec<(String, String)>,
    cache: &mut BTreeMap<(String, String), ResolutionOutcome>,
) -> ResolutionOutcome {
    let key = (path.to_string(), name.to_string());
    if stack.iter().any(|item| item == &key) {
        return unresolved("re-export-cycle");
    }
    if stack.len() >= MAX_REEXPORT_DEPTH {
        return unresolved("re-export-depth-capped");
    }
    if let Some(cached) = cache.get(&key) {
        return cached.clone();
    }
    stack.push((path.to_string(), name.to_string()));
    let Some(exports) = resolution.module_exports.get(path) else {
        stack.pop();
        let result = unresolved("missing-relative-module");
        if cache.len() < MAX_RESOLUTION_RECORDS {
            cache.insert(key, result.clone());
        }
        return result;
    };
    let explicit = exports.get(name).cloned().unwrap_or_default();
    let explicit_is_empty = explicit.is_empty();
    let bindings = if explicit_is_empty {
        exports.get("*").cloned().unwrap_or_default()
    } else {
        explicit
    };
    let has_star = explicit_is_empty && !bindings.is_empty();
    let mut candidates = Vec::new();
    let mut namespace_modules = Vec::new();
    let mut saw_external = false;
    let mut reasons = Vec::new();
    let mut traversed_reexport = false;
    let mut type_only = false;
    for binding in bindings {
        match binding {
            ExportBinding::Local(ids) => candidates.extend(ids),
            ExportBinding::TypeOnly { ids } => {
                type_only = true;
                candidates.extend(ids);
                reasons.push("type-only-export".to_string());
            }
            ExportBinding::Namespace {
                source,
                type_only: binding_type_only,
            } => {
                type_only |= binding_type_only;
                traversed_reexport = true;
                match resolve_import_module(path, &source, resolution) {
                    Some(target) => namespace_modules.push(target),
                    None => {
                        let outcome = resolve_external_or_unresolved(&source, resolution);
                        saw_external |= outcome.status == "external";
                        reasons.push(outcome.reason);
                    }
                }
            }
            ExportBinding::Forward {
                source,
                name,
                type_only: binding_type_only,
            } => {
                type_only |= binding_type_only;
                traversed_reexport = true;
                match resolve_import_module(path, &source, resolution) {
                    Some(target) => {
                        let outcome = resolve_export(&target, &name, resolution, stack, cache);
                        candidates.extend(outcome.candidates);
                        namespace_modules.extend(outcome.namespace_modules);
                        type_only |= outcome.type_only;
                        reasons.push(outcome.reason);
                    }
                    None => {
                        let outcome = resolve_external_or_unresolved(&source, resolution);
                        saw_external |= outcome.status == "external";
                        reasons.push(outcome.reason);
                    }
                }
            }
            ExportBinding::Star {
                source,
                type_only: binding_type_only,
            } => {
                type_only |= binding_type_only;
                traversed_reexport = true;
                if name == "default" {
                    continue;
                }
                match resolve_import_module(path, &source, resolution) {
                    Some(target) => {
                        let outcome = resolve_export(&target, name, resolution, stack, cache);
                        candidates.extend(outcome.candidates);
                        namespace_modules.extend(outcome.namespace_modules);
                        type_only |= outcome.type_only;
                        reasons.push(outcome.reason);
                    }
                    None => {
                        let outcome = resolve_external_or_unresolved(&source, resolution);
                        saw_external |= outcome.status == "external";
                        reasons.push(outcome.reason);
                    }
                }
            }
        }
    }
    stack.pop();
    candidates.sort();
    candidates.dedup();
    namespace_modules.sort();
    namespace_modules.dedup();
    let result = if candidates.is_empty() && namespace_modules.is_empty() {
        if saw_external {
            ResolutionOutcome {
                status: "external".to_string(),
                reason: "external-reexport".to_string(),
                candidates: Vec::new(),
                namespace_modules: Vec::new(),
                type_only,
            }
        } else {
            let reason = if reasons.iter().any(|reason| reason == "type-only-export") {
                "type-only-export"
            } else if has_star {
                if reasons
                    .iter()
                    .any(|reason| reason == "re-export-depth-capped")
                {
                    "re-export-depth-capped"
                } else if reasons.iter().any(|reason| reason == "re-export-cycle") {
                    "re-export-cycle"
                } else {
                    "missing-reexport"
                }
            } else if reasons
                .iter()
                .any(|reason| reason == "re-export-depth-capped")
            {
                "re-export-depth-capped"
            } else if reasons.iter().any(|reason| reason == "re-export-cycle") {
                "re-export-cycle"
            } else {
                "missing-export"
            };
            unresolved(reason)
        }
    } else if candidates.len() > 1 {
        ResolutionOutcome {
            status: "ambiguous".to_string(),
            reason: if has_star {
                "ambiguous-star-reexport".to_string()
            } else {
                "ambiguous-reexport".to_string()
            },
            candidates,
            namespace_modules,
            type_only,
        }
    } else if candidates.len() == 1 {
        ResolutionOutcome {
            status: "resolved".to_string(),
            reason: if traversed_reexport
                || reasons.iter().any(|reason| reason == "re-export-binding")
                || has_star
            {
                "re-export-binding".to_string()
            } else {
                "local-export-binding".to_string()
            },
            candidates,
            namespace_modules,
            type_only,
        }
    } else {
        ResolutionOutcome {
            status: "resolved".to_string(),
            reason: "namespace-reexport".to_string(),
            candidates: Vec::new(),
            namespace_modules,
            type_only,
        }
    };
    if cache.len() < MAX_RESOLUTION_RECORDS {
        cache.insert(key, result.clone());
    }
    result
}

fn combine_outcomes(outcomes: &mut [ResolutionOutcome]) -> ResolutionOutcome {
    let mut candidates = outcomes
        .iter()
        .flat_map(|outcome| outcome.candidates.iter().cloned())
        .collect::<Vec<_>>();
    candidates.sort();
    candidates.dedup();
    let mut namespaces = outcomes
        .iter()
        .flat_map(|outcome| outcome.namespace_modules.iter().cloned())
        .collect::<Vec<_>>();
    namespaces.sort();
    namespaces.dedup();
    let type_only = outcomes.iter().any(|outcome| outcome.type_only);
    if candidates.len() > 1 {
        return ResolutionOutcome {
            status: "ambiguous".to_string(),
            reason: "ambiguous-namespace-member".to_string(),
            candidates,
            namespace_modules: namespaces,
            type_only,
        };
    }
    if candidates.len() == 1 {
        return ResolutionOutcome {
            status: "resolved".to_string(),
            reason: "namespace-member-binding".to_string(),
            candidates,
            namespace_modules: namespaces,
            type_only,
        };
    }
    if outcomes.iter().any(|outcome| outcome.status == "external") {
        return ResolutionOutcome {
            status: "external".to_string(),
            reason: "external-reexport".to_string(),
            candidates: Vec::new(),
            namespace_modules: namespaces,
            type_only,
        };
    }
    let reason = outcomes
        .iter()
        .find(|outcome| outcome.status == "unresolved")
        .map(|outcome| outcome.reason.clone())
        .unwrap_or_else(|| "missing-export".to_string());
    unresolved(&reason)
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
            namespace_modules: Vec::new(),
            type_only: false,
        },
        [] => unresolved("unresolved-identifier"),
        _ => ResolutionOutcome {
            status: "ambiguous".to_string(),
            reason: "ambiguous-symbol".to_string(),
            candidates,
            namespace_modules: Vec::new(),
            type_only: false,
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
        namespace_modules: Vec::new(),
        type_only: false,
    }
}

fn unresolved_with_candidates(
    reason: &str,
    status: &str,
    mut candidates: Vec<String>,
) -> ResolutionOutcome {
    candidates.sort();
    candidates.dedup();
    ResolutionOutcome {
        status: status.to_string(),
        reason: reason.to_string(),
        candidates,
        namespace_modules: Vec::new(),
        type_only: false,
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
                            node.path.as_deref().is_some_and(|path| {
                                node.id == node_id("symbol", path, &declaration.qualified_name)
                            })
                        })
                        .map(|declaration| {
                            format!(
                                "{}:{}:{}:{}:{}",
                                declaration.ast_fingerprint,
                                declaration.exported,
                                declaration.visibility,
                                declaration.static_member,
                                declaration.abstract_member
                            )
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
                        node.path.as_deref().is_some_and(|path| {
                            node.id == node_id("symbol", path, &declaration.qualified_name)
                        }) && declaration.exported
                    })
                });
            let mut declaration_fingerprints = declaration_fingerprints;
            declaration_fingerprints.sort();
            format!(
                "symbol\0{}\0{}\0{}\0exported={exported}\0ast={}",
                node.kind,
                node.path.as_deref().unwrap_or_default(),
                node.id,
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
    resolve_module_relative(current, specifier, known_paths)
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
        assert!(graph.edges.iter().any(|edge| {
            edge.from == visible.id && edge.to == secret.id && edge.kind == "calls"
        }));
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
        assert!(graph.edges.iter().any(|edge| {
            edge.from == checkout.id && edge.to == charge.id && edge.kind == "calls"
        }));
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
            cycle_consumer.resolution_records.iter().any(|record| {
                record.status == "unresolved" && record.reason == "re-export-cycle"
            })
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
}
