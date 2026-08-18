//! Repository-relative module and path mapping resolution.

use super::config::PathMapping;
use crate::discovery::normalize_relative_path;
use std::collections::BTreeMap;
use std::path::Path;
pub(super) fn normalize_config_path(base: &str, value: &str) -> Result<String, String> {
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

pub(super) fn matching_mapping<'a>(
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

pub(super) fn join_relative(base: &str, value: &str) -> Result<String, String> {
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
