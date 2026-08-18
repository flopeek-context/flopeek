"use strict";

const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");

const ROOT = path.resolve(__dirname, "..", "..");
const readWorkflow = (name) => fs.readFileSync(path.join(ROOT, ".github", "workflows", name), "utf8");
const assertThirdPartyActionsPinned = (workflow) => {
  for (const match of workflow.matchAll(/uses:\s+([^\s]+)@([^\s]+)/gu)) {
    assert.match(match[2], /^[a-f0-9]{40}$/u, `${match[1]} must use a full commit SHA`);
  }
};

test("authoritative CI is one Rust and TypeScript/TSX lane without legacy language matrices", () => {
  const workflow = readWorkflow("ci.yml");
  assert.match(workflow, /name: Verify TypeScript authority/);
  assert.match(workflow, /push:\s*\n\s+branches:\s*\[main\]/);
  assert.match(workflow, /pull_request:/);
  assert.match(workflow, /workflow_call:/);
  assert.match(workflow, /ref:\s*\$\{\{ inputs\.source_sha \|\| github\.sha \}\}/);
  assert.match(workflow, /rust-typescript-authority:\s*\n\s+name: Rust \/ TypeScript authority\s*\n\s+runs-on: ubuntu-latest/);
  assert.match(workflow, /uses: actions\/checkout@[a-f0-9]{40}/);
  assert.match(workflow, /uses: actions\/setup-node@[a-f0-9]{40}[\s\S]*node-version: 22/);
  assert.match(workflow, /uses: dtolnay\/rust-toolchain@[a-f0-9]{40}[^\n]*[\s\S]*?toolchain:\s*\$\{\{ steps\.rust-toolchain\.outputs\.channel \}\}/);
  for (const command of ["cargo fmt --check --manifest-path native/flopeek-core/Cargo.toml", "cargo clippy --locked --manifest-path native/flopeek-core/Cargo.toml -- -D warnings", "cargo test --locked --manifest-path native/flopeek-core/Cargo.toml js_facts::tests::", "npm run test:rust-ts-authority", "npm run test:contracts", "npm run check:docs", "npm run check:document-contracts", "npm run audit:package", "npm run verify:import-safety", "node scripts/verify-branch-name.js"]) {
    assert.match(workflow, new RegExp(`- run: ${command.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}`));
  }
  assert.equal((workflow.match(/cargo fmt --check --manifest-path/g) || []).length, 1);
  assert.equal((workflow.match(/cargo clippy --locked --manifest-path/g) || []).length, 1);
  assert.doesNotMatch(workflow, /matrix:|windows-latest|macos-latest|setup-dotnet|setup-go|test:unit|test:native-platform|verify:native-adapter-parity/);
  assert.doesNotMatch(workflow, /\.py\b|\.go\b|\.java\b|\.php\b|\.cs\b|\.svelte\b/);
  assertThirdPartyActionsPinned(workflow);
});

test("dependency automation covers npm and Cargo without auto-merge", () => {
  const dependabot = fs.readFileSync(path.join(ROOT, ".github", "dependabot.yml"), "utf8");
  assert.match(dependabot, /package-ecosystem: npm[\s\S]*directory: \//);
  assert.match(dependabot, /package-ecosystem: cargo[\s\S]*directory: \/native\/flopeek-core/);
  assert.equal((dependabot.match(/open-pull-requests-limit:\s*0/g) || []).length, 3);
  assert.doesNotMatch(dependabot, /auto-merge|automerge/iu);

  const deny = fs.readFileSync(path.join(ROOT, "deny.toml"), "utf8");
  assert.match(deny, /\[advisories\][\s\S]*ignore = \[\]/);
  assert.match(deny, /\[sources\][\s\S]*unknown-registry = "deny"[\s\S]*unknown-git = "deny"/);
});

test("candidate workflow builds each platform once and produces one immutable complete bundle", () => {
  const workflow = readWorkflow("native-candidate.yml");
  assert.match(workflow, /workflow_dispatch:/);
  assert.match(workflow, /name: Full required correctness CI/);
  assert.doesNotMatch(workflow, /Full six-cell correctness CI/);
  for (const input of ["source_sha", "package_version", "release_channel"]) {
    assert.match(workflow, new RegExp(`\\n\\s{6}${input}:`));
  }
  assert.match(workflow, /uses: \.\/\.github\/workflows\/ci\.yml[\s\S]*source_sha: \$\{\{ inputs\.source_sha \}\}/);
  assert.equal((workflow.match(/cargo build --release --locked/g) || []).length, 1);
  assert.match(workflow, /matrix:[\s\S]*@flopeek\/native-win32-x64[\s\S]*@flopeek\/native-win32-arm64[\s\S]*@flopeek\/native-linux-x64-gnu[\s\S]*@flopeek\/native-linux-arm64-gnu[\s\S]*@flopeek\/native-darwin-x64[\s\S]*@flopeek\/native-darwin-arm64/);
  assert.match(workflow, /tar -xOf "\$tarball" package\/bin\/flopeek-native-core/);
  assert.match(workflow, /run-native-candidate-evidence\.js/);
  assert.match(workflow, /create-native-dogfood-pending\.js/);
  assert.match(workflow, /build-native-rollout-evidence\.js/);
  assert.match(workflow, /--dogfood "\$CANDIDATE_EVIDENCE\/native-dogfood\.json"/);
  assert.match(workflow, /CANDIDATE_BUNDLE=\$\{\{ runner\.temp \}\}\/flopeek-native-bundle/);
  assert.match(workflow, /npm pack --json --pack-destination \$env:CANDIDATE_BUNDLE/);
  assert.match(workflow, /--output "\$CANDIDATE_EVIDENCE\/native-rollout-evidence\.json"/);
  assert.match(workflow, /build-native-release-manifest\.js/);
  assert.match(workflow, /name: Clean install \$\{\{ matrix\.package \}\}/);
  assert.match(workflow, /--require-platform-install-evidence/);
  assert.match(workflow, /pattern: native-candidate-install-\*/);
  assert.match(workflow, /name: native-candidate-bundle/);
  assert.doesNotMatch(workflow, /npm publish|gh release create|git push/);
  assert.doesNotMatch(workflow, /continue-on-error|\|\|\s*true/);
  assertThirdPartyActionsPinned(workflow);
});

test("native dogfood workflow accumulates only exact revision-bound UTC days", () => {
  const workflow = readWorkflow("native-dogfood.yml");
  assert.match(workflow, /workflow_dispatch:/);
  for (const input of ["source_sha", "candidate_run_id", "binary_sha256", "previous_dogfood_run_id"]) {
    assert.match(workflow, new RegExp(`\\n\\s{6}${input}:`));
  }
  assert.match(workflow, /actions\/download-artifact@[a-f0-9]{40}[\s\S]*run-id: \$\{\{ inputs\.candidate_run_id \}\}/);
  assert.match(workflow, /run-native-dogfood-day\.js/);
  assert.match(workflow, /build-native-dogfood-evidence\.js/);
  assert.match(workflow, /verify-native-dogfood\.js/);
  assert.doesNotMatch(workflow, /continue-on-error|\|\|\s*true/);
  assertThirdPartyActionsPinned(workflow);
});

test("legacy promotion workflow is read-only and cannot publish or release", () => {
  const workflow = readWorkflow("native-promotion.yml");
  assert.match(workflow, /workflow_dispatch:/);
  assert.match(workflow, /permissions:\s*\n\s+contents: read/);
  assert.match(workflow, /node scripts\/verify-import-safety\.js/);
  assert.doesNotMatch(workflow, /npm publish|npm dist-tag add|git push|gh release (?:create|upload)/);
  assert.doesNotMatch(workflow, /id-token:\s*write/);
  assertThirdPartyActionsPinned(workflow);
});
