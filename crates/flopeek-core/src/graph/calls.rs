//! Caller, heritage, and direct call resolution.

use super::*;

pub struct ResolutionContext<'a> {
    pub(super) file_ids: &'a BTreeMap<String, String>,
    pub(super) symbols_by_path: &'a BTreeMap<String, BTreeMap<String, Vec<String>>>,
    pub(super) members_by_owner: &'a BTreeMap<String, BTreeMap<String, Vec<String>>>,
    pub(super) declarations_by_id: &'a BTreeMap<String, TypeScriptDeclaration>,
    pub(super) symbol_kinds: &'a BTreeMap<String, String>,
    pub(super) module_exports: &'a ModuleExports,
    pub(super) known_paths: &'a BTreeSet<String>,
    pub(super) external_ids: &'a BTreeMap<String, String>,
    pub(super) module_resolver: &'a ModuleResolver,
}

pub fn resolve_calls(
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

pub fn resolve_heritage(
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
pub struct ResolutionOutcome {
    pub(super) status: String,
    pub(super) reason: String,
    pub(super) candidates: Vec<String>,
    pub(super) namespace_modules: Vec<String>,
    pub(super) type_only: bool,
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
