//! Export table and module-local binding evidence.

use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportBinding {
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

pub type ModuleExports = BTreeMap<String, BTreeMap<String, Vec<ExportBinding>>>;

pub fn build_module_exports(
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
