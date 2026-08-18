"use strict";

const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const test = require("node:test");
const { EXPECTED_BASELINE, verifyImportSafety } = require("../../scripts/verify-import-safety");

const ROOT = path.resolve(__dirname, "..", "..");
const REQUIRED_FILES = [
  "package.json",
  "packaging/repository-provenance.json",
  "packaging/npm-publication-approval.json",
  "packaging/github-release-approval.json",
  ".github/workflows/native-promotion.yml",
  ".github/dependabot.yml",
];

function fixtureRoot(context) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "repository-import-safety-"));
  context.after(() => fs.rmSync(root, { recursive: true, force: true }));
  for (const relativePath of REQUIRED_FILES) {
    const target = path.join(root, relativePath);
    fs.mkdirSync(path.dirname(target), { recursive: true });
    fs.copyFileSync(path.join(ROOT, relativePath), target);
  }
  return root;
}

test("current import provenance and legacy release blocks are consistent", () => {
  const result = verifyImportSafety(ROOT);
  assert.equal(result.status, "passed");
  assert.equal(result.sourceBaseline, EXPECTED_BASELINE);
  assert.equal(result.identityIsolation, "pending");
  assert.equal(result.legacyPublication, "blocked");
  assert.equal(result.legacyRelease, "blocked");
  assert.equal(result.dependabotVersionUpdates, "blocked");
});

test("import safety rejects publishable package metadata and release mutations", (context) => {
  const root = fixtureRoot(context);
  const packageFile = path.join(root, "package.json");
  const packageJson = JSON.parse(fs.readFileSync(packageFile, "utf8"));
  fs.writeFileSync(packageFile, JSON.stringify({ ...packageJson, private: false }));
  assert.throws(() => verifyImportSafety(root), /legacy package must remain private/);

  fs.copyFileSync(path.join(ROOT, "package.json"), packageFile);
  fs.appendFileSync(path.join(root, ".github", "workflows", "native-promotion.yml"), "\n# npm publish\n");
  assert.throws(() => verifyImportSafety(root), /legacy promotion workflow still contains mutating command: npm publish/);
});

test("import safety rejects routine Dependabot version updates during Phase 0", (context) => {
  const root = fixtureRoot(context);
  const dependabotFile = path.join(root, ".github", "dependabot.yml");
  const config = fs.readFileSync(dependabotFile, "utf8").replace("open-pull-requests-limit: 0", "open-pull-requests-limit: 5");
  fs.writeFileSync(dependabotFile, config);
  assert.throws(() => verifyImportSafety(root), /routine Dependabot version updates must remain disabled/);
});
