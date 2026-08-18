#!/usr/bin/env node
"use strict";

const fs = require("node:fs");
const path = require("node:path");

const EXPECTED_BASELINE = "72a95fe1a6497683e96e90872438cd3c83b7272f";
const PROVENANCE_SCHEMA = "repository-engineering-memory-provenance/v1";

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
  if (provenance.identityIsolation?.status !== "pending" || provenance.identityIsolation?.targetIdentity !== null) {
    errors.push("identity isolation must remain explicitly pending until the dedicated rename change");
  }
  if (provenance.identityIsolation?.legacyPackagePublication !== "blocked"
    || provenance.identityIsolation?.legacyGithubRelease !== "blocked") {
    errors.push("legacy publication and release must remain blocked");
  }
  if (packageJson.name !== "flopeek" || packageJson.private !== true) errors.push("the imported legacy package must remain private");
  if (packageJson.publishConfig !== undefined) errors.push("the imported legacy package must not declare publishConfig");
  if (packageJson.scripts?.prepublishOnly !== "node scripts/block-legacy-publication.js") errors.push("prepublishOnly must use the hard legacy-publication blocker");
  if (npmApproval.status !== "not-approved" || githubApproval.status !== "not-approved") errors.push("legacy approval records must remain not-approved");

  for (const forbidden of ["npm publish", "npm dist-tag add", "git push", "gh release create", "gh release upload"]) {
    if (promotionWorkflow.includes(forbidden)) errors.push(`legacy promotion workflow still contains mutating command: ${forbidden}`);
  }
  if (!/^\s*permissions:\s*\n\s+contents:\s*read\s*$/mu.test(promotionWorkflow)) errors.push("legacy promotion workflow must have read-only repository permissions");
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
    identityIsolation: "pending",
    legacyPublication: "blocked",
    legacyRelease: "blocked",
    dependabotVersionUpdates: "blocked",
  };
}

if (require.main === module) {
  try {
    const result = verifyImportSafety(path.resolve(__dirname, ".."));
    console.log(`Import safety verified at ${result.sourceBaseline}: legacy publication and release are blocked.`);
  } catch (error) {
    console.error(`Import safety verification failed: ${error.message}`);
    process.exitCode = 1;
  }
}

module.exports = { EXPECTED_BASELINE, PROVENANCE_SCHEMA, verifyImportSafety };
