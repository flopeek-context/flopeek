//! Import targets, re-exports, and bounded resolution outcomes.

use super::*;

pub fn resolve_import_target(
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

pub fn resolve_import_module(
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

pub fn resolve_external_or_unresolved(
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

pub fn resolve_export(
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

pub fn combine_outcomes(outcomes: &mut [ResolutionOutcome]) -> ResolutionOutcome {
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

pub fn callable_candidates(
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

pub fn unique_or_unresolved(candidates: Vec<String>, reason: &str) -> ResolutionOutcome {
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

pub fn unresolved(reason: &str) -> ResolutionOutcome {
    unresolved_with_status(reason, "unresolved")
}

pub fn unresolved_with_status(reason: &str, status: &str) -> ResolutionOutcome {
    ResolutionOutcome {
        status: status.to_string(),
        reason: reason.to_string(),
        candidates: Vec::new(),
        namespace_modules: Vec::new(),
        type_only: false,
    }
}

pub fn unresolved_with_candidates(
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
