//! Bounded, repository-local TypeScript module configuration resolution.
//!
//! This module intentionally implements only the deterministic subset needed by the
//! product contract: one root `tsconfig.json`, local `extends`, `baseUrl`, and
//! `paths`.  It never follows packages, project references, or machine-local paths.

use crate::discovery::{normalize_relative_path, read_source};
use crate::model::{ModuleResolutionBasis, ModuleResolutionConfigFile};
use jsonc_parser::parse_to_serde_value;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;

pub const MODULE_RESOLUTION_SCHEMA: &str = "flopeek-typescript-module-resolution/v1";
pub const MAX_CONFIG_FILES: usize = 16;
pub const MAX_CONFIG_BYTES: usize = 1024 * 1024;
pub const MAX_PATH_MAPPINGS: usize = 1_000;
pub const MAX_PATH_TARGETS: usize = 10_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleResolver {
    pub basis: ModuleResolutionBasis,
    effective: Option<EffectiveConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EffectiveConfig {
    base_url: Option<ConfiguredPath>,
    paths: BTreeMap<String, PathMapping>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ConfiguredPath {
    path: String,
    origin: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PathMapping {
    pattern: String,
    targets: Vec<ConfiguredPath>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ConfigDocument {
    path: String,
    bytes: usize,
    hash: String,
    extends: Option<String>,
    compiler_options: RawCompilerOptions,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
struct RawConfig {
    #[serde(rename = "extends")]
    extends: Option<String>,
    #[serde(rename = "compilerOptions", default)]
    compiler_options: RawCompilerOptions,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
struct RawCompilerOptions {
    #[serde(rename = "baseUrl")]
    base_url: Option<String>,
    #[serde(default)]
    paths: Option<BTreeMap<String, Vec<String>>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NonRelativeResolution {
    Local(String),
    External,
    Unresolved(String),
}

impl ModuleResolver {
    pub fn load(root: &Path) -> Self {
        let config_path = "tsconfig.json";
        let absolute = root.join(config_path);
        if !absolute.is_file() {
            return Self {
                basis: complete_basis(
                    None,
                    Vec::new(),
                    &EffectiveConfig {
                        base_url: None,
                        paths: BTreeMap::new(),
                    },
                ),
                effective: Some(EffectiveConfig {
                    base_url: None,
                    paths: BTreeMap::new(),
                }),
            };
        }

        let mut documents = BTreeMap::new();
        let mut stack = Vec::new();
        let mut total_bytes = 0_usize;
        match load_chain(
            root,
            config_path,
            &mut documents,
            &mut stack,
            &mut total_bytes,
        ) {
            Ok(()) => {
                let mut effective = EffectiveConfig {
                    base_url: None,
                    paths: BTreeMap::new(),
                };
                let mut ordered = documents.keys().cloned().collect::<Vec<_>>();
                ordered.sort();
                // The documents map is sorted for deterministic manifests, while the
                // effective merge must follow base-to-child extends order.
                let chain = extends_order(config_path, &documents).unwrap_or_default();
                for path in chain {
                    let Some(document) = documents.get(&path) else {
                        continue;
                    };
                    if let Err(reason) = merge_document(root, document, &mut effective) {
                        return Self::unavailable(
                            documents.values().map(config_file).collect(),
                            reason,
                        );
                    }
                }
                let files = ordered
                    .iter()
                    .filter_map(|path| documents.get(path).map(config_file))
                    .collect::<Vec<_>>();
                Self {
                    basis: complete_basis(Some(config_path.to_string()), files, &effective),
                    effective: Some(effective),
                }
            }
            Err(reason) => Self::unavailable(documents.values().map(config_file).collect(), reason),
        }
    }

    fn unavailable(files: Vec<ModuleResolutionConfigFile>, reason: String) -> Self {
        let mut files = files;
        files.sort_by(|left, right| left.path.cmp(&right.path));
        let exact_fingerprint = blake3::hash(&serde_json::to_vec(&files).unwrap_or_default())
            .to_hex()
            .to_string();
        Self {
            basis: ModuleResolutionBasis {
                schema_version: MODULE_RESOLUTION_SCHEMA.to_string(),
                status: if reason.contains("capped") {
                    "truncated".to_string()
                } else {
                    "unavailable".to_string()
                },
                root_config: Some("tsconfig.json".to_string()),
                config_files: files,
                exact_fingerprint,
                effective_fingerprint: String::new(),
                limitations: vec![reason],
                omissions: Vec::new(),
            },
            effective: None,
        }
    }

    pub fn resolve_non_relative(
        &self,
        specifier: &str,
        known_paths: &std::collections::BTreeSet<String>,
    ) -> NonRelativeResolution {
        let Some(effective) = self.effective.as_ref() else {
            let reason = if self.basis.status == "truncated" {
                "tsconfig-resolution-truncated"
            } else {
                "tsconfig-resolution-unavailable"
            };
            return NonRelativeResolution::Unresolved(reason.to_string());
        };

        if let Some(mapping) = matching_mapping(specifier, &effective.paths) {
            let wildcard = mapping.pattern.find('*').map(|index| {
                let suffix_len = mapping.pattern.len().saturating_sub(index + 1);
                let end = specifier.len().saturating_sub(suffix_len);
                specifier[index..end].to_string()
            });
            for target in &mapping.targets {
                let candidate = if let Some(value) = wildcard.as_deref() {
                    target.path.replace('*', value)
                } else {
                    target.path.clone()
                };
                if let Some(path) = resolve_known_path(&candidate, known_paths) {
                    return NonRelativeResolution::Local(path);
                }
            }
            return NonRelativeResolution::Unresolved("missing-tsconfig-path-target".to_string());
        }

        if let Some(base_url) = effective.base_url.as_ref()
            && let Ok(candidate) = join_relative(&base_url.path, specifier)
            && let Some(path) = resolve_known_path(&candidate, known_paths)
        {
            return NonRelativeResolution::Local(path);
        }
        NonRelativeResolution::External
    }
}

fn load_chain(
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

fn extends_order(
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

fn merge_document(
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

fn normalize_config_path(base: &str, value: &str) -> Result<String, String> {
    if value.starts_with('/') || value.contains('\\') {
        return Err("tsconfig-path-absolute-or-machine-path".to_string());
    }
    let joined = join_relative(base, value)?;
    if joined.is_empty() {
        return Ok(String::new());
    }
    normalize_relative_path(Path::new(&joined))
        .map_err(|_| "tsconfig-path-escapes-repository".to_string())
}

fn matching_mapping<'a>(
    specifier: &str,
    mappings: &'a BTreeMap<String, PathMapping>,
) -> Option<&'a PathMapping> {
    if let Some(mapping) = mappings.get(specifier) {
        return Some(mapping);
    }
    mappings
        .values()
        .filter(|mapping| {
            let Some(index) = mapping.pattern.find('*') else {
                return false;
            };
            let prefix = &mapping.pattern[..index];
            let suffix = &mapping.pattern[index + 1..];
            specifier.starts_with(prefix)
                && specifier.ends_with(suffix)
                && specifier.len() >= prefix.len() + suffix.len()
        })
        .max_by(|left, right| {
            let left_prefix = left.pattern.find('*').unwrap_or(left.pattern.len());
            let right_prefix = right.pattern.find('*').unwrap_or(right.pattern.len());
            left_prefix
                .cmp(&right_prefix)
                .then_with(|| right.pattern.cmp(&left.pattern))
        })
}

pub fn resolve_known_path(
    base: &str,
    known_paths: &std::collections::BTreeSet<String>,
) -> Option<String> {
    let candidates = [
        base.to_string(),
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

pub fn resolve_relative(
    current: &str,
    specifier: &str,
    known_paths: &std::collections::BTreeSet<String>,
) -> Option<String> {
    if !specifier.starts_with('.') {
        return None;
    }
    let parent = Path::new(current)
        .parent()
        .and_then(Path::to_str)
        .unwrap_or("");
    let base = join_relative(parent, specifier).ok()?;
    resolve_known_path(&base, known_paths)
}

fn join_relative(base: &str, value: &str) -> Result<String, String> {
    if value.starts_with('/') || value.starts_with('\\') || value.contains('\\') {
        return Err("tsconfig-path-absolute-or-machine-path".to_string());
    }
    let mut components = Vec::new();
    for component in base.split('/') {
        if component.is_empty() || component == "." {
            continue;
        }
        if component == ".." {
            if components.pop().is_none() {
                return Err("tsconfig-path-escapes-repository".to_string());
            }
            continue;
        }
        components.push(component.to_string());
    }
    for component in value.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                if components.pop().is_none() {
                    return Err("tsconfig-path-escapes-repository".to_string());
                }
            }
            value if value.contains(':') => {
                return Err("tsconfig-path-absolute-or-machine-path".to_string());
            }
            value => components.push(value.to_string()),
        }
    }
    Ok(components.join("/"))
}

fn config_file(document: &ConfigDocument) -> ModuleResolutionConfigFile {
    ModuleResolutionConfigFile {
        path: document.path.clone(),
        bytes: document.bytes as u64,
        hash: document.hash.clone(),
    }
}

fn complete_basis(
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root() -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!("flopeek-module-resolution-{suffix}"))
    }

    #[test]
    fn resolves_jsonc_paths_and_base_url_with_deterministic_precedence() {
        let root = temp_root();
        fs::create_dir_all(root.join("src/components")).expect("mkdir");
        fs::write(
            root.join("tsconfig.json"),
            r#"{
              // JSONC is intentional.
              "compilerOptions": {
                "baseUrl": ".",
                "paths": {
                  "@app/*": ["src/*"],
                  "@app/components/*": ["missing/*", "src/components/*"],
                },
              },
            }"#,
        )
        .expect("config");
        fs::write(
            root.join("src/components/Button.tsx"),
            "export const Button = 1;",
        )
        .expect("source");
        let resolver = ModuleResolver::load(&root);
        let known = ["src/components/Button.tsx".to_string()]
            .into_iter()
            .collect();
        assert_eq!(
            resolver.resolve_non_relative("@app/components/Button", &known),
            NonRelativeResolution::Local("src/components/Button.tsx".to_string())
        );
        assert_eq!(resolver.basis.status, "complete");
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn local_extends_preserves_base_paths_and_rejects_cycles() {
        let root = temp_root();
        fs::create_dir_all(root.join("src")).expect("mkdir");
        fs::write(
            root.join("base.json"),
            r#"{"compilerOptions":{"baseUrl":".","paths":{"@base/*":["src/*"]}}}"#,
        )
        .expect("base");
        fs::write(
            root.join("tsconfig.json"),
            r#"{"extends":"./base","compilerOptions":{"paths":{"@local/*":["src/*"]}}}"#,
        )
        .expect("config");
        fs::write(root.join("src/item.ts"), "export const item = 1;").expect("source");
        let resolver = ModuleResolver::load(&root);
        let known = ["src/item.ts".to_string()].into_iter().collect();
        assert_eq!(
            resolver.resolve_non_relative("@local/item", &known),
            NonRelativeResolution::Local("src/item.ts".to_string())
        );
        assert_eq!(
            resolver.resolve_non_relative("@base/item", &known),
            NonRelativeResolution::External
        );
        fs::write(root.join("base.json"), r#"{"extends":"./tsconfig"}"#).expect("cycle");
        let cycle = ModuleResolver::load(&root);
        assert_eq!(cycle.basis.status, "unavailable");
        assert!(cycle.basis.limitations[0].contains("tsconfig-extends-cycle"));
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn rejects_repository_escape_invalid_jsonc_and_package_extends() {
        let root = temp_root();
        fs::create_dir_all(&root).expect("mkdir");
        fs::write(root.join("tsconfig.json"), r#"{"extends":"../outside"}"#)
            .expect("escape config");
        let escape = ModuleResolver::load(&root);
        assert_eq!(escape.basis.status, "unavailable");
        assert!(
            escape
                .basis
                .limitations
                .iter()
                .any(|reason| reason == "tsconfig-extends-escapes-repository")
        );

        fs::write(root.join("tsconfig.json"), r#"{"compilerOptions": {"#).expect("invalid config");
        let invalid = ModuleResolver::load(&root);
        assert_eq!(invalid.basis.status, "unavailable");
        assert_eq!(invalid.basis.config_files.len(), 1);
        assert!(
            invalid
                .basis
                .limitations
                .iter()
                .any(|reason| reason.starts_with("tsconfig-invalid-jsonc:"))
        );

        fs::write(root.join("tsconfig.json"), r#"{"extends":"shared-config"}"#)
            .expect("package config");
        let package = ModuleResolver::load(&root);
        assert_eq!(package.basis.status, "unavailable");
        assert!(
            package
                .basis
                .limitations
                .iter()
                .any(|reason| reason == "tsconfig-package-extends-unsupported")
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn enforces_config_file_bound_and_all_known_module_extensions() {
        let root = temp_root();
        fs::create_dir_all(&root).expect("mkdir");
        let mut parent = "base15".to_string();
        fs::write(root.join("base15.json"), r#"{"compilerOptions":{}}"#).expect("base");
        for index in (0..15).rev() {
            let current = format!("base{index}");
            fs::write(
                root.join(format!("{current}.json")),
                format!(r#"{{"extends":"./{parent}"}}"#),
            )
            .expect("chain");
            parent = current;
        }
        fs::write(
            root.join("tsconfig.json"),
            format!(r#"{{"extends":"./{parent}"}}"#),
        )
        .expect("root config");
        let capped = ModuleResolver::load(&root);
        assert_eq!(capped.basis.status, "truncated");
        assert!(
            capped
                .basis
                .limitations
                .iter()
                .any(|reason| reason.contains("tsconfig-files-capped"))
        );

        let declaration_path = ["src/types.d.ts".to_string()].into_iter().collect();
        assert_eq!(
            resolve_known_path("src/types", &declaration_path),
            Some("src/types.d.ts".to_string())
        );
        let index_path = ["src/components/index.tsx".to_string()]
            .into_iter()
            .collect();
        assert_eq!(
            resolve_known_path("src/components", &index_path),
            Some("src/components/index.tsx".to_string())
        );
        fs::remove_dir_all(root).expect("cleanup");
    }
}
