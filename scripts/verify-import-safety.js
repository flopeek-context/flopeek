#!/usr/bin/env node
"use strict";

const fs = require("node:fs");
const path = require("node:path");

const EXPECTED_BASELINE = "72a95fe1a6497683e96e90872438cd3c83b7272f";
const PROVENANCE_SCHEMA = "repository-engineering-memory-provenance/v1";
const CANONICAL_REPOSITORY = "flopeek-context/flopeek";

function verifyImportSafety(root) {
  const readJson = (relativePath) => JSON.parse(fs.readFileSync(path.join(root, relativePath), "utf8"));
  const provenance = readJson("packaging/repository-provenance.json");
  const packageJson = readJson("package.json");
  const npmApproval = readJson("packaging/npm-publication-approval.json");
  const githubApproval = readJson("packaging/github-release-approval.json");
  const promotionWorkflow = fs.readFileSync(path.join(root, ".github", "workflows", "native-promotion.yml"), "utf8");
  const dependabotConfig = fs.readFileSync(path.join(root, ".github", "dependabot.yml"), "utf8");
  const errors = [];

  if (provenance.schemaVersion !== PROVENANCE_SCHEMA) errors.push(`provenance must use ${PROVENANCE_SCHEMA}`);
  if (provenance.sourceBaseline?.repository !== "badsleepyday/flopeek-core"
    || provenance.sourceBaseline?.branch !== "development"
    || provenance.sourceBaseline?.commit !== EXPECTED_BASELINE
    || provenance.sourceBaseline?.role !== "initial-source-snapshot-only") {
    errors.push("source baseline provenance does not match the immutable import contract");
  }
  if (provenance.repositoryRelationship !== "independent-product-line") errors.push("repository relationship must remain independent-product-line");
  if (provenance.canonicalRepository?.repository !== CANONICAL_REPOSITORY
    || provenance.canonicalRepository?.role !== "sole-active-repository-authority") {
    errors.push(`canonical repository authority must be ${CANONICAL_REPOSITORY}`);
  }
  if (provenance.productIdentity?.status !== "preserved"
    || provenance.productIdentity?.product !== "Flopeek"
    || provenance.productIdentity?.package !== "flopeek"
    || provenance.productIdentity?.cli !== "flopeek"
    || provenance.productIdentity?.metadataDirectory !== ".flopeek"
    || provenance.productIdentity?.contextRefScheme !== "fp") {
    errors.push("the canonical Flopeek product, package, CLI, metadata, and Context Ref identities must remain preserved");
  }
  if (provenance.publicationAuthority?.status !== "pending"
    || provenance.publicationAuthority?.npmPublication !== "blocked"
    || provenance.publicationAuthority?.githubRelease !== "blocked"
    || provenance.publicationAuthority?.historicalApprovals !== "non-authoritative") {
    errors.push("canonical publication authority must remain pending and historical approvals non-authoritative");
  }
  if (packageJson.name !== "flopeek" || packageJson.private !== true) errors.push("the canonical Flopeek package must remain private until publication approval");
  if (packageJson.publishConfig !== undefined) errors.push("the package must not declare publishConfig before canonical publication approval");
  if (packageJson.homepage !== `https://github.com/${CANONICAL_REPOSITORY}#readme`
    || packageJson.bugs?.url !== `https://github.com/${CANONICAL_REPOSITORY}/issues`
    || packageJson.repository?.url !== `git+https://github.com/${CANONICAL_REPOSITORY}.git`) {
    errors.push(`active package metadata must point to ${CANONICAL_REPOSITORY}`);
  }
  if (packageJson.scripts?.prepublishOnly !== "node scripts/block-legacy-publication.js") errors.push("prepublishOnly must use the historical-approval publication blocker");
  if (npmApproval.status !== "not-approved" || githubApproval.status !== "not-approved") errors.push("publication approval records must remain not-approved");

  for (const forbidden of ["npm publish", "npm dist-tag add", "git push", "gh release create", "gh release upload"]) {
    if (promotionWorkflow.includes(forbidden)) errors.push(`disabled promotion workflow still contains mutating command: ${forbidden}`);
  }
  if (!/^\s*permissions:\s*\n\s+contents:\s*read\s*$/mu.test(promotionWorkflow)) errors.push("disabled promotion workflow must have read-only repository permissions");
  const dependabotLimits = [...dependabotConfig.matchAll(/^\s*open-pull-requests-limit:\s*(\d+)\s*$/gmu)]
    .map((match) => Number(match[1]));
  if (dependabotLimits.length !== 3 || dependabotLimits.some((limit) => limit !== 0)) {
    errors.push("routine Dependabot version updates must remain disabled for GitHub Actions, npm, and Cargo during Phase 0");
  }

  if (errors.length) throw new Error(errors.join("; "));
  return {
    schemaVersion: "repository-import-safety-verification/v1",
    status: "passed",
    sourceBaseline: EXPECTED_BASELINE,
    repositoryAuthority: "canonical",
    canonicalRepository: CANONICAL_REPOSITORY,
    productIdentity: "preserved",
    publicationAuthority: "pending",
    npmPublication: "blocked",
    githubRelease: "blocked",
    dependabotVersionUpdates: "blocked",
  };
}

if (require.main === module) {
  try {
    const result = verifyImportSafety(path.resolve(__dirname, ".."));
    console.log(`Import safety verified at ${result.sourceBaseline}: ${result.canonicalRepository} is authoritative and publication remains blocked.`);
  } catch (error) {
    console.error(`Import safety verification failed: ${error.message}`);
    process.exitCode = 1;
  }
}

module.exports = { CANONICAL_REPOSITORY, EXPECTED_BASELINE, PROVENANCE_SCHEMA, verifyImportSafety };
