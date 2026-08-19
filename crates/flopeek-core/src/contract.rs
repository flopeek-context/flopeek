//! Machine-checked product boundary.

use serde_json::Value;

pub fn validate() -> Result<(), String> {
    let value: Value = serde_json::from_str(include_str!("../../../contracts/product.json"))
        .map_err(|error| format!("Invalid product contract JSON: {error}"))?;
    if value["schemaVersion"] != crate::model::PRODUCT_CONTRACT_SCHEMA
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
        || value["repositoryIdentity"] != "explicit-versioned-root-manifest"
        || value["checkoutIdentity"] != "canonical-path-local-only"
        || value["legacyProjectIdentity"] != "local-alias-only"
        || value["crossCheckoutContext"] != "repository-identity-required"
        || value["historicalContextContinuity"] != "adjacent-first-parent-static-evidence"
        || value["lastKnownGood"] != "attributed-human-confirmation"
        || value["lastKnownGoodLifecycle"] != "protocol-1.0-deterministic-reducer"
        || value["lastKnownGoodProvenance"] != "revision-observation-graph-consistent"
        || value["humanActorIdentity"] != "caller-attributed-not-authenticated"
        || value["lastKnownGoodModel"] != "immutable-candidate-append-only-event-reduced-state"
        || value["lastKnownGoodIntegrity"] != "observation-owned-revision-and-graph-contract"
        || value["lastKnownGoodApplicability"] != "current-first-parent-and-context-revision"
        || value["lastKnownGoodTrust"] != "local-transition-boundary-caller-attributed"
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

pub fn validate_lkg_protocol() -> Result<(), String> {
    let value: Value = serde_json::from_str(include_str!("../../../contracts/lkg-protocol.json"))
        .map_err(|error| format!("Invalid LKG protocol contract JSON: {error}"))?;
    if value["schemaVersion"] != "flopeek-lkg-protocol/v1"
        || value["candidate"]["schemaVersion"] != "flopeek-last-known-good-candidate/v1"
        || value["event"]["schemaVersion"] != "flopeek-last-known-good-event/v1"
        || value["state"]["schemaVersion"] != "flopeek-last-known-good-state/v1"
        || value["reviewPacket"]["schemaVersion"] != "flopeek-last-known-good-review-packet/v1"
        || value["reviewPacket"]["includes"]
            != serde_json::json!([
                "context",
                "candidate",
                "state",
                "applicability",
                "candidate-to-current-structural-delta",
                "confirmability"
            ])
        || value["event"]["types"] != serde_json::json!(["PROPOSE", "CONFIRM", "REJECT", "REVOKE"])
        || value["event"]["forbiddenTypes"] != serde_json::json!(["SUPERSEDE"])
        || value["event"]["onePendingCandidate"] != true
        || value["event"]["oneActiveCandidate"] != true
        || value["concurrency"]["expectedTipRequired"] != true
        || value["concurrency"]["idempotencyRequired"] != true
        || value["trust"]["actorIdentity"] != "caller-attributed-not-authenticated"
        || value["validation"]["revisionAuthority"] != "graph_observations.git_revision"
        || value["validation"]["graphVersionSourceRevisionAuthority"] != false
        || value["migration"]["policy"] != "preserve-semantics-or-quarantine"
        || value["migration"]["failClosed"] != true
    {
        return Err("LKG Protocol 1.0 contract is invalid.".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn active_product_contract_is_narrow_and_machine_checked() {
        super::validate().expect("product contract");
    }

    #[test]
    fn lkg_protocol_contract_is_machine_checked() {
        super::validate_lkg_protocol().expect("LKG protocol contract");
    }
}
