//! Configuration loading and effective compiler-option merging.

use super::paths::{join_relative, normalize_config_path};
use super::*;
use crate::discovery::{normalize_relative_path, read_source};
use crate::model::ModuleResolutionConfigFile;
use jsonc_parser::parse_to_serde_value;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct EffectiveConfig {
    pub(super) base_url: Option<ConfiguredPath>,
    pub(super) paths: BTreeMap<String, PathMapping>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ConfiguredPath {
    pub(super) path: String,
    pub(super) origin: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PathMapping {
    pub(super) pattern: String,
    pub(super) targets: Vec<ConfiguredPath>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ConfigDocument {
    pub(super) path: String,
    pub(super) bytes: usize,
    pub(super) hash: String,
    pub(super) extends: Option<String>,
    pub(super) compiler_options: RawCompilerOptions,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
struct RawConfig {
    #[serde(rename = "extends")]
    extends: Option<String>,
    #[serde(rename = "compilerOptions", default)]
    compiler_options: RawCompilerOptions,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub(super) struct RawCompilerOptions {
    #[serde(rename = "baseUrl")]
    base_url: Option<String>,
    #[serde(default)]
    paths: Option<BTreeMap<String, Vec<String>>>,
}

pub(super) fn load_chain(
    root: &Path,
    path: &str,
    documents: &mut BTreeMap<String, ConfigDocument>,
    stack: &mut Vec<String>,
    total_bytes: &mut usize,
) -> Result<(), String> {
    if stack.iter().any(|item| item == path) {
        return Err(format!("tsconfig-extends-cycle:{path}"));
    }
    if documents.contains_key(path) {
        return Ok(());
    }
    if documents.len() >= MAX_CONFIG_FILES {
        return Err(format!("tsconfig-files-capped-at-{MAX_CONFIG_FILES}"));
    }
    let bytes = read_source(root, path).map_err(|error| format!("tsconfig-read-failed:{error}"))?;
    let hash = blake3::hash(&bytes).to_hex().to_string();
    *total_bytes = total_bytes.saturating_add(bytes.len());
    if *total_bytes > MAX_CONFIG_BYTES {
        documents.insert(
            path.to_string(),
            ConfigDocument {
                path: path.to_string(),
                bytes: bytes.len(),
                hash,
                extends: None,
                compiler_options: RawCompilerOptions::default(),
            },
        );
        return Err(format!("tsconfig-bytes-capped-at-{MAX_CONFIG_BYTES}"));
    }
    let text = match std::str::from_utf8(&bytes) {
        Ok(text) => text,
        Err(_) => {
            documents.insert(
                path.to_string(),
                ConfigDocument {
                    path: path.to_string(),
                    bytes: bytes.len(),
                    hash,
                    extends: None,
                    compiler_options: RawCompilerOptions::default(),
                },
            );
            return Err(format!("tsconfig-not-utf8:{path}"));
        }
    };
    let raw = match parse_to_serde_value::<RawConfig>(text, &Default::default()) {
        Ok(raw) => raw,
        Err(error) => {
            documents.insert(
                path.to_string(),
                ConfigDocument {
                    path: path.to_string(),
                    bytes: bytes.len(),
                    hash,
                    extends: None,
                    compiler_options: RawCompilerOptions::default(),
                },
            );
            return Err(format!("tsconfig-invalid-jsonc:{path}:{error}"));
        }
    };
    let document = ConfigDocument {
        path: path.to_string(),
        bytes: bytes.len(),
        hash,
        extends: raw.extends.clone(),
        compiler_options: raw.compiler_options,
    };
    documents.insert(path.to_string(), document.clone());
    stack.push(path.to_string());
    let result = if let Some(extends) = document.extends.as_deref() {
        let parent = resolve_extends_path(path, extends)?;
        load_chain(root, &parent, documents, stack, total_bytes)
    } else {
        Ok(())
    };
    stack.pop();
    result
}

fn resolve_extends_path(current: &str, extends: &str) -> Result<String, String> {
    if !extends.starts_with('.') {
        return Err("tsconfig-package-extends-unsupported".to_string());
    }
    let parent = join_relative(
        Path::new(current)
            .parent()
            .and_then(Path::to_str)
            .unwrap_or(""),
        extends,
    )
    .map_err(|reason| {
        if reason == "tsconfig-path-escapes-repository" {
            "tsconfig-extends-escapes-repository".to_string()
        } else {
            reason
        }
    })?;
    let normalized = normalize_relative_path(Path::new(&parent))
        .map_err(|_| "tsconfig-extends-escapes-repository".to_string())?;
    if Path::new(&normalized).extension().is_none() {
        Ok(format!("{normalized}.json"))
    } else {
        Ok(normalized)
    }
}

pub fn config_extends(path: &str, bytes: &[u8]) -> Result<Option<String>, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| format!("tsconfig-not-utf8:{path}"))?;
    let raw = parse_to_serde_value::<RawConfig>(text, &Default::default())
        .map_err(|error| format!("tsconfig-invalid-jsonc:{path}:{}", error))?;
    raw.extends
        .as_deref()
        .map(|extends| resolve_extends_path(path, extends))
        .transpose()
}

pub(super) fn extends_order(
    path: &str,
    documents: &BTreeMap<String, ConfigDocument>,
) -> Result<Vec<String>, String> {
    let mut result = Vec::new();
    let mut visited = Vec::new();
    fn visit(
        path: &str,
        documents: &BTreeMap<String, ConfigDocument>,
        visited: &mut Vec<String>,
        result: &mut Vec<String>,
    ) -> Result<(), String> {
        if visited.iter().any(|item| item == path) {
            return Err(format!("tsconfig-extends-cycle:{path}"));
        }
        let Some(document) = documents.get(path) else {
            return Ok(());
        };
        visited.push(path.to_string());
        if let Some(extends) = document.extends.as_deref() {
            let parent = resolve_extends_path(path, extends)?;
            visit(&parent, documents, visited, result)?;
        }
        visited.pop();
        result.push(path.to_string());
        Ok(())
    }
    visit(path, documents, &mut visited, &mut result)?;
    Ok(result)
}

pub(super) fn merge_document(
    _root: &Path,
    document: &ConfigDocument,
    effective: &mut EffectiveConfig,
) -> Result<(), String> {
    let config_dir = Path::new(&document.path)
        .parent()
        .and_then(Path::to_str)
        .unwrap_or("");
    if let Some(base_url) = document.compiler_options.base_url.as_deref() {
        let path = normalize_config_path(config_dir, base_url)?;
        effective.base_url = Some(ConfiguredPath {
            path,
            origin: document.path.clone(),
        });
    }
    if let Some(paths) = document.compiler_options.paths.as_ref() {
        if paths.len() > MAX_PATH_MAPPINGS {
            return Err(format!(
                "tsconfig-path-mappings-capped-at-{MAX_PATH_MAPPINGS}"
            ));
        }
        let mut total_targets = 0_usize;
        let mut mappings = BTreeMap::new();
        for (pattern, targets) in paths {
            if pattern.matches('*').count() > 1 {
                return Err(format!("tsconfig-path-pattern-unsupported:{pattern}"));
            }
            if targets.is_empty() {
                return Err(format!("tsconfig-path-targets-empty:{pattern}"));
            }
            total_targets = total_targets.saturating_add(targets.len());
            if total_targets > MAX_PATH_TARGETS {
                return Err(format!(
                    "tsconfig-path-targets-capped-at-{MAX_PATH_TARGETS}"
                ));
            }
            let base = effective
                .base_url
                .as_ref()
                .map(|value| value.path.as_str())
                .unwrap_or(config_dir);
            let mut resolved_targets = Vec::with_capacity(targets.len());
            for target in targets {
                resolved_targets.push(ConfiguredPath {
                    path: normalize_config_path(base, target)?,
                    origin: document.path.clone(),
                });
            }
            mappings.insert(
                pattern.clone(),
                PathMapping {
                    pattern: pattern.clone(),
                    targets: resolved_targets,
                },
            );
        }
        effective.paths = mappings;
    }
    Ok(())
}

pub(super) fn config_file(document: &ConfigDocument) -> ModuleResolutionConfigFile {
    ModuleResolutionConfigFile {
        path: document.path.clone(),
        bytes: document.bytes as u64,
        hash: document.hash.clone(),
    }
}

pub(super) fn complete_basis(
    root_config: Option<String>,
    mut files: Vec<ModuleResolutionConfigFile>,
    effective: &EffectiveConfig,
) -> ModuleResolutionBasis {
    files.sort_by(|left, right| left.path.cmp(&right.path));
    let exact_input = serde_json::to_vec(&files).unwrap_or_default();
    let exact_fingerprint = blake3::hash(&exact_input).to_hex().to_string();
    let effective_value = (
        effective
            .base_url
            .as_ref()
            .map(|value| (&value.path, &value.origin)),
        effective
            .paths
            .iter()
            .map(|(pattern, mapping)| {
                (
                    pattern,
                    mapping
                        .targets
                        .iter()
                        .map(|target| (&target.path, &target.origin))
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<Vec<_>>(),
    );
    let effective_fingerprint =
        blake3::hash(&serde_json::to_vec(&effective_value).unwrap_or_default())
            .to_hex()
            .to_string();
    ModuleResolutionBasis {
        schema_version: MODULE_RESOLUTION_SCHEMA.to_string(),
        status: "complete".to_string(),
        root_config,
        config_files: files,
        exact_fingerprint,
        effective_fingerprint,
        limitations: vec![
            "nested-tsconfig-project-selection-unsupported".to_string(),
            "package-and-project-reference-resolution-unsupported".to_string(),
        ],
        omissions: Vec::new(),
    }
}
