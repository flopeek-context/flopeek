# AGENTS.md

## 0. Status and authority

This file is the **single human-readable source of truth** for this repository.

Canonical repository:

`github.com/flopeek-context/flopeek`

Historical implementation provenance:

- source repository: `badsleepyday/flopeek-core`
- source branch: `development`
- imported baseline commit: `72a95fe1a6497683e96e90872438cd3c83b7272f`
- baseline date: `2026-08-09`
- baseline role: **historical provenance only**

The historical repository is not an active upstream, roadmap authority, release authority, publication authority, or synchronization source.

Current `main` is the canonical product line.

---

## 1. Documentation authority

`AGENTS.md` is the only authoritative prose document for:

- product identity;
- product direction;
- architecture authority;
- active scope;
- non-goals;
- evidence semantics;
- implementation authority;
- persistence authority;
- language policy;
- migration policy;
- active priorities;
- CI policy;
- definition of done.

Do not create parallel authority documents such as:

- `PRODUCT.md`
- `ARCHITECTURE.md`
- `ROADMAP.md`
- `DEVELOPMENT_STATUS.md`
- `DESIGN.md`

unless this authority model is explicitly changed first.

Other documents may exist only in distinct non-authority roles.

### README and user documentation

May explain:

- what Flopeek currently does;
- installation;
- usage;
- examples;
- commands;
- current support.

It must not define independent product direction.

### ADRs

May explain why an architectural decision was made.

ADRs are historical records.

They do not override this file.

### Generated reference documentation

May describe implementation state when derived from:

- code;
- schemas;
- capability registries;
- machine-readable contracts.

Generated reference documentation does not create product scope.

### Operational documentation

Files such as:

- `SECURITY.md`
- `CONTRIBUTING.md`
- release notes
- changelogs

may exist for operational purposes only.

If another document conflicts with this file, this file wins.

---

## 2. Product identity

Flopeek is:

> **A deterministic, versioned repository context and engineering-memory layer for humans and coding agents.**

Flopeek computes repository evidence, tracks where and when that evidence was observed, determines whether referenced context remains valid, reconstructs bounded historical context, and preserves attributed engineering understanding.

Flopeek is not primarily:

- a code-review graph;
- a static analyzer product;
- a code search engine;
- a RAG system;
- a vector database;
- a generic knowledge graph;
- a project-management system;
- an AI chatbot;
- an automatic debugger;
- an automatic root-cause oracle.

The core distinction is:

> **The graph is deterministic substrate. Versioned repository context and engineering memory are the product.**

---

## 3. Product North Star

Flopeek succeeds when repository context can be:

1. computed once;
2. identified precisely;
3. reused by multiple humans and agents;
4. tied to exact repository observations;
5. checked later for freshness;
6. compared across history;
7. combined with attributed engineering memory;
8. transferred between actors without reconstructing understanding from scratch.

The product should optimize for:

- exact repository-state attribution;
- durable Context Refs;
- deterministic freshness;
- historical continuity;
- provenance;
- bounded reusable context;
- engineering-memory preservation;
- reliable human/agent handoff.

Do not use these as primary product-success metrics:

- number of supported languages;
- number of graph edges;
- maximum graph size;
- token-reduction headline;
- blast-radius score;
- risk score;
- number of framework adapters.

Those may be useful measurements, but they are not the product identity.

---

## 4. Product boundary

Conceptually:

```text
Repository Source
      |
      v
Language Intelligence
      |
      v
Deterministic Evidence
      |
      v
Graph Identity
      |
      +----------------------+
      |                      |
      v                      v
Immutable Observation     Context Ref
      |                      |
      |                   Freshness
      |                      |
      +----------+-----------+
                 |
                 v
        Historical Context
                 |
                 v
        Engineering Memory
                 |
      +----------+----------+
      |          |          |
      v          v          v
    Human      Agent A    Agent B
```

The graph represents structural repository evidence.

An observation represents where and when a graph was observed.

A Context Ref is a durable reference into repository knowledge.

Engineering memory records attributed understanding about that context.

---

## 5. Core implementation authority

### 5.1 Rust authority

**Rust is the single implementation authority for Flopeek repository truth and authoritative product behavior.**

Rust owns:

- repository discovery;
- source inventory;
- TypeScript/TSX parsing;
- import/module resolution;
- deterministic structural facts;
- graph construction;
- graph identity;
- graph versioning;
- graph delta;
- immutable source observation identity;
- Context Ref creation;
- Context Ref resolution;
- Context Ref freshness;
- Git historical reconstruction;
- historical candidate generation;
- Diagnostic Context;
- Diagnostic Assertion;
- diagnostic queries;
- Diagnostic Packet semantics;
- SQLite lifecycle;
- canonical CLI behavior;
- canonical protocol behavior.

JavaScript/Node.js must not become a second repository-truth authority.

### 5.2 SQLite authority

**SQLite is the canonical persisted authority.**

Authoritative product state must not be split across:

- JavaScript JSON stores;
- ad-hoc mutable cache files;
- vector databases;
- cloud databases;
- mandatory external services.

Portable JSON may exist only for:

- export;
- test fixtures;
- reports;
- immutable derived caches.

Portable formats are not primary persisted authority.

### 5.3 LLM role

LLMs are optional consumers and reasoning clients.

LLMs may:

- explain evidence;
- propose hypotheses;
- draft findings;
- suggest remediation;
- summarize historical candidates;
- consume Diagnostic Packets.

LLMs may not:

- create parser facts;
- determine canonical graph identity;
- determine Context Ref freshness;
- rewrite deterministic history;
- become mandatory for scanning;
- become mandatory for persistence;
- become repository-truth authority;
- silently promote hypotheses into deterministic evidence.

---

## 6. Final architectural invariant

Preserve this invariant:

> **Rust computes repository truth. SQLite persists authoritative state. Engineering understanding is versioned and attributed. AI reasoning is optional and is never silently promoted to repository truth.**

Equivalent shorthand:

```text
Rust       = computation authority
SQLite     = persisted authority
TS / TSX   = current analyzed language family
JavaScript = optional transport/presentation only
LLM        = optional reasoning consumer
```

---

## 7. Temporal identity model

Do not collapse repository structure, repository observation, and engineering understanding into one version.

### 7.1 Graph identity

A graph ID represents deterministic structural repository evidence.

Graph identity must not change merely because:

- Git HEAD changed;
- README changed;
- comments changed;
- formatting changed;
- irrelevant non-analyzed files changed;
- raw byte hash changed without structural meaning.

Graph identity may change when deterministic structural evidence changes.

### 7.2 Graph version

`graphVersion` is the persisted version of structural repository evidence.

Equivalent structural evidence should reuse the same graph identity/version where contractually valid.

### 7.3 Immutable graph observation

Each analyzed source state is represented by an immutable observation.

An observation may retain:

- project identity;
- graph identity;
- graph version;
- exact Git revision;
- dirty state;
- exact source fingerprint;
- analyzed source manifest;
- observation identity.

Two repository observations may point to the same graph identity.

Example:

```text
Commit A:
graph G12
observation O40

README-only commit:
graph G12
observation O41
```

This distinction is required.

Exact source evidence belongs to the immutable observation that recorded it.
`graph_versions` and their `source_files` rows are structural graph
materialization and may be reused or rebuilt for an equivalent graph; they are
not historical exact-source authority. Cross-observation source comparison
must use the observation-owned source manifest.

Repository identity is explicit and separate from checkout identity. A tracked
root `.flopeek-repository.json` manifest uses
`flopeek-repository-identity/v1` and contains only a validated `repo_<uuid>`
identifier. The scanner reads this manifest but never creates or mutates it.
The manifest is bounded, strict JSON, repository-relative, and is the only
authority for portable repository identity. Git remote URLs are not identity
authority.

When the manifest is present, `project_id` is derived from the repository
identity and is stable across checkouts. The canonical checkout path remains a
local-only checkout identity used only for compatibility aliases and must not
be persisted in portable evidence. Without a valid manifest, Flopeek remains
usable in explicit `checkout-local` mode and reports cross-checkout context as
unavailable. Invalid identity metadata is an error; it must never silently
fall back to a different portable identity.

Legacy checkout-local project IDs and refs remain addressable only through a
same-database alias. They are not portable repository refs and are not silently
rewritten. Identity transitions start a new observation chain with an explicit
limitation rather than inventing continuity.

Last-known-good follows normative `flopeek-lkg-protocol/v1`. It is three separate
domains scoped to one Diagnostic Context:

```text
LastKnownGoodCandidate  = immutable proposition and integrity evidence
LastKnownGoodEvent      = append-only PROPOSE/CONFIRM/REJECT/REVOKE event
LastKnownGoodState      = pure deterministic reduction and projection
```

Candidates bind repository/project, exact Context revision and expected behavior
fingerprint, full Git SHA, observation-owned Graph Basis, evidence contract,
proposer, evidence, reason, timestamp, and integrity. Integrity is `complete`,
`partial`, or `invalid`; only `complete` candidates can be confirmed. A bounded
detached historical observation may be `partial` without changing current
project state or observation continuity.

The reducer accepts only `PROPOSE`, `CONFIRM`, `REJECT`, and `REVOKE`; `SUPERSEDE`
is forbidden. There is at most one pending and one active candidate. A proposal
does not change active state; rejection preserves active state; revocation clears
it; replacement confirmation targets the pending candidate and records the active
candidate as `replacesCandidateId`. Every confirmation requires a pending
candidate; direct confirmation is forbidden. `tipEventId` and
`lastKnownGoodCandidateId` are transactional projections and never advance the
Diagnostic Context revision.

The LKG Review Packet has two explicit applicability views. Its `applicability`
belongs to the selected review candidate (pending first, otherwise active), while
`state.applicabilityStatus` belongs to the effective reduced state (active first,
otherwise pending). State applicability is always reduced from the complete
candidate and event stream; a review packet must not reduce a singleton candidate
slice. Protocol-1.0 diagnosis uses Candidate, Event, and State as its authority
and does not synthesize a legacy binding that attributes a proposer as a
confirmer. The compatibility `lastKnownGoodBinding` field is therefore null for
protocol candidates; actual confirmer attribution remains on the `CONFIRM` event.

Graph reuse and exact observation are separate authorities. Detached historical
materialization may reuse a graph only after validating structural graph metadata,
canonical resolution/entry/related-test evidence, nodes, edges, and flows. It must
not compare exact source bytes, raw source hashes, source positions, or serialized
facts. Exact historical source belongs to the immutable observation manifest
(`graph_observations.source_manifest_json`); detached reuse never rewrites the
canonical graph materialization or current observation state.

Exact revision authority is `graph_observations.git_revision`. The
`graph_versions.source_revision` column is structural materialization metadata
only and must not be used to reject a candidate when its observation basis is
consistent. Applicability is re-evaluated on read/use: repository and Context
revision must match, the candidate revision must be on current HEAD's explicit
first-parent lineage, and observation/graph/evidence contracts must agree.
Statuses include `applicable`, `out-of-lineage`, `repository-mismatch`,
`context-revision-mismatch`, `basis-unavailable`, `contract-incompatible`, and
`unavailable`.

The raw JSONL boundary is untrusted: it can propose but cannot confirm, reject,
or revoke merely by sending `actorKind = human`. Those transitions are exposed
only through the trusted local `flopeek lkg` CLI, whose actor identity is
caller-attributed, not authenticated. Retry with the same idempotency key and
payload is idempotent; a different payload or stale expected tip fails closed.
Legacy v2 bindings remain raw read-only evidence and ambiguous semantics are
quarantined during migration; they are never silently promoted into an active
protocol state. Historical diagnosis uses only complete, applicable active
candidates and never calls a candidate a cause or root cause.

### 7.4 Diagnostic revision

Engineering understanding has its own lifecycle.

Adding or changing:

- observation;
- hypothesis;
- finding;
- remediation;
- verification

must not implicitly create a new graph version.

Invariant:

```text
graph identity
    !=
source observation
    !=
diagnostic revision
```

---

## 8. Context Ref contract

Context Refs are a first-class Flopeek capability.

Preferred URI family:

```text
fp://...
```

A Context Ref must preserve stable repository-context identity and provenance.

Context Ref resolution should expose, where available:

- project identity;
- graph identity;
- graph version;
- node identity;
- origin observation;
- current observation;
- origin source basis;
- current source basis;
- fingerprint scope;
- freshness status;
- freshness reason.

### 8.1 Freshness semantics

Freshness must be deterministic.

Current supported model:

```text
canonical AST/evidence fingerprint
+
sorted direct-edge signatures
```

Formatting and comments should not create false stale states when structural evidence is unchanged.

Current statuses may include:

```text
current
stale
unresolved
unavailable
wrong-project
```

`superseded` is reserved for a proven successor established by deterministic
lineage or explicit attributed verification. A unique compatible fingerprint
is a reconciliation candidate and remains `stale`; it is not proof of rename,
continuity, semantic equivalence, or business intent.

Do not silently reinterpret stale evidence as current.

### 8.2 Provenance invariant

A durable Context Ref keeps its canonical origin provenance.

If the same Context Ref remains valid at a later observation:

```text
origin observation  = original observation
current observation = latest observation
status              = current
```

Do not rewrite origin provenance merely because the repository was rescanned.

---

## 9. Repository evidence model

Never collapse deterministic repository evidence and interpreted engineering knowledge.

### 9.1 Deterministic evidence

Produced from source, graph state, or Git history.

Examples:

- file exists;
- declaration exists;
- supported import exists;
- supported direct call exists;
- edge exists;
- graph changed;
- node fingerprint changed;
- Context Ref is current;
- Context Ref is stale;
- historical candidate touched focused context.

Deterministic evidence should carry sufficient provenance to identify its basis.

### 9.2 Observation

Something reported or measured.

Examples:

- regression reproduced;
- timeout observed;
- CI failed;
- duplicate charge observed.

An observation is not automatically a cause.

### 9.3 Hypothesis

A proposed explanation.

May come from:

- human;
- agent;
- optional LLM.

A hypothesis is not deterministic truth.

### 9.4 Finding

An attributed engineering conclusion supported by evidence.

### 9.5 Remediation

A proposed or implemented correction.

Remediation is not proof of success.

### 9.6 Verification

Records whether a finding or remediation was:

- confirmed;
- rejected;
- superseded;
- implemented;
- regression-tested;
- verified.

---

## 10. Engineering memory

Engineering memory is a core product capability.

Flopeek should preserve why work exists and what has been learned about it.

Core objects include:

- Diagnostic Context;
- Diagnostic Assertion;
- Historical Candidate;
- Diagnostic Packet.

Engineering memory must remain:

- attributed;
- versioned;
- repository-basis-aware;
- freshness-aware where relevant;
- separate from deterministic parser truth.

Do not turn engineering memory into a generic ticket/project-management system.

---

## 11. Diagnostic Context

`DiagnosticContext` records why investigation exists.

Minimum semantics:

- id;
- project identity;
- diagnostic revision;
- intent;
- symptom;
- expected behavior;
- focus Context Refs;
- current graph basis;
- optional last-known-good basis;
- constraints;
- acceptance criteria;
- unresolved questions;
- actor;
- createdAt;
- status;
- revision lineage.

V1 intents remain narrow:

```text
diagnose
audit
verify-fix
```

A Diagnostic Context must bind to an actual current repository observation when created.

Do not accept invented or mismatched repository basis.

---

## 12. Diagnostic Assertion

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

Any technical assertion claiming evidence must reference supporting evidence or explicitly report that evidence is unavailable.

AI/model output must remain attributed and unverified unless separately verified.

---

## 13. Historical diagnosis

Historical diagnosis is a primary Flopeek capability.

Goal:

> identify deterministic historical context changes relevant to a focused repository context without claiming runtime causality.

Input may include:

- current Context Ref;
- current repository observation;
- optional last-known-good revision;
- bounded Git range.

Output may include:

- candidate commits;
- changed paths;
- changed structural evidence;
- candidate relevance reasons;
- retained/changed/removed state;
- explicit limits;
- explicit omissions;
- truncation;
- limitations.

### Historical Candidate

A `HistoricalCandidate` may report that a change:

- occurred after last-known-good;
- touched focused context;
- changed focused AST evidence;
- changed a focused edge;
- changed dependency-cone context;
- changed related tests;
- remains present now;
- was later changed or removed.

It must not claim:

```text
root cause
this commit caused the bug
runtime causality
business intent
```

Preferred terms:

```text
candidate-change
historically-relevant-change
requires-diagnosis
```

### First-parent semantics

Historical diagnosis should use deterministic first-parent semantics for normal lineage analysis.

Merge commits must be compared against their explicit first parent.

An adjacent historical-continuity comparison must verify that its explicit
`fromRevision` is exactly the first parent of `toRevision`. A non-adjacent pair
must be `unavailable` with an explicit reason; it must never be labeled adjacent.

Rename/copy records must preserve relevant old/new paths.

---

## 14. Bounded history

Never scan unbounded history by default.

Define explicit limits for:

- commits;
- candidate count;
- paths;
- snapshot bytes;
- Context Refs;
- assertions;
- packet size.

When a limit is reached:

```text
truncated = true
```

and omissions must be explicit.

A bounded result must never be presented as complete.

Historical snapshot cache identity must include enough derivation metadata to prevent an older/weaker cache from satisfying a newer/stronger request.

At minimum cache identity should include:

- Git revision;
- derivation identity;
- exact parser identity;
- relevant bounds.

---

## 15. Diagnostic Packet

A Diagnostic Packet is the bounded handoff unit for humans and agents.

It may include:

- intent;
- symptom;
- expected behavior;
- current repository basis;
- last-known-good;
- focus Context Refs;
- freshness;
- compact graph evidence;
- historical candidates;
- observations;
- hypotheses;
- findings;
- remediation;
- verification state;
- acceptance criteria;
- unresolved questions;
- limits;
- omissions;
- truncation;
- explicit limitations.

The packet must preserve evidence classes.

Do not include private model chain-of-thought.

The goal is reusable context, not a full repository dump.

---

## 16. Language intelligence policy

Language support exists to improve repository-context quality.

Adding a language is not a product goal by itself.

Each language implementation must feed the same downstream product concepts:

```text
Source
  ↓
Deterministic Facts
  ↓
Structural Identity
  ↓
Context Ref
  ↓
Freshness
  ↓
History
  ↓
Engineering Memory
```

A new language must not require:

- parallel graph authority;
- parallel storage authority;
- parallel diagnostic model;
- language-specific repository-truth semantics incompatible with the core.

### Current V1 language

V1 analyzes:

```text
TypeScript
TSX
```

TypeScript/TSX is the analyzed source language family.

Rust remains the implementation language.

### TypeScript maturity before language expansion

Before another language becomes active product scope, TypeScript/TSX should have sufficiently reliable:

- declaration identity;
- import binding;
- module resolution;
- symbol resolution;
- direct-call resolution;
- class/method relationships;
- deterministic ambiguity handling;
- framework-neutral entry/flow semantics;
- Context Ref freshness;
- historical reconstruction;
- real-repository fixtures;
- acceptable false-positive/false-negative behavior.

Language expansion belongs after TypeScript maturity, not before it.

---

## 17. TypeScript evidence direction

The TypeScript adapter should be strengthened only when doing so improves Flopeek context quality.

Priority evidence areas:

1. declaration identity;
2. import alias binding;
3. default imports;
4. namespace imports;
5. re-export chains;
6. barrel resolution;
7. `tsconfig.json` paths/baseUrl support;
8. caller-symbol → callee-symbol relationships;
9. class methods;
10. constructors;
11. inheritance/interface relationships;
12. deterministic ambiguity reporting;
13. framework-neutral entry points;
14. test relationships where statically defensible.

Do not add speculative runtime relationships simply to increase graph density.

---

## 18. Review-feature policy

Flopeek may expose derived review-oriented features when they are useful.

Examples:

- impact radius;
- change summaries;
- affected tests;
- review context;
- risk hints;
- graph traversal;
- token-efficient context selection.

These are **derived capabilities**, not primary product identity.

They must not displace the core roadmap unless they strengthen at least one of:

- versioned context;
- provenance;
- Context Ref quality;
- freshness;
- historical diagnosis;
- engineering memory;
- bounded human/agent handoff.

Do not optimize Flopeek primarily to become a clone of a code-review graph product.

---

## 19. Explicit non-goals

Do not add unless this file is explicitly changed:

- mandatory LLM integration;
- built-in foundation models;
- generic RAG platform;
- generic vector-search product;
- generic enterprise knowledge graph;
- ontology engine;
- generic workflow engine;
- generic planning engine;
- generic project-management expansion;
- automatic root-cause truth;
- runtime causality from static edges;
- arbitrary dynamic-dispatch guessing;
- reflection guessing;
- arbitrary business-intent inference;
- arbitrary shell execution through diagnostic MCP;
- automatic target-source mutation through repository-truth APIs;
- multi-repository diagnosis in V1;
- cross-project automatic graph edges in V1;
- multi-language parity before TypeScript maturity;
- language-count competition as roadmap policy.

---

## 20. Local-first and privacy boundary

Flopeek should remain local-first by default.

Core repository evidence must not require:

- network access;
- telemetry;
- cloud APIs;
- model-provider credentials.

Do not send repository source to external services without explicit user action.

Authoritative persisted records must not contain:

- credentials;
- secrets;
- private chain-of-thought;
- unnecessary raw source bodies;
- absolute machine paths in portable records.

---

## 21. SQLite persistence policy

SQLite migrations must be:

- monotonic;
- transactional;
- recoverable;
- explicit by schema version;
- safe against partially applied upgrades.

`PRAGMA user_version` must only advance after a migration succeeds.

A database newer than the supported schema must be rejected explicitly.

Authoritative domains may include:

```text
graph versions
graph observations
source facts
nodes
edges
Context Refs
project state
Diagnostic Contexts
Diagnostic Assertions
Historical Candidates
```

Derived evidence may be rebuilt if its schema changes.

Authoritative engineering memory must not be silently discarded during migration.

---

## 22. Machine-readable product contract

Maintain a small machine-readable product contract under:

```text
contracts/product.json
```

It should enforce high-risk invariants.

Required concepts include:

```json
{
  "canonicalRepository": "flopeek-context/flopeek",
  "coreImplementation": "rust",
  "persistedAuthority": "sqlite",
  "primaryAnalyzedLanguages": ["typescript", "tsx"],
  "llmRequired": false,
  "javascriptRepositoryAuthority": false,
  "automaticRootCauseClaims": false,
  "graphIdentityBasis": "typescript-context-structural-evidence",
  "sourceBasis": "immutable-graph-observation",
  "contextFreshness": "node-ast-and-direct-edges",
  "productIdentity": "versioned-repository-context",
  "graphRole": "deterministic-substrate",
  "languageCountIsProductGoal": false,
  "reviewGraphIsPrimaryProduct": false,
  "contextReconciliation": "exact-compatible-fingerprint-candidates",
  "automaticSupersession": "disabled-without-lineage-proof",
  "lastKnownGoodModel": "immutable-candidate-append-only-event-reduced-state",
  "lastKnownGoodLifecycle": "protocol-1.0-deterministic-reducer",
  "lastKnownGoodProvenance": "revision-observation-graph-consistent",
  "lastKnownGoodIntegrity": "observation-owned-revision-and-graph-contract",
  "lastKnownGoodApplicability": "current-first-parent-and-context-revision",
  "lastKnownGoodTrust": "local-transition-boundary-caller-attributed",
  "humanActorIdentity": "caller-attributed-not-authenticated"
}
```

`AGENTS.md` remains the human-readable authority.

The machine contract exists to prevent accidental architectural regression.

---

## 23. Historical migration record

The repository was destructively migrated from the historical JavaScript/multi-language Flopeek Core into a Rust/SQLite/TypeScript foundation.

The removed implementation remains available in Git history.

Do not restore:

- JavaScript repository-truth authority;
- dual-core selection;
- automatic JavaScript fallback;
- legacy native-promotion machinery;
- root Node package authority;
- old broad multi-language CI;
- old roadmap authority;
- removed legacy implementation merely for historical convenience.

Historical import commit:

`72a95fe1a6497683e96e90872438cd3c83b7272f`

The old repository is provenance, not active upstream.

---

## 24. Active priorities

### P0 — Foundation correctness

Status:

**implemented; maintenance-only**

Includes:

- Rust single authority;
- SQLite authority;
- TypeScript/TSX-only analyzed scope;
- graph identity;
- immutable graph observations;
- Context Ref provenance;
- node-level freshness;
- transactional migrations;
- bounded history;
- minimal Rust CI.

Do not restart foundation migration unless a concrete correctness defect requires it.

### P1 — TypeScript Context Intelligence

Status:

**implemented; maturity gate passed**

Strengthen evidence required for better repository context:

- symbol identity;
- import/module resolution;
- call precision;
- class/method semantics;
- deterministic ambiguity handling;
- framework-neutral flows;
- related-test evidence where defensible.

Goal:

> improve Context Ref quality, historical diagnosis, and reusable context.

### P2 — Temporal Context Intelligence

Status:

**implemented; LKG Protocol 1.0 conformant; correctness frozen**

Strengthen:

- observation continuity;
- Context Ref reconciliation;
- stale/superseded semantics;
- structural change attribution;
- historical continuity;
- candidate relevance precision;
- last-known-good workflows.

### P3 — Engineering Memory

Status:

**paused pending final LKG Protocol 1.0 conformance gate**

Strengthen:

- Diagnostic Context lifecycle;
- assertion lifecycle;
- findings;
- remediation;
- verification;
- revision lineage;
- actor attribution;
- cross-agent handoff.

### P4 — Diagnostic Context Delivery

Improve:

- bounded Diagnostic Packets;
- CLI ergonomics;
- protocol ergonomics;
- MCP integration;
- agent consumption;
- human review surfaces.

Do not allow delivery surfaces to create repository truth independently.

### P5 — Language Expansion

Only after TypeScript maturity gate.

Add one language at a time.

A new language must reuse:

- the same graph authority;
- the same observation model;
- the same Context Ref semantics;
- the same freshness model;
- the same historical-diagnosis domain;
- the same engineering-memory domain.

---

## 25. CI policy

Normal PR/main CI should remain intentionally small.

Required default gate:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --all-targets --locked
```

Do not add CI matrices merely because a capability could theoretically be tested on more environments.

Security/dependency checks may run separately on a schedule.

Cross-platform packaging belongs to release/tag/manual workflows.

Normal CI must not restore:

- Node matrix testing;
- npm clean-room;
- Go setup;
- .NET setup;
- multi-language adapter matrices;
- native-vs-JavaScript parity;
- old native-promotion/dogfood workflows.

---

## 26. Test policy

Every authoritative capability must have deterministic tests.

Important categories:

- parser facts;
- TypeScript/TSX fixtures;
- graph identity;
- graph-version reuse;
- immutable observations;
- Context Ref origin provenance;
- Context Ref current/stale/unresolved semantics;
- node fingerprint behavior;
- comment/formatting stability;
- SQLite migrations;
- migration rollback;
- graph/context atomicity;
- Diagnostic Context basis validation;
- assertion lifecycle;
- bounded history;
- merge first-parent behavior;
- rename/copy history;
- truncation;
- deterministic ordering;
- historical candidate false-positive controls;
- no root-cause claims;
- no source-body persistence where prohibited;
- no credential persistence;
- exact parser identity in cache namespace.

Do not reproduce irrelevant legacy test breadth.

Test active product contracts.

---

## 27. TypeScript historical fixture

Maintain a deterministic Git-backed TypeScript/TSX history fixture.

Minimum story:

```text
A: checkout/payment last-known-good
B: retry path introduced
C: unrelated change
D: timeout branch changed
E: current/bad state
```

The fixture should prove:

- retry-related change is historically relevant;
- timeout-related change is historically relevant;
- unrelated change ranks low or is excluded;
- neither candidate is called root cause;
- stale Context Refs are surfaced;
- last-known-good remains explicit;
- candidate evidence is reproducible;
- merge commits are handled against first parent;
- assertion lifecycle remains separate from deterministic evidence.

---

## 28. Code structure and dependency policy

The core must remain modular as capability grows. A module has one primary
responsibility and a named domain boundary. Do not add new behavior to a god
file merely because an existing file is convenient.

Dependency direction is one-way:

```text
protocol / CLI
      -> orchestration
      -> domain evidence and temporal logic
      -> persistence adapters
```

The following boundaries are mandatory:

- Temporal, graph, and diagnostic domain logic must not depend on JSONL, SQLite,
  filesystem, or Git adapters.
- SQL belongs in the storage subsystem. Git command execution belongs in the
  historical Git adapter.
- `mod.rs` files are facades and re-export surfaces; they do not contain large
  implementations.
- Do not create catch-all `utils`, `helpers`, or `common` modules without a
  specific domain responsibility.
- A module should target at most 500 production lines. More than 700 production
  lines is a blocker for review unless the file is declarative/generated and
  the PR documents the exception.
- Facades should target at most 200 production lines.
- Functions longer than 100 lines must be decomposed or document the atomicity
  or invariant that requires one function body.
- Large tests must be split by behavior into sibling test modules or integration
  tests; test volume must not conceal a production god file.
- Every new cross-module dependency must state its owner, direction, and error
  boundary in the PR description.

These are review gates, not a line-count CI check. Existing oversized modules
must be reduced when their subsystem is next changed. A refactor must preserve
public paths through explicit facades and re-exports.

Review every architectural change for responsibility, dependency direction,
state ownership, error propagation, bounds, persistence authority, and tests.

---

## 29. Branch and change policy

`main` is canonical and protected.

Use short-lived branches.

Recommended patterns:

```text
feature/typescript-symbol-resolution
feature/context-reconciliation
feature/engineering-memory
feature/diagnostic-packet
fix/<specific-correctness-problem>
test/<specific-evidence-gap>
```

Avoid mixing unrelated high-risk changes.

Especially avoid:

- parser semantic change + SQLite migration;
- storage migration + UI work;
- language expansion + core identity change;
- framework adapter + historical engine rewrite;
- product-direction change hidden inside refactor.

---

## 30. Definition of done

A core capability is done only when:

- Rust owns authoritative behavior;
- SQLite owns authoritative persisted state where applicable;
- behavior is deterministic where claimed;
- repository basis is explicit;
- provenance is explicit;
- freshness is explicit where relevant;
- limits are explicit;
- unsupported behavior is explicit;
- human/agent assertions are attributed;
- static history makes no root-cause claim;
- migrations are safe;
- tests pass;
- no mandatory LLM exists;
- no silent JavaScript authority exists;
- product scope is not silently broadened.

A language-intelligence improvement is done only when it demonstrably improves at least one downstream Flopeek capability:

- evidence quality;
- Context Ref identity;
- freshness;
- historical diagnosis;
- Diagnostic Packet quality;
- engineering-memory binding.

---

## 31. Agent safety rules

Agents must not:

- execute target application code merely to improve static evidence;
- send repository source to external services by default;
- add telemetry by default;
- add mandatory LLM credentials;
- expose arbitrary shell through diagnostic interfaces;
- invent historical evidence;
- invent last-known-good;
- infer hidden human intent when structured intent is absent;
- treat missing evidence as proof of absence;
- claim runtime behavior from static edges;
- call a historical candidate a cause;
- silently broaden beyond active language scope;
- restore JavaScript repository-truth authority;
- restore automatic JavaScript fallback;
- turn review features into product identity without explicit direction;
- add languages merely to increase language count;
- create parallel product/architecture/roadmap authority documents.

When evidence is unavailable, say:

```text
unavailable
```

When context is stale, say:

```text
stale
```

When historical relevance is not causality, say:

```text
candidate
```

not:

```text
cause
root cause
```

---

## 32. Agent response discipline

When implementing or auditing Flopeek, classify conclusions as:

- **Deterministic evidence**
- **Observation**
- **Hypothesis**
- **Finding**
- **Remediation**
- **Verification**
- **Unknown / unavailable**

For historical diagnosis always report:

- current basis;
- origin basis where relevant;
- last-known-good if supplied;
- historical range inspected;
- limits;
- truncation;
- candidate changes;
- relevance reasons;
- unresolved runtime/dynamic behavior.

Never collapse evidence classes into one narrative truth.

---

## 33. Final invariants

### Product identity

> **The graph is deterministic substrate. Versioned repository context and engineering memory are the product.**

### Repository truth

> **Repository truth is computed deterministically. Engineering understanding is versioned and attributed. AI reasoning is optional and never silently promoted to repository truth.**

### Implementation authority

> **Rust is the single implementation authority for repository truth.**

### Persistence authority

> **SQLite is the canonical persisted authority for Rust-owned repository and engineering-memory state.**

### Temporal identity

> **Graph identity, repository observation, and diagnostic revision are distinct lifecycles.**

### Context freshness

> **A durable Context Ref preserves origin provenance and is deterministically evaluated against the current repository observation.**

### Historical boundary

> **Historical analysis identifies context-change candidates, not automatic runtime root cause.**

### Language boundary

> **Language intelligence is an input capability, not the product identity.**

### Product North Star

> **Flopeek should reduce the need for humans and coding agents to repeatedly reconstruct repository understanding while preserving exactly what state that understanding refers to and whether it is still valid.**

That is the product boundary.

Keep the product narrow.

Strengthen context before breadth.

Strengthen provenance before convenience.

Strengthen TypeScript before adding languages.

Do not turn Flopeek into a generic review graph.
