use serde::{Deserialize, Serialize};

use super::{SourcePosition, TYPESCRIPT_RESOLUTION_SCHEMA};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TypeScriptImport {
    pub specifier: String,
    pub kind: String,
    pub position: SourcePosition,
    #[serde(default)]
    pub local_name: Option<String>,
    #[serde(default)]
    pub imported_name: Option<String>,
    #[serde(default)]
    pub type_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TypeScriptDeclaration {
    pub name: String,
    pub kind: String,
    pub exported: bool,
    pub position: SourcePosition,
    #[serde(default)]
    pub qualified_name: String,
    #[serde(default)]
    pub ast_fingerprint: String,
    #[serde(default)]
    pub owner: Option<String>,
    #[serde(default)]
    pub static_member: bool,
    #[serde(default)]
    pub visibility: String,
    #[serde(default)]
    pub abstract_member: bool,
    #[serde(default)]
    pub declaration_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TypeScriptCall {
    pub callee: Option<String>,
    pub dynamic: bool,
    pub position: SourcePosition,
    #[serde(default)]
    pub caller: Option<String>,
    #[serde(default)]
    pub callee_form: String,
    #[serde(default)]
    pub receiver: Option<String>,
    #[serde(default)]
    pub shadowed: bool,
    #[serde(default)]
    pub enclosing_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TypeScriptHeritage {
    pub owner: String,
    pub relation: String,
    pub reference: String,
    pub form: String,
    pub position: SourcePosition,
    #[serde(default)]
    pub type_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TypeScriptExport {
    pub exported_name: String,
    pub local_name: Option<String>,
    pub kind: String,
    pub source: Option<String>,
    pub type_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SymbolResolution {
    pub path: String,
    pub caller_node_id: String,
    pub reference: String,
    pub form: String,
    pub status: String,
    pub reason: String,
    pub candidate_node_ids: Vec<String>,
    pub occurrence_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TypeScriptFacts {
    #[serde(default)]
    pub schema_version: String,
    pub path: String,
    pub language: String,
    pub source_hash: String,
    pub parser: String,
    pub parse_status: String,
    pub imports: Vec<TypeScriptImport>,
    pub declarations: Vec<TypeScriptDeclaration>,
    #[serde(default)]
    pub exports: Vec<TypeScriptExport>,
    pub calls: Vec<TypeScriptCall>,
    pub unsupported: Vec<String>,
    #[serde(default)]
    pub resolution_records: Vec<SymbolResolution>,
    #[serde(default)]
    pub canonical_fingerprint: String,
    #[serde(default)]
    pub heritage: Vec<TypeScriptHeritage>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ResolutionEvidence {
    pub schema_version: String,
    pub status: String,
    pub records: Vec<SymbolResolution>,
    pub truncated: bool,
    pub omissions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct ModuleResolutionConfigFile {
    pub path: String,
    pub bytes: u64,
    pub hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct ModuleResolutionBasis {
    pub schema_version: String,
    pub status: String,
    pub root_config: Option<String>,
    pub config_files: Vec<ModuleResolutionConfigFile>,
    pub exact_fingerprint: String,
    pub effective_fingerprint: String,
    pub limitations: Vec<String>,
    pub omissions: Vec<String>,
}

impl Default for ModuleResolutionBasis {
    fn default() -> Self {
        Self {
            schema_version: "flopeek-typescript-module-resolution/v1".to_string(),
            status: "unavailable".to_string(),
            root_config: None,
            config_files: Vec::new(),
            exact_fingerprint: String::new(),
            effective_fingerprint: String::new(),
            limitations: vec!["module-resolution-basis-unavailable".to_string()],
            omissions: Vec::new(),
        }
    }
}

impl Default for ResolutionEvidence {
    fn default() -> Self {
        Self {
            schema_version: TYPESCRIPT_RESOLUTION_SCHEMA.to_string(),
            status: "unavailable".to_string(),
            records: Vec::new(),
            truncated: false,
            omissions: vec![
                "resolution evidence is unavailable until a v2 TypeScript facts scan".to_string(),
            ],
        }
    }
}
