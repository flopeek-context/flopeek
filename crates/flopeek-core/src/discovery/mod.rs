//! Deterministic TypeScript/TSX repository discovery.

use crate::model::SourceFile;
use std::fs;
use std::path::{Path, PathBuf};

const EXCLUDED_DIRECTORIES: &[&str] = &[
    ".git",
    ".flopeek",
    "node_modules",
    "target",
    "dist",
    "build",
    "coverage",
    ".next",
    ".turbo",
];

pub fn is_typescript_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| matches!(extension.to_ascii_lowercase().as_str(), "ts" | "tsx"))
}

pub fn language_for_path(path: &Path) -> Option<&'static str> {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("ts") => Some("typescript"),
        Some("tsx") => Some("tsx"),
        _ => None,
    }
}

pub fn discover(root: &Path) -> Result<Vec<SourceFile>, String> {
    let root = root.canonicalize().map_err(|error| {
        format!(
            "Unable to resolve repository root {}: {error}",
            root.display()
        )
    })?;
    if !root.is_dir() {
        return Err(format!(
            "Repository root is not a directory: {}",
            root.display()
        ));
    }

    let mut pending = vec![root.clone()];
    let mut files = Vec::new();
    while let Some(directory) = pending.pop() {
        let mut entries = fs::read_dir(&directory)
            .map_err(|error| format!("Unable to read {}: {error}", directory.display()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("Unable to enumerate {}: {error}", directory.display()))?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries.into_iter().rev() {
            let path = entry.path();
            let file_type = entry
                .file_type()
                .map_err(|error| format!("Unable to inspect {}: {error}", path.display()))?;
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                if entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| EXCLUDED_DIRECTORIES.contains(&name))
                {
                    continue;
                }
                pending.push(path);
                continue;
            }
            if !file_type.is_file() || !is_typescript_path(&path) {
                continue;
            }
            files.push(source_file(&root, &path)?);
        }
    }
    files.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(files)
}

pub fn source_file(root: &Path, path: &Path) -> Result<SourceFile, String> {
    let language = language_for_path(path)
        .ok_or_else(|| format!("Unsupported analyzed file: {}", path.display()))?;
    let bytes =
        fs::read(path).map_err(|error| format!("Unable to read {}: {error}", path.display()))?;
    let relative = path
        .strip_prefix(root)
        .map_err(|error| format!("Unable to relativize {}: {error}", path.display()))?;
    let path = normalize_relative_path(relative)?;
    Ok(SourceFile {
        path,
        language: language.to_string(),
        bytes: bytes.len() as u64,
        hash: blake3::hash(&bytes).to_hex().to_string(),
    })
}

pub fn normalize_relative_path(path: &Path) -> Result<String, String> {
    let mut components = Vec::new();
    for component in path.components() {
        let text = component
            .as_os_str()
            .to_str()
            .ok_or_else(|| "Repository paths must be valid UTF-8.".to_string())?;
        if text.is_empty() || text == "." {
            continue;
        }
        if text == ".." {
            return Err("Repository-relative paths may not escape the project root.".to_string());
        }
        components.push(text.replace('\\', "/"));
    }
    if components.is_empty() {
        return Err("Repository-relative path is empty.".to_string());
    }
    Ok(components.join("/"))
}

pub fn read_source(root: &Path, relative_path: &str) -> Result<Vec<u8>, String> {
    let relative = PathBuf::from(relative_path);
    let normalized = normalize_relative_path(&relative)?;
    if normalized != relative_path.replace('\\', "/") {
        return Err("Source path must be normalized and repository-relative.".to_string());
    }
    let path = root.join(&relative);
    let canonical = path
        .canonicalize()
        .map_err(|error| format!("Unable to resolve source path {relative_path}: {error}"))?;
    let root = root
        .canonicalize()
        .map_err(|error| format!("Unable to resolve repository root: {error}"))?;
    if !canonical.starts_with(&root) {
        return Err("Source path resolves outside the repository root.".to_string());
    }
    fs::read(canonical).map_err(|error| format!("Unable to read source {relative_path}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root() -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!("flopeek-discovery-{suffix}"))
    }

    #[test]
    fn discovers_only_typescript_and_tsx() {
        let root = temp_root();
        fs::create_dir_all(root.join("src")).expect("mkdir");
        fs::create_dir_all(root.join("node_modules/pkg")).expect("mkdir");
        fs::write(root.join("src/a.ts"), "export const a = 1;").expect("write");
        fs::write(root.join("src/b.tsx"), "export const B = () => <div />;").expect("write");
        fs::write(root.join("src/c.js"), "module.exports = 1;").expect("write");
        fs::write(root.join("src/d.py"), "answer = 42").expect("write");
        fs::write(root.join("node_modules/pkg/x.ts"), "export const x = 1;").expect("write");

        let files = discover(&root).expect("discover");
        assert_eq!(
            files
                .iter()
                .map(|file| file.path.as_str())
                .collect::<Vec<_>>(),
            vec!["src/a.ts", "src/b.tsx"]
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn rejects_escaping_source_paths() {
        assert!(normalize_relative_path(Path::new("../outside.ts")).is_err());
    }
}
