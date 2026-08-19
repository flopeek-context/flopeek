//! Observation-owned exact source evidence helpers.

use crate::model::SourceFile;
use std::path::{Component, Path};

const MAX_OBSERVATION_MANIFEST_BYTES: usize = 4 * 1024 * 1024;

pub(super) fn decode_source_manifest(raw: &str) -> Result<Vec<SourceFile>, String> {
    if raw.len() > MAX_OBSERVATION_MANIFEST_BYTES {
        return Err(format!(
            "observation-source-manifest-invalid: manifest exceeds {} bytes",
            MAX_OBSERVATION_MANIFEST_BYTES
        ));
    }
    let mut files = serde_json::from_str::<Vec<SourceFile>>(raw)
        .map_err(|error| format!("observation-source-manifest-invalid: {error}"))?;
    if files.len() > crate::graph::MAX_SOURCE_FILES {
        return Err(format!(
            "observation-source-manifest-invalid: file count exceeds {}",
            crate::graph::MAX_SOURCE_FILES
        ));
    }
    for file in &files {
        let normalized_segments = file.path.split('/').all(|segment| {
            !segment.is_empty() && segment != "." && segment != ".."
        });
        if file.path.is_empty()
            || file.path.contains('\\')
            || file.path.contains('\0')
            || file.hash.is_empty()
            || !matches!(file.language.as_str(), "typescript" | "tsx")
            || !normalized_segments
            || Path::new(&file.path).components().any(|component| {
                matches!(
                    component,
                    Component::Prefix(_)
                        | Component::RootDir
                        | Component::ParentDir
                        | Component::CurDir
                )
            })
        {
            return Err(
                "observation-source-manifest-invalid: source file is not normalized and repository-relative"
                    .to_string(),
            );
        }
    }
    files.sort_by(|left, right| left.path.cmp(&right.path));
    if files
        .windows(2)
        .any(|window| window[0].path == window[1].path)
    {
        return Err("observation-source-manifest-invalid: duplicate source path".to_string());
    }
    Ok(files)
}
