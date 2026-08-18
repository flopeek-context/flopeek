//! Graph snapshot assembly orchestration.

use super::*;

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
    let project_id = project_id(root);
    let flow_seed = flow::derive(root, &project_id, &files, &nodes, &edges)?;
    nodes.extend(flow_seed.entry_nodes);
    edges.extend(flow_seed.entry_edges);
    edges.sort_by(|left, right| {
        left.from
            .cmp(&right.from)
            .then_with(|| left.to.cmp(&right.to))
            .then_with(|| left.kind.cmp(&right.kind))
            .then_with(|| left.evidence.cmp(&right.evidence))
    });
    edges.dedup();
    assign_node_fingerprints(&mut nodes, &edges, &facts);
    let flow_derivation = flow::derive(root, &project_id, &files, &nodes, &edges)?;
    let entry_evidence = flow_derivation.entry_evidence;
    let related_test_evidence = flow_derivation.related_test_evidence;
    let flows = flow_derivation.flows;
    if flow_derivation.truncated {
        truncated = true;
        omissions.extend(flow_derivation.omissions);
    }
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
        entry_effective_fingerprint: &entry_evidence.effective_fingerprint,
        entry_status: &entry_evidence.status,
        entry_records: &entry_evidence.records,
        related_test_evidence: &related_test_evidence,
        flows: &flows,
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
            entry_evidence,
            related_test_evidence,
            flows,
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
    entry_effective_fingerprint: &'a str,
    entry_status: &'a str,
    entry_records: &'a [crate::model::EntryRecord],
    related_test_evidence: &'a crate::model::RelatedTestEvidence,
    flows: &'a [crate::model::ContextFlow],
}
