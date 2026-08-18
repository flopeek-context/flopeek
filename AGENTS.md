# AGENTS.md

## 0. Status and authority

This file is the **single human-readable source of truth** for this repository.

Canonical repository:

`github.com/flopeek-context/flopeek`

Historical import provenance:

- Source repository: `badsleepyday/flopeek-core`
- Source branch: `development`
- Imported baseline commit: `72a95fe1a6497683e96e90872438cd3c83b7272f`
- Baseline date: 2026-08-09
- Baseline role: **historical implementation provenance only**

The historical repository is not an active upstream, roadmap authority, release authority, or synchronization source.

The active Flopeek product line lives here.

Current `main` may and should diverge from the historical baseline.

---

## 1. Single-source-of-truth documentation model

`AGENTS.md` is the only authoritative prose document for:

- product direction;
- architecture authority;
- current scope;
- non-goals;
- migration policy;
- implementation priority;
- evidence semantics;
- language scope;
- storage authority;
- CI policy;
- definition of done.

Do not create parallel authority documents such as:

- `PRODUCT.md`
- `ARCHITECTURE.md`
- `ROADMAP.md`
- `DEVELOPMENT_STATUS.md`

unless this authority model is explicitly changed first.

Other documentation may exist only in these roles:

1. **README / user documentation**
   - explains current behavior to users;
   - does not define independent product direction.

2. **ADR**
   - records why a historical architectural decision was made;
   - does not override current rules in this file.

3. **Generated reference/capability documentation**
   - generated from code or machine-readable contracts;
   - describes implementation state;
   - does not create product scope.

4. **Release/security/contribution documentation**
   - operational only;
   - does not redefine architecture or roadmap.

If another document conflicts with this file, this file wins.

Agents MUST NOT introduce a new product, architecture, or roadmap source-of-truth document without an explicit request to change this documentation authority model.

---

## 2. Product purpose

Flopeek is:

> **Versioned repository engineering context, deterministic code evidence, diagnostic memory, and historical diagnosis for humans and coding agents.**

Flopeek is not an AI chatbot.

Flopeek is not a generic knowledge graph.

Flopeek is not a generic RAG platform.

Flopeek is not a project-management platform.

Flopeek is not an automatic root-cause oracle.

The product exists to answer questions such as:

- What source state are we talking about?
- What is demonstrably present in the repository?
- Which context is current or stale?
- Why are we investigating this code?
- What changed since last-known-good?
- Which historical changes are relevant candidates?
- What do humans/agents currently believe?
- Which conclusions are verified?
- What still requires source inspection or runtime verification?

---

## 3. Final architectural authority

The architecture is intentionally strict.

### 3.1 Implementation authority

**Rust is the single implementation authority for Flopeek core.**

Rust owns:

- repository discovery;
- source inventory;
- TypeScript/TSX parsing;
- module/import resolution;
- deterministic structural facts;
- graph construction;
- graph identity;
- graph versioning;
- graph delta;
- Context Ref resolution;
- Context Ref freshness;
- Git historical reconstruction;
- historical candidate generation;
- diagnostic context;
- diagnostic assertions;
- diagnostic queries;
- diagnostic packet composition;
- persistence lifecycle;
- canonical CLI behavior.

### 3.2 Persisted authority

**SQLite is the canonical persisted authority.**

New authoritative product state must not be split across:

- JavaScript JSON stores;
- ad-hoc cache files;
- vector databases;
- external services;
- mandatory cloud stores.

Portable export formats may exist, but they are not authoritative storage.

### 3.3 Target language

V1 analyzes:

- TypeScript (`.ts`)
- TSX (`.tsx`)

TypeScript/TSX is the **target analyzed language**, not the implementation language.

### 3.4 JavaScript role

JavaScript/Node.js is not repository-truth authority.

JavaScript may temporarily exist only as:

- migration material;
- compatibility oracle;
- optional integration host;
- optional UI/viewer layer;
- optional Node binding;
- temporary protocol adapter.

JavaScript MUST NOT independently recreate repository truth for new product capabilities.

### 3.5 LLM role

LLMs are optional consumers and reasoning clients.

LLMs do not own repository truth.

LLMs may:

- explain evidence;
- propose hypotheses;
- draft findings;
- suggest remediation;
- summarize historical candidates.

LLMs may not:

- create parser facts;
- determine canonical graph identity;
- determine Context Ref freshness;
- silently convert hypotheses into deterministic truth;
- become mandatory for scanning;
- become mandatory for persistence;
- become mandatory for diagnosis storage.

---

## 4. Core architectural invariant

Preserve this invariant above all:

> **Rust computes repository truth. SQLite persists authoritative state. Engineering understanding is versioned and attributed. AI reasoning is optional and is never silently promoted to repository truth.**

Equivalent shorthand:

```text
Rust       = computation authority
SQLite     = persisted authority
TS / TSX   = analyzed language
JavaScript = compatibility/presentation only
LLM        = optional reasoning consumer
```

---

## 5. Product authority gate

Legacy baseline behavior is **not automatically active product scope**.

Existing inherited code may remain temporarily during cutover without being an active product direction.

Every new product change MUST trace directly to one of:

1. Rust authority cutover
2. TypeScript/TSX repository evidence
3. Diagnostic Context
4. Diagnostic Assertion
5. Historical Diagnosis
6. Diagnostic Packet
7. correctness/security/performance/infrastructure strictly required by 1–6

If a proposed feature cannot be traced to these areas, do not implement it without an explicit product-direction change.

Use this rule:

> **Inherited capabilities are migration substrate, not roadmap commitments.**

The mere existence of a subsystem is not justification to keep or expand it.

---

## 6. Historical baseline policy

The imported commit:

`72a95fe1a6497683e96e90872438cd3c83b7272f`

is not the final architecture baseline.

It is the **historical import baseline**.

The repository must establish a new baseline after destructive Rust cutover.

Expected lifecycle:

```text
historical import baseline
        |
        v
Rust authority cutover
        |
        v
new Rust foundation baseline
```

After the cutover is complete and tests pass, create a new baseline tag, for example:

`rust-foundation-v1`

The exact tag name may change, but the concept is required.

---

## 7. Repository independence

Do not continuously synchronize from the historical repository.

Agents MUST NOT:

- merge old `main` automatically;
- merge old `development` automatically;
- mirror old branches;
- rebase onto the historical repository;
- silently cherry-pick old changes;
- treat old release evidence as current evidence;
- use old roadmap priority as current priority.

A historical change may be imported only when explicitly justified.

Every imported historical change must record:

- source repository;
- exact source SHA;
- reason;
- files/behavior imported;
- compatibility impact;
- local tests;
- confirmation that product scope was not silently broadened.

---

## 8. Flopeek identity policy

This repository remains Flopeek.

Preserve by default:

- product name `Flopeek`;
- CLI name `flopeek`;
- `.flopeek/`;
- Context Ref scheme such as `fp://...`;
- technically valid `flopeek-*` schemas/protocol names.

Repository separation does not require rebranding Flopeek.

What must change is authority-specific identity, such as:

- old repository URLs;
- old issue URLs;
- old release destinations;
- old package provenance;
- old CI ownership assumptions;
- old publication approvals;
- old release approval records.

Do not perform unnecessary brand renames.

---

## 9. V1 language scope

V1 product support is intentionally narrow:

```text
TypeScript
TSX
```

No V1 diagnostic parity commitment exists for:

- JavaScript
- JSX
- Python
- Go
- Java
- PHP
- C#
- Rust source analysis
- Svelte
- other languages

Inherited support for other languages must not force:

- CI installation;
- parser dependencies;
- fixtures;
- benchmarks;
- support documentation;
- compatibility tests.

The product should become excellent for one language family before expanding.

---

## 10. V1 TypeScript evidence contract

V1 may support deterministic evidence such as:

- source files;
- declarations;
- classes;
- functions;
- methods;
- imports;
- module relationships;
- supported direct calls;
- supported entry points;
- supported route/handler relationships;
- graph nodes;
- graph edges;
- flow projections;
- related-test relationships where statically supported;
- Git revision;
- graph version;
- graph delta;
- current/stale Context Refs.

Unsupported dynamic behavior must remain explicit.

Do not guess:

- reflection;
- runtime DI resolution;
- dynamic dispatch;
- runtime-only module selection;
- generated behavior not visible in analyzed source;
- runtime causality.

---

## 11. Explicit V1 non-goals

Do not add:

- mandatory LLM integration;
- built-in foundation models;
- generic vector search as authority;
- generic RAG platform;
- generic knowledge graph;
- ontology engine;
- generic workflow engine;
- generic planning system;
- project-management expansion;
- multi-repository diagnosis;
- cross-project automatic graph edges;
- multi-language parity;
- automatic root-cause truth;
- dynamic-dispatch guessing;
- reflection inference;
- arbitrary business-intent inference;
- arbitrary shell execution through MCP;
- automatic target-source mutation through diagnostic tools;
- WebGL migration;
- unrelated framework adapters.

---

## 12. Destructive cutover policy

This repository is allowed to deliberately delete inherited implementation.

Git history is the archive.

Do not preserve dead implementation in:

- `legacy/`
- `old/`
- `archive/`
- `compat-old/`

unless a specific migration test requires a short-lived copy.

The goal is a clean active tree, not permanent historical baggage.

Before deleting an inherited implementation:

1. extract any TypeScript/TSX fixture needed for parity;
2. confirm equivalent Rust behavior required by V1;
3. add Rust tests;
4. delete the inherited implementation.

Do not keep an entire subsystem merely because one fixture is useful.

---

## 13. Explicit deletion plan

The following inherited areas are scheduled for deletion or complete replacement.

### 13.1 Delete JavaScript core

Delete the active JavaScript core implementation under:

```text
src/
```

after required TypeScript/TSX fixtures have been extracted.

Do not retain JavaScript equivalents of:

- scanner;
- graph builder;
- history engine;
- Context Ref authority;
- graph version authority;
- semantic proposal engine;
- workflow engine;
- continuation authority;
- core CLI implementation;
- repository-truth queries.

### 13.2 Delete dual-core selection

Delete all concepts and code for:

```text
js
shadow
native
native-experimental
FLOPEEK_CORE
automatic JavaScript fallback
native rollout gating
native promotion
native shadow comparison
```

The user does not choose a core.

There is one core:

```text
Flopeek Core = Rust
```

If Rust core is unavailable, return an explicit error/unavailable state.

Do not silently fall back to JavaScript.

### 13.3 Delete root Node package

After Rust CLI/core cutover, delete root:

```text
package.json
package-lock.json
```

The repository root must become a Rust workspace.

If a viewer or Node binding is needed later, give it its own isolated package:

```text
viewer/package.json
bindings/node/package.json
```

Do not restore Node as root authority.

### 13.4 Delete JavaScript scripts

Delete the inherited JavaScript automation layer under:

```text
scripts/
```

especially:

- `benchmark-native-*`
- `build-native-*`
- `capture-native-*`
- `compare-native-js-*`
- `create-native-*`
- `profile-native-*`
- `run-native-*`
- `verify-native-*`
- `verify-npm-*`
- `verify-published-*`
- `verify-github-release-*`
- `verify-clean-room*`
- `generate-go-stdlib*`
- old documentation generators tied to removed docs
- old dual-core baseline scripts

If developer automation is required, prefer:

```text
crates/xtask/
```

or small Rust binaries.

### 13.5 Delete old rollout workflows

Delete:

```text
.github/workflows/native-candidate.yml
.github/workflows/native-dogfood.yml
.github/workflows/native-promotion.yml
.github/workflows/real-corpus.yml
```

These workflows belong to the old question:

> Is native ready to replace JavaScript?

That question no longer exists.

### 13.6 Replace CI completely

Do not incrementally patch the old `ci.yml`.

Rewrite it from zero.

Normal PR/main CI must not install:

- Node.js;
- npm dependencies;
- Go;
- .NET;
- multi-language toolchains.

Normal PR/main CI must not run:

- Node 22/24 matrices;
- JS unit suites;
- JS contracts;
- Rust-vs-JS parity;
- native adapter parity;
- Go stdlib checks;
- multi-language dogfood;
- native promotion;
- native candidate;
- npm packaging;
- npm clean-room;
- full release packaging.

### 13.7 Delete non-TypeScript Rust analyzers

Remove active V1 modules such as:

```text
csharp_facts.rs
go_facts.rs
java_facts.rs
php_facts.rs
python_facts.rs
rust_facts.rs
svelte_facts.rs
```

Remove their module declarations.

### 13.8 Delete non-TypeScript parser dependencies

Remove inherited dependencies that are not needed for TypeScript/TSX V1, including when no longer referenced:

```text
tree-sitter-c-sharp
tree-sitter-go
tree-sitter-java
tree-sitter-php
tree-sitter-python
tree-sitter-rust
tree-sitter-svelte-next
syn
```

Keep only dependencies required by the new Rust core.

`tree-sitter-javascript` should also be removed once TypeScript/TSX code no longer requires it.

### 13.9 Rename JS-oriented Rust modules

Rust files named around historical JS architecture must be refactored when they are actually TypeScript authority.

For example, move away from:

```text
js_facts.rs
js_resolver.rs
js_batch.rs
```

toward:

```text
typescript/
  mod.rs
  facts.rs
  resolver.rs
  batch.rs
```

Remove `.js/.jsx` analysis behavior from the V1 product contract.

### 13.10 Delete old JavaScript tests

After extracting required TypeScript/TSX fixtures, delete the inherited Node test suite:

```text
test/
```

Rebuild tests in Rust.

Do not keep a 1:1 JS test suite indefinitely.

### 13.11 Delete non-TypeScript fixtures

Keep only fixtures relevant to TypeScript/TSX V1.

Delete inherited fixtures for:

- Python;
- Django;
- PHP;
- Go;
- Java;
- C#;
- Rust source analysis;
- Svelte;
- CommonJS-only behavior;
- unrelated runners/frameworks.

Move retained fixtures into:

```text
fixtures/typescript/
```

### 13.12 Delete legacy benchmark evidence

Delete or reset inherited:

```text
benchmarks/
```

including old:

- JS core baseline;
- native adapter corpus;
- native rollout benchmark;
- agent comparison;
- orientation benchmarks;
- semantic suggestions;
- old dogfood;
- old real-repository corpus;
- renderer corpus.

Historical benchmark results are not authority for the new product.

New benchmarks may later be created under:

```text
benchmarks/typescript/
```

### 13.13 Delete old packaging/native evidence

Delete inherited rollout/package evidence that exists only for dual-core promotion, including:

```text
packaging/native-rollout-evidence.json
packaging/evidence/*
```

and related generated native-promotion manifests.

### 13.14 Delete old authority documents

Because this file is the source of truth, delete:

```text
ARCHITECTURE.md
PRODUCT.md
ROADMAP.md
DEVELOPMENT_STATUS.md
SUPPORT.md
BENCHMARKS.md
RELEASING.md
```

unless a specific operational reason is documented first.

Keep only documents with a distinct non-authority role, such as:

```text
README.md
CONTRIBUTING.md
SECURITY.md
LICENSE
```

Reset `CHANGELOG.md` when the new release line begins if useful.

### 13.15 Remove old showcase/viewer/integration scope

During Rust foundation rewrite, remove or defer inherited:

```text
public/
examples/commerce-showcase/
integrations/skills/flopeek/
```

unless a specific file is required for a migration test.

Rebuild user-facing surfaces after the Rust product contract is stable.

### 13.16 Remove multi-language contracts

Remove inherited multi-language contracts such as:

```text
contracts/adapter-capabilities.json
contracts/go-stdlib-catalog.json
```

and their generators.

Create a small new machine contract only for active invariants.

### 13.17 Remove unused toolchain files

Remove infrastructure used only by deleted language support, such as:

- Go toolchain configuration;
- .NET toolchain configuration;
- related CI setup;
- related generated catalogs.

---

## 14. Target repository layout

The target active tree should converge toward:

```text
flopeek/
├── AGENTS.md
├── README.md
├── LICENSE
├── SECURITY.md
├── CONTRIBUTING.md
│
├── Cargo.toml
├── Cargo.lock
├── rust-toolchain.toml
│
├── crates/
│   ├── flopeek-core/
│   │   ├── src/
│   │   │   ├── discovery/
│   │   │   ├── typescript/
│   │   │   ├── graph/
│   │   │   ├── context/
│   │   │   └── history/
│   │
│   ├── flopeek-storage/
│   │   └── src/
│   │       └── sqlite/
│   │
│   ├── flopeek-diagnostics/
│   │   └── src/
│   │       ├── context/
│   │       ├── assertion/
│   │       ├── historical/
│   │       └── packet/
│   │
│   ├── flopeek-protocol/
│   ├── flopeek-cli/
│   └── xtask/
│
├── contracts/
│   └── product.json
│
├── fixtures/
│   └── typescript/
│
└── .github/
    └── workflows/
        └── ci.yml
```

Do not create unnecessary crates early.

A single crate may temporarily contain multiple modules if that keeps the cutover simpler.

Split only when module boundaries become real.

---

## 15. Rust core modules to preserve/refactor

Useful inherited Rust foundations may be preserved/refactored if they support V1, for example:

- facts;
- graph;
- identity;
- identity store;
- identity v2;
- inventory;
- project identity;
- protocol;
- record cache;
- scope;
- source text;
- store;
- structural contract;
- structural graph.

Do not preserve them blindly.

Each retained module must satisfy at least one active V1 requirement.

---

## 16. SQLite authority model

SQLite must become the canonical product store for:

- graph versions;
- nodes;
- edges;
- flow state;
- Context Ref state;
- source revision linkage;
- diagnostic contexts;
- diagnostic revisions;
- diagnostic assertions;
- assertion evidence references;
- historical candidates;
- verification state.

Recommended logical domains:

```text
repository evidence
context identity
historical state
diagnostics
```

A single SQLite database is allowed.

Domain boundaries must remain explicit.

Do not use JSON files as the primary authority for new diagnostic state.

JSON may be used only for:

- export;
- test fixtures;
- portable snapshots;
- generated reports.

---

## 17. Evidence hierarchy

Never collapse deterministic evidence and human/agent reasoning into one class.

### 17.1 Deterministic evidence

Produced from source/Git/Rust analysis.

Examples:

- node exists;
- import exists;
- supported direct call exists;
- graph changed;
- edge introduced;
- flow changed;
- Context Ref current/stale.

### 17.2 Observation

Reported/measured behavior.

Examples:

- regression test failed;
- timeout reproduced;
- operator observed duplicate charge.

Observation is not automatically cause.

### 17.3 Hypothesis

A proposed explanation.

May come from:

- human;
- agent;
- LLM.

Never deterministic truth.

### 17.4 Finding

An attributed audit/review conclusion supported by evidence.

### 17.5 Remediation

A proposed or implemented correction.

### 17.6 Verification

Records confirmation, rejection, supersession, implementation, or regression verification.

---

## 18. Diagnostic Context

`DiagnosticContext` records why investigation exists.

Minimum semantics:

- id;
- project identity;
- diagnostic revision;
- intent;
- symptom;
- expected behavior;
- focused Context Refs;
- current graph basis;
- optional last-known-good basis;
- constraints;
- acceptance criteria;
- unresolved questions;
- actor;
- createdAt;
- status;
- revision lineage.

V1 intents:

```text
diagnose
audit
verify-fix
```

---

## 19. Diagnostic Assertion

Use one attributed assertion model.

Kinds:

```text
observation
hypothesis
finding
remediation
verification
```

Statuses may include:

```text
proposed
confirmed
rejected
superseded
implemented
verified
```

Every assertion must identify its actor.

Every technical assertion must reference supporting evidence or explicitly state that evidence is unavailable.

---

## 20. Historical Candidate

`HistoricalCandidate` is deterministic output.

It may report that a change:

- happened after last-known-good;
- touched focused context;
- changed focused node/edge/flow;
- remains present now;
- lies on current dependency cone;
- changed related-test structure.

It must never claim:

```text
root cause
this commit caused the bug
runtime causality
business intent
```

Use terms:

```text
candidate-change
historically-relevant-change
requires-diagnosis
```

---

## 21. Version model

Do not overload versions.

### Repository graph version

Represents repository evidence state.

### Diagnostic revision

Represents evolution of engineering understanding.

A new hypothesis or finding does not advance graph version.

A source change does not automatically rewrite historical assertions.

Every repository-bound diagnostic item must be resolvable as:

```text
current
stale
superseded
unavailable
unresolved
```

---

## 22. Historical diagnosis contract

Historical diagnosis is a main product differentiator.

Input:

- current TypeScript/TSX Context Ref;
- current repository revision;
- optional last-known-good revision;
- bounded historical range.

Output:

- deterministic candidate changes;
- candidate relevance reasons;
- retained/changed/removed state;
- explicit limits;
- explicit truncation;
- no root-cause claim.

Initial algorithm:

1. resolve current revision;
2. resolve last-known-good if supplied;
3. enumerate bounded commit range;
4. reuse cached immutable snapshots;
5. compare adjacent graph states;
6. intersect deltas with focused current context;
7. rank candidates using explainable rules;
8. return evidence and limitations.

Possible relevance rules:

```text
+ edge introduced and still present
+ focused flow changed
+ focused node changed
+ change lies in current dependency cone
+ related test structure changed
+ happened after last-known-good
- later removed
- outside focused context
```

Any score must be deterministic and explainable.

---

## 23. Bounded history

Never scan unbounded Git history by default.

Define explicit limits for:

- commits inspected;
- bytes loaded;
- snapshots materialized;
- candidates returned;
- paths returned;
- Context Refs returned;
- diagnostic assertions;
- diagnostic packet size.

When limits are reached:

```text
truncated = true
```

State exactly what was omitted.

Do not pretend partial results are complete.

---

## 24. Diagnostic Packet

Build a bounded packet for humans/agents.

It may include:

- intent;
- symptom;
- expected behavior;
- current revision;
- graph version;
- last-known-good;
- focused Context Refs;
- freshness;
- compact node/flow evidence;
- historical candidates;
- observations;
- hypotheses;
- findings;
- remediation;
- verification state;
- acceptance criteria;
- unresolved questions;
- limits;
- omissions.

The packet must preserve evidence classes.

Do not include private model chain-of-thought.

---

## 25. Machine-enforced product contract

Maintain a small machine-readable product contract.

Suggested shape:

```json
{
  "canonicalRepository": "flopeek-context/flopeek",
  "coreImplementation": "rust",
  "persistedAuthority": "sqlite",
  "primaryAnalyzedLanguages": ["typescript", "tsx"],
  "llmRequired": false,
  "javascriptRepositoryAuthority": false,
  "automaticRootCauseClaims": false
}
```

This contract is not a second product authority.

`AGENTS.md` remains the human-readable source of truth.

The contract exists so CI can prevent accidental architectural regression.

---

## 26. CI policy

V1 CI must be intentionally small.

### 26.1 Normal PR/main CI

Use one primary required Rust job.

Minimum:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
```

That is the default CI contract.

Do not add matrix jobs without evidence that they are necessary.

### 26.2 Security/dependency CI

May run separately on a schedule:

```text
cargo deny
advisory scanning
```

This should not make normal PR feedback unnecessarily slow.

### 26.3 Release CI

Cross-platform builds belong to release/tag/manual workflows.

Release-only targets may include:

- Linux x64;
- Windows x64;
- macOS arm64;
- macOS x64 if required.

Do not run full release packaging on every pull request.

### 26.4 CI non-goals

Normal CI must not contain:

- Node version matrix;
- npm clean-room;
- npm publication verification;
- JavaScript parity;
- Go setup;
- .NET setup;
- multi-language adapter tests;
- native promotion;
- native dogfood;
- native candidate;
- historical release evidence from the old product.

---

## 27. Test policy

New tests must be Rust-first.

Required categories:

- parser tests;
- TypeScript/TSX fixture tests;
- graph identity tests;
- graph delta tests;
- Context Ref freshness tests;
- SQLite migration tests;
- SQLite recovery tests;
- diagnostic context tests;
- assertion lifecycle tests;
- bounded history tests;
- truncation tests;
- historical candidate false-positive controls;
- deterministic ordering tests;
- no-secret persistence tests;
- no-source-body export tests where relevant.

Do not reproduce the entire old JS suite.

Test active product contracts only.

---

## 28. Required TypeScript diagnostic fixture

Create a deterministic fixture history.

Minimum story:

```text
A: checkout/payment is last-known-good
B: retry path introduced
C: unrelated change
D: timeout branch changed
E: current/bad state
```

Tests must prove:

- unrelated change ranks low or is excluded;
- retry change is historically relevant;
- timeout change is historically relevant;
- neither is declared root cause;
- stale Context Refs are surfaced;
- last-known-good remains explicit;
- historical candidates are reproducible;
- assertion lifecycle remains separate from deterministic candidate output.

---

## 29. Migration sequence

Use an explicit cutover branch, for example:

```text
rewrite/rust-foundation
```

Recommended sequence:

1. preserve historical import SHA;
2. commit final `AGENTS.md`;
3. create/normalize root Rust workspace;
4. extract TypeScript/TSX fixtures from old tests;
5. move/refactor inherited Rust core into canonical workspace;
6. establish TypeScript/TSX Rust parser authority;
7. establish Rust graph/query authority;
8. establish SQLite single authority;
9. add Rust CLI;
10. add new Rust tests;
11. remove non-TypeScript Rust parsers;
12. remove unused language dependencies;
13. delete `src/`;
14. delete old `test/`;
15. delete `scripts/`;
16. delete root Node package files;
17. delete old benchmark evidence;
18. delete old packaging/native rollout evidence;
19. delete old authority documents;
20. delete old showcase/integration scope;
21. replace all old workflows with one minimal Rust CI;
22. run full Rust test suite;
23. verify product contract;
24. merge to `main`;
25. create new Rust foundation baseline tag.

Do not interleave unrelated feature expansion during this migration.

### Rust foundation hardening decisions

The foundation hardening is implemented as two ordered changes before the
`rust-foundation-v1` tag. SQLite migrations are transactional and monotonic:
the runner reads `PRAGMA user_version`, executes each migration inside
`BEGIN IMMEDIATE`, records the new version only at the end, and rejects a
database newer than the supported version. Diagnostic contexts and assertions
are authoritative and are retained during upgrades; historical candidates are
derived evidence and may be rebuilt or removed when their schema is rebuilt.

Every Git/source state is represented by an immutable graph observation. A
graph ID is structural TypeScript/TSX evidence and is independent of Git SHA,
raw byte count, and raw source hash. Observation identity retains the exact
source fingerprint, Git revision, dirty state, and graph version for provenance.

Context Ref freshness is node-level and deterministic. The v2 scope is a
canonical AST/evidence fingerprint plus sorted direct edge signatures; it does
not include neighbour fingerprints or transitive dependencies. Formatting and
comments are ignored. Resolution returns the origin basis, current basis,
fingerprint scope, and a deterministic reason. Legacy references use a
conservative file-level fallback when source evidence exists and are otherwise
`unresolved`; they are never silently treated as current.

Historical diagnosis remains bounded and candidate-only. A non-root commit is
compared explicitly with its first parent, root commits with the empty tree,
and rename/copy records retain both paths. Dirty or source-mismatched working
copies make historical candidates `unavailable`, while current static evidence
may still be returned in a packet. No candidate is a root-cause claim.

---

## 30. Active priority

### P0 — Rust foundation cutover

- canonical repository authority;
- root Rust workspace;
- TypeScript/TSX Rust source authority;
- Rust graph/query authority;
- SQLite authority;
- Rust CLI;
- removal of JS fallback;
- removal of legacy CI;
- destructive cleanup.

### P1 — Diagnostic Context

Rust implementation.

### P2 — Diagnostic Assertion

Rust implementation.

### P3 — Historical Diagnosis

Rust implementation.

### P4 — Diagnostic Packet

Rust implementation.

### P5 — Human/agent surfaces

Only after P0–P4 are stable:

- MCP;
- Viewer;
- Node binding if justified;
- editor integration.

---

## 31. Branch policy

`main` is canonical.

Use short-lived branches.

Recommended:

```text
rewrite/rust-foundation
feature/diagnostic-context
feature/historical-diagnosis
fix/<specific-problem>
test/<specific-gap>
```

Do not use model/vendor names in branch names.

Avoid mixing:

- destructive migration + unrelated feature;
- parser semantic change + UI work;
- SQLite migration + viewer work;
- architecture change + release packaging.

---

## 32. Definition of done for Rust foundation

Rust foundation cutover is done only when:

- root is a Rust workspace;
- Rust is the only repository-truth implementation;
- TypeScript/TSX parsing is Rust-owned;
- graph/query authority is Rust-owned;
- SQLite is authoritative;
- Rust CLI exists;
- JS core is deleted;
- JS fallback is deleted;
- dual-core modes are deleted;
- root Node package is deleted;
- non-TypeScript analyzers are removed from active V1;
- non-TypeScript fixtures are removed;
- old rollout workflows are deleted;
- old CI is replaced;
- normal CI is Rust-only;
- old authority docs are deleted;
- machine product contract passes;
- Rust tests pass.

---

## 33. Definition of done for diagnostic features

A diagnostic feature is done only when:

- implementation is Rust;
- persistence is SQLite where authoritative state is required;
- no LLM is required;
- TypeScript/TSX scope is explicit;
- evidence class is explicit;
- repository basis is explicit;
- graph version is explicit;
- freshness is explicit;
- historical limits are explicit;
- assertions are attributed;
- static history never declares root cause;
- tests pass;
- output is deterministic where claimed;
- no secret/private reasoning is persisted;
- docs do not create a second source of truth.

---

## 34. Agent safety rules

Agents must not:

- send repository source to external services without explicit user request;
- add telemetry by default;
- add mandatory LLM credentials;
- expose arbitrary shell through MCP;
- invent historical evidence;
- invent last-known-good;
- claim runtime behavior from static edges;
- treat missing evidence as proof of absence;
- silently broaden beyond TypeScript/TSX;
- restore JavaScript as repository authority;
- restore automatic JS fallback;
- restore multi-language CI;
- preserve dead code just for history;
- create parallel roadmap/product/architecture authority docs;
- publish using historical repository approvals/credentials.

When evidence is missing:

```text
unavailable
```

When context is stale:

```text
stale
```

When historical relevance is not causality:

```text
candidate
```

Never say:

```text
cause
root cause
```

unless separately verified by an appropriate evidence process.

---

## 35. Agent response discipline

When implementing or auditing, classify conclusions as:

- **Deterministic evidence**
- **Observation**
- **Hypothesis**
- **Finding**
- **Remediation**
- **Verification**
- **Unknown / unavailable**

For historical diagnosis always report:

- current revision;
- graph version;
- last-known-good if supplied;
- historical range inspected;
- limits;
- truncation;
- candidate changes;
- candidate relevance reasons;
- unresolved runtime/dynamic behavior.

---

## 36. Final product boundary

Flopeek succeeds when:

```text
Repository
    |
    v
Rust deterministic analysis
    |
    v
SQLite versioned evidence
    |
    +--> Context identity
    +--> Historical change
    +--> Diagnostic memory
    |
    v
Bounded diagnostic context
    |
    +--> Human
    +--> Coding agent
    +--> Optional LLM
```

The purpose is not to replace reasoning.

The purpose is to give reasoning systems a stable, versioned, deterministic repository substrate.

Keep the product narrow.

Keep the engine Rust.

Keep authoritative state in SQLite.

Keep V1 focused on TypeScript/TSX.

Delete inherited complexity that does not serve that goal.
