"use strict";

const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const {
  DOCUMENTS,
  assertGeneratedDocuments,
  buildProductContractFromInputs,
  expectedDocuments,
  loadProductContractInputs,
} = require("../../scripts/product-contract");

const ROOT = path.resolve(__dirname, "..", "..");

function inputs() {
  return structuredClone(loadProductContractInputs(ROOT));
}

test("product contract is derived from package, rollout, approval, mode, and adapter authorities", () => {
  const contract = buildProductContractFromInputs(inputs());
  assert.equal(contract.package.sourceVersion, require("../../package.json").version);
  assert.equal(contract.package.minimumNodeMajor, 22);
  assert.equal(contract.package.publicationState, "blocked-pending-canonical-approval");
  assert.equal(contract.package.distTag, null);
  assert.equal(contract.authority.canonicalRepository, "flopeek-context/flopeek");
  assert.equal(contract.authority.coreImplementation, "rust");
  assert.equal(contract.authority.persistedAuthority, "sqlite");
  assert.deepEqual(contract.authority.primaryDiagnosticLanguages, ["typescript", "tsx"]);
  assert.equal(contract.authority.llmRequired, false);
  assert.equal(contract.authority.javascriptRepositoryAuthority, false);
  assert.equal(contract.authority.automaticRootCauseClaims, false);
  assert.equal(contract.authority.historicalOutputClass, "candidate-not-cause");
  assert.equal(contract.core.publicDefaultImplementation, "javascript");
  assert.equal(contract.core.experimentalImplementation, "native");
  assert.equal(contract.core.authorityCutoverStatus, "pending");
  assert.equal(contract.core.nativeRolloutStatus, "incomplete");
  assert.equal(contract.core.nativeDefaultEligible, false);
  assert.ok(contract.adapters.native.some((adapter) => adapter.id === "go"));
});

test("product contract rejects source-version and Node-minimum contradictions", () => {
  const versionMismatch = inputs();
  versionMismatch.npmApproval.version = "0.0.0-contradiction";
  assert.throws(() => buildProductContractFromInputs(versionMismatch), /approval identity differs/);
  const nodeMismatch = inputs();
  nodeMismatch.packagePolicy.package.minimumNodeMajor = 20;
  assert.throws(() => buildProductContractFromInputs(nodeMismatch), /Node minimum differs/);
  const unsafePublication = inputs();
  unsafePublication.packageJson.private = false;
  assert.throws(() => buildProductContractFromInputs(unsafePublication), /Pending canonical publication approval requires/);
});

test("a complete evidence packet with a negative native gate remains non-eligible", () => {
  const blocked = inputs();
  blocked.rolloutEvidence = {
    ...blocked.rolloutEvidence,
    status: "blocked",
    decision: { eligible: false, reasons: ["memory-peak-not-proven"] },
  };
  const contract = buildProductContractFromInputs(blocked);
  assert.equal(contract.core.nativeRolloutStatus, "blocked");
  assert.equal(contract.core.nativeDefaultEligible, false);
});

test("generated documentation detects version, Node, language, and native-default drift", () => {
  const contract = buildProductContractFromInputs(inputs());
  const originals = Object.fromEntries(DOCUMENTS.map((name) => [name, fs.readFileSync(path.join(ROOT, name), "utf8")]));
  const generated = expectedDocuments(contract, originals);
  assert.equal(assertGeneratedDocuments(contract, generated), true);
  const crlfCheckout = Object.fromEntries(Object.entries(generated)
    .map(([name, source]) => [name, source.replace(/\n/g, "\r\n")]));
  assert.equal(assertGeneratedDocuments(contract, crlfCheckout), true);
  for (const [label, mutate] of [
    ["version", (source) => source.replace(contract.package.sourceVersion, "0.0.0-stale")],
    ["Node", (source) => source.replace("Node.js 22", "Node.js 20")],
    ["language", (source) => source.replace("go (go; bundled)", "go (go; unavailable)")],
    ["native default", (source) => source.replace("native-default eligibility is `false`", "native-default eligibility is `true`")],
  ]) {
    const contradictory = { ...generated, "native/flopeek-core/README.md": mutate(generated["native/flopeek-core/README.md"]) };
    assert.throws(() => assertGeneratedDocuments(contract, contradictory), /stale/, label);
  }
});
