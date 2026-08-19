//! Repository/checkout identity resolution.
//!
//! Repository identity is explicit and portable.  Checkout identity remains a
//! local compatibility fallback and is never serialized as portable evidence.

use serde::de::{self, Deserializer, MapAccess, Visitor};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::path::{Component, Path, PathBuf};

pub const MANIFEST_PATH: &str = ".flopeek-repository.json";
pub const MANIFEST_SCHEMA: &str = "flopeek-repository-identity/v1";
pub const MAX_MANIFEST_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct IdentityManifest {
    pub schema_version: String,
    pub repository_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct IdentityBasis {
    pub schema_version: String,
    pub status: String,
    pub repository_id: Option<String>,
    pub manifest_path: Option<String>,
    pub manifest_bytes: Option<u64>,
    pub manifest_hash: Option<String>,
    pub limitations: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedIdentity {
    pub project_id: String,
    pub checkout_id: String,
    pub repository_id: Option<String>,
    pub basis: IdentityBasis,
    pub manifest: Option<IdentityManifest>,
}

pub fn resolve(root: &Path) -> Result<ResolvedIdentity, String> {
    let root = root.canonicalize().map_err(|error| {
        format!(
            "Unable to resolve identity root {}: {error}",
            root.display()
        )
    })?;
    if !root.is_dir() {
        return Err(format!(
            "Identity root is not a directory: {}",
            root.display()
        ));
    }
    let checkout_id = checkout_id(&root);
    let manifest_path = root.join(MANIFEST_PATH);
    if !manifest_path.exists() {
        return Ok(ResolvedIdentity {
            project_id: checkout_id.clone(),
            checkout_id,
            repository_id: None,
            basis: IdentityBasis {
                schema_version: MANIFEST_SCHEMA.to_string(),
                status: "unavailable".to_string(),
                repository_id: None,
                manifest_path: None,
                manifest_bytes: None,
                manifest_hash: None,
                limitations: vec![
                    "repository-identity-manifest-unavailable".to_string(),
                    "cross-checkout-context-unavailable".to_string(),
                ],
            },
            manifest: None,
        });
    }
    let metadata = fs::symlink_metadata(&manifest_path)
        .map_err(|error| format!("Unable to inspect repository identity manifest: {error}"))?;
    if metadata.file_type().is_symlink() {
        return Err("repository-identity-manifest-invalid: symlink is not allowed".to_string());
    }
    let canonical_manifest = manifest_path.canonicalize().map_err(|error| {
        format!("repository-identity-manifest-invalid: unable to resolve path: {error}")
    })?;
    if canonical_manifest.parent() != Some(root.as_path()) {
        return Err(
            "repository-identity-manifest-invalid: path escapes repository root".to_string(),
        );
    }
    let bytes = fs::read(&manifest_path)
        .map_err(|error| format!("Unable to read repository identity manifest: {error}"))?;
    if bytes.len() > MAX_MANIFEST_BYTES {
        return Err(format!(
            "repository-identity-manifest-invalid: manifest exceeds {MAX_MANIFEST_BYTES} bytes"
        ));
    }
    let manifest = parse_manifest(&bytes)?;
    let manifest_hash = blake3::hash(&bytes).to_hex().to_string();
    let project_id = repository_project_id(&manifest.repository_id);
    Ok(ResolvedIdentity {
        project_id,
        checkout_id,
        repository_id: Some(manifest.repository_id.clone()),
        basis: IdentityBasis {
            schema_version: MANIFEST_SCHEMA.to_string(),
            status: "available".to_string(),
            repository_id: Some(manifest.repository_id.clone()),
            manifest_path: Some(MANIFEST_PATH.to_string()),
            manifest_bytes: Some(bytes.len() as u64),
            manifest_hash: Some(manifest_hash),
            limitations: vec!["checkout-identity-is-local-only".to_string()],
        },
        manifest: Some(manifest),
    })
}

fn parse_manifest(bytes: &[u8]) -> Result<IdentityManifest, String> {
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let object = deserializer
        .deserialize_map(StrictObjectVisitor)
        .map_err(|error| format!("repository-identity-manifest-invalid: {error}"))?;
    deserializer
        .end()
        .map_err(|error| format!("repository-identity-manifest-invalid: {error}"))?;
    let value = serde_json::Value::Object(object);
    let object = value.as_object().ok_or_else(|| {
        "repository-identity-manifest-invalid: root must be an object".to_string()
    })?;
    let expected = ["schemaVersion", "repositoryId"];
    let actual = object.keys().map(String::as_str).collect::<BTreeSet<_>>();
    let expected_set = expected.into_iter().collect::<BTreeSet<_>>();
    if actual != expected_set {
        return Err("repository-identity-manifest-invalid: unknown or missing fields".to_string());
    }
    let manifest: IdentityManifest = serde_json::from_value(value)
        .map_err(|error| format!("repository-identity-manifest-invalid: {error}"))?;
    if manifest.schema_version != MANIFEST_SCHEMA {
        return Err(format!(
            "repository-identity-manifest-invalid: unsupported schema {}",
            manifest.schema_version
        ));
    }
    if !valid_repository_id(&manifest.repository_id) {
        return Err(
            "repository-identity-manifest-invalid: repositoryId must be repo_<uuid>".to_string(),
        );
    }
    Ok(manifest)
}

struct StrictObjectVisitor;

impl<'de> Visitor<'de> for StrictObjectVisitor {
    type Value = serde_json::Map<String, serde_json::Value>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON object with unique fields")
    }

    fn visit_map<A>(self, mut access: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut object = serde_json::Map::new();
        let mut seen = BTreeSet::new();
        while let Some(key) = access.next_key::<String>()? {
            if !seen.insert(key.clone()) {
                return Err(de::Error::custom(format!("duplicate field {key}")));
            }
            object.insert(key, access.next_value()?);
        }
        Ok(object)
    }
}

fn valid_repository_id(value: &str) -> bool {
    let Some(uuid) = value.strip_prefix("repo_") else {
        return false;
    };
    if uuid.len() != 36 {
        return false;
    }
    uuid.chars().enumerate().all(|(index, character)| {
        if matches!(index, 8 | 13 | 18 | 23) {
            character == '-'
        } else {
            character.is_ascii_hexdigit()
        }
    })
}

pub fn checkout_id(root: &Path) -> String {
    let identity = root
        .canonicalize()
        .ok()
        .and_then(|path| path.to_str().map(ToOwned::to_owned))
        .unwrap_or_else(|| root.to_string_lossy().into_owned());
    format!(
        "project_{}",
        blake3::hash(format!("flopeek-project-v1\0{identity}").as_bytes()).to_hex()
    )
}

pub fn repository_project_id(repository_id: &str) -> String {
    format!(
        "project_{}",
        blake3::hash(format!("flopeek-project-repository-v1\0{repository_id}").as_bytes()).to_hex()
    )
}

pub fn manifest_path_is_safe(root: &Path) -> bool {
    root.join(MANIFEST_PATH)
        .components()
        .all(|component| !matches!(component, Component::Prefix(_) | Component::ParentDir))
}

pub fn manifest_path(root: &Path) -> PathBuf {
    root.join(MANIFEST_PATH)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("flopeek-identity-{label}-{nonce}"));
        fs::create_dir_all(&root).expect("root");
        root
    }

    fn write_manifest(root: &Path, id: &str) {
        fs::write(
            root.join(MANIFEST_PATH),
            format!("{{\"schemaVersion\":\"{MANIFEST_SCHEMA}\",\"repositoryId\":\"{id}\"}}"),
        )
        .expect("manifest");
    }

    #[test]
    fn explicit_manifest_is_stable_across_checkout_paths() {
        let left = temp_root("left");
        let right = temp_root("right");
        let id = "repo_123e4567-e89b-12d3-a456-426614174000";
        write_manifest(&left, id);
        write_manifest(&right, id);
        let left_identity = resolve(&left).expect("left identity");
        let right_identity = resolve(&right).expect("right identity");
        assert_eq!(left_identity.project_id, right_identity.project_id);
        assert_ne!(left_identity.checkout_id, right_identity.checkout_id);
        fs::remove_dir_all(left).expect("left cleanup");
        fs::remove_dir_all(right).expect("right cleanup");
    }

    #[test]
    fn missing_manifest_is_explicitly_local_only() {
        let root = temp_root("missing");
        let identity = resolve(&root).expect("identity");
        assert_eq!(identity.basis.status, "unavailable");
        assert!(
            identity
                .basis
                .limitations
                .iter()
                .any(|item| item == "cross-checkout-context-unavailable")
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn invalid_manifest_is_rejected() {
        let root = temp_root("invalid");
        fs::write(root.join(MANIFEST_PATH), "{\"repositoryId\":\"bad\"}").expect("manifest");
        let error = resolve(&root).expect_err("invalid identity");
        assert!(error.contains("repository-identity-manifest-invalid"));
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn duplicate_manifest_fields_are_rejected() {
        let root = temp_root("duplicate");
        fs::write(
            root.join(MANIFEST_PATH),
            format!(
                "{{\"schemaVersion\":\"{MANIFEST_SCHEMA}\",\"schemaVersion\":\"{MANIFEST_SCHEMA}\",\"repositoryId\":\"repo_123e4567-e89b-12d3-a456-426614174000\"}}"
            ),
        )
        .expect("manifest");
        let error = resolve(&root).expect_err("duplicate identity");
        assert!(error.contains("duplicate field"));
        fs::remove_dir_all(root).expect("cleanup");
    }
}
