//! Bounded, repository-local TypeScript module configuration resolution.
//!
//! The facade owns the public resolver contract; configuration loading and
//! path matching live in focused sibling modules.

use crate::model::ModuleResolutionBasis;
use std::collections::BTreeMap;
use std::path::Path;

mod config;
mod paths;
#[cfg(test)]
mod tests;

use config::EffectiveConfig;

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
                basis: config::complete_basis(
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
        match config::load_chain(
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
                let chain = config::extends_order(config_path, &documents).unwrap_or_default();
                for path in chain {
                    let Some(document) = documents.get(&path) else {
                        continue;
                    };
                    if let Err(reason) = config::merge_document(root, document, &mut effective) {
                        return Self::unavailable(
                            documents.values().map(config::config_file).collect(),
                            reason,
                        );
                    }
                }
                let files = ordered
                    .iter()
                    .filter_map(|path| documents.get(path).map(config::config_file))
                    .collect::<Vec<_>>();
                Self {
                    basis: config::complete_basis(Some(config_path.to_string()), files, &effective),
                    effective: Some(effective),
                }
            }
            Err(reason) => Self::unavailable(
                documents.values().map(config::config_file).collect(),
                reason,
            ),
        }
    }

    fn unavailable(files: Vec<crate::model::ModuleResolutionConfigFile>, reason: String) -> Self {
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

        if let Some(mapping) = paths::matching_mapping(specifier, &effective.paths) {
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
                if let Some(path) = paths::resolve_known_path(&candidate, known_paths) {
                    return NonRelativeResolution::Local(path);
                }
            }
            return NonRelativeResolution::Unresolved("missing-tsconfig-path-target".to_string());
        }

        if let Some(base_url) = effective.base_url.as_ref()
            && let Ok(candidate) = paths::join_relative(&base_url.path, specifier)
            && let Some(path) = paths::resolve_known_path(&candidate, known_paths)
        {
            return NonRelativeResolution::Local(path);
        }
        NonRelativeResolution::External
    }
}

pub use config::config_extends;
pub use paths::{resolve_known_path, resolve_relative};
