//! Machine-checked product boundary.

use serde_json::Value;

pub fn validate() -> Result<(), String> {
    let value: Value = serde_json::from_str(include_str!("../../../contracts/product.json"))
        .map_err(|error| format!("Invalid product contract JSON: {error}"))?;
    if value["schemaVersion"] != "flopeek-product-contract/v5"
        || value["canonicalRepository"] != "flopeek-context/flopeek"
        || value["coreImplementation"] != "rust"
        || value["persistedAuthority"] != "sqlite"
        || value["diagnosticMetadataAuthority"] != "sqlite"
        || value["llmRequired"] != false
        || value["automaticRootCauseClaims"] != false
        || value["javascriptRepositoryAuthority"] != false
        || value["graphIdentityBasis"] != "typescript-context-structural-evidence"
        || value["sourceBasis"] != "immutable-graph-observation"
        || value["contextFreshness"] != "node-ast-and-direct-edges"
        || value["flowEvidenceBasis"] != "root-package-manifest-and-static-call-projection"
        || value["flowFreshness"] != "entry-step-evidence-and-traversed-edges"
        || value["relatedTestEvidence"] != "direct-call-construct-or-import"
        || value["observationContinuity"] != "immutable-scan-event-chain"
        || value["contextReconciliation"] != "exact-compatible-fingerprint-candidates"
        || value["automaticSupersession"] != "disabled-without-lineage-proof"
        || value["structuralChangeAttribution"] != "adjacent-observation-compatible-evidence"
        || value["productIdentity"] != "versioned-repository-context"
        || value["graphRole"] != "deterministic-substrate"
        || value["languageCountIsProductGoal"] != false
        || value["reviewGraphIsPrimaryProduct"] != false
    {
        return Err("Product contract violates the Rust/SQLite TypeScript boundary.".to_string());
    }
    let languages = value["primaryAnalyzedLanguages"]
        .as_array()
        .ok_or_else(|| "Product contract analyzedLanguages must be an array.".to_string())?;
    if languages
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>()
        != ["typescript", "tsx"]
    {
        return Err("Product contract must analyze only TypeScript and TSX.".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn active_product_contract_is_narrow_and_machine_checked() {
        super::validate().expect("product contract");
    }
}
