# AGENTS.md

## 0. Authority

This file is the **single human-readable source of truth** for the Flopeek product line in this repository.

Canonical repository:

`github.com/flopeek-context/flopeek`

This file defines:

- product direction;
- architecture authority;
- active product scope;
- non-goals;
- evidence semantics;
- implementation language authority;
- persistence authority;
- migration policy;
- active priorities;
- definition of done;
- rules for humans and coding agents.

If another document conflicts with this file on any of those topics, **this file wins** until an explicit product-direction change updates it.

Do not create parallel product/architecture/roadmap authority documents.

The following files MUST NOT become independent sources of truth:

- `PRODUCT.md`
- `ARCHITECTURE.md`
- `ROADMAP.md`
- `DESIGN.md`
- similarly named replacement authority documents

If inherited copies exist from the historical baseline, treat them as legacy reference material only and remove, archive, or clearly mark them non-authoritative when appropriate.

Agents MUST NOT introduce a new product, architecture, or roadmap source-of-truth document without an explicit request to change this documentation-authority model.

---

## 1. Product mission

Flopeek exists to provide:

> **Versioned repository engineering context, diagnostic memory, and historical diagnosis for humans and coding agents.**

Flopeek is not intended to out-reason AI models.

Flopeek provides the deterministic, versioned, attributable repository context that humans and AI systems can reason over.

The central product boundary is:

```text
Repository source
      |
      v
Deterministic Flopeek engine
      |
      +--> repository evidence
      +--> graph identity
      +--> context identity
      +--> version history
      +--> freshness
      +--> historical deltas
      |
      v
Versioned engineering context
      |
      +--> diagnostic intent
      +--> observations
      +--> hypotheses
      +--> findings
      +--> remediation
      +--> verification
      |
      v
Human / coding agent / optional LLM
```

The repository evidence layer answers:

> What is demonstrably present in source?

The diagnostic layer answers:

> Why are we inspecting this context, what changed, what has been observed, what do we currently believe, what has been verified, and whether that knowledge still applies to the current repository state?

---

## 2. Historical baseline and repository independence

This repository originated from the following historical implementation baseline:

- Historical repository: `badsleepyday/flopeek-core`
- Historical branch: `development`
- Exact imported baseline commit: `72a95fe1a6497683e96e90872438cd3c83b7272f`
- Baseline date: `2026-08-09`
- Baseline role: **historical implementation provenance and starting point only**

The canonical active repository is:

`flopeek-context/flopeek`

The historical repository is NOT:

- an active upstream;
- a roadmap authority;
- a release authority;
- a publication authority;
- a product authority;
- an automatic synchronization source.

Current `main` is expected to advance independently from the historical baseline.

Current `HEAD` does not need to equal the imported baseline SHA.

### 2.1 No automatic upstream synchronization

Agents MUST NOT:

- continuously merge from `badsleepyday/flopeek-core`;
- automatically rebase against it;
- mirror its branches;
- restore its roadmap;
- import features merely because they exist upstream;
- silently perform a broad "sync upstream".

A specific historical change may be adopted only when explicitly requested or clearly justified for:

- correctness;
- security;
- compatibility;
- required infrastructure.

Any such adoption must record:

- source repository;
- exact commit SHA;
- reason;
- imported behavior/files;
- compatibility effect;
- product-scope effect;
- validation/tests.

---

## 3. Documentation model

`AGENTS.md` is the single human-readable product and engineering constitution.

Other documentation has only these roles.

### 3.1 README and user documentation

README/user documentation may explain:

- what Flopeek currently does;
- installation;
- usage;
- examples;
- current commands;
- current support.

It MUST NOT define independent product direction or architecture policy.

### 3.2 ADRs

Architecture Decision Records may explain why a decision was made.

Use ADRs for historical reasoning such as:

- why Rust became the core authority;
- why JavaScript fallback was retired from new product capabilities;
- why diagnostic assertions are separate from deterministic evidence.

ADRs are historical records.

They do NOT override current rules in this file.

Do not rewrite old ADRs to hide history.

If a decision changes, create a new ADR and update this file.

### 3.3 Generated reference documentation

Generated documents such as support matrices may describe implementation state.

They must be derived from:

- code;
- schemas;
- capability registries;
- machine-readable contracts.

Generated/reference documentation does not define product direction.

### 3.4 Machine-readable contract

Critical invariants SHOULD be duplicated in a machine-readable contract and enforced by tests.

Recommended location:

```text
contracts/product-contract.json
```

Recommended concepts:

```json
{
  "canonicalRepository": "flopeek-context/flopeek",
  "coreImplementation": "rust",
  "persistedAuthority": "sqlite",
  "primaryDiagnosticLanguage": "typescript",
  "llmRequired": false,
  "javascriptRepositoryAuthority": false,
  "automaticRootCauseClaims": false,
  "historicalOutputClass": "candidate-not-cause"
}
```

`AGENTS.md` is the human-readable authority.

The machine contract exists to enforce high-risk invariants, not to create a second product specification.

---

## 4. Flopeek identity

This repository remains a Flopeek product.

Repository independence does NOT mean renaming established Flopeek identity.

Preserve by default:

- product name `Flopeek`;
- CLI name `flopeek`;
- `.flopeek/` local metadata directory;
- Context Ref scheme such as `fp://...`;
- valid existing `flopeek-*` schema identifiers;
- valid established Flopeek protocol identifiers.

What changes is authority and ownership:

```text
Historical provenance:
badsleepyday/flopeek-core

Canonical authority:
flopeek-context/flopeek
```

Before public release, update repository-specific identity where necessary:

- repository links;
- issue links;
- support links;
- badges;
- package provenance;
- release destinations;
- GitHub organization ownership;
- publication credentials;
- release approvals;
- generated repository references.

Do not reuse old publication/release authority automatically.

---

## 5. Core implementation authority

### 5.1 Rust is the single core authority

Rust is the authoritative implementation language for Flopeek repository truth and new product capabilities.

TypeScript/TSX is the first **target language being analyzed**.

It is NOT the implementation language of the core.

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
- Context Ref resolution;
- Context Ref freshness;
- Git historical reconstruction;
- historical graph comparison;
- historical candidate generation;
- diagnostic persistence;
- diagnostic queries;
- diagnostic packet construction where practical;
- SQLite lifecycle.

Architectural invariant:

> **Rust is the single implementation authority for repository truth. JavaScript may transport or present that truth, but MUST NOT independently recreate repository truth for new product capabilities.**

### 5.2 SQLite is persisted authority

SQLite is the authoritative persisted state for Rust-owned repository and diagnostic state unless this file is explicitly changed.

New core diagnostic features should be designed Rust-first and SQLite-first.

Do not create a new JavaScript/JSON authority layer merely for implementation convenience if it will later require migration into Rust.

Logical domains should remain separated even when stored in one database.

Conceptually:

```text
SQLite
 |
 +-- evidence domain
 |    +-- graph versions
 |    +-- nodes
 |    +-- edges
 |    +-- flows
 |    +-- historical state
 |
 +-- context domain
 |    +-- context identity
 |    +-- freshness
 |
 +-- diagnostic domain
      +-- diagnostic contexts
      +-- assertions
      +-- assertion evidence refs
      +-- revisions
      +-- historical candidates
```

Storage implementation details belong in code/schema migrations, not in this document.

---

## 6. JavaScript / Node.js policy

JavaScript/Node.js is inherited infrastructure and may temporarily remain for:

- CLI hosting;
- MCP integration;
- HTTP/server glue;
- Viewer integration;
- packaging;
- compatibility tests;
- migration tooling;
- comparison against historical behavior.

JavaScript MUST NOT become a second repository-truth authority.

For new product capabilities, JavaScript MUST NOT independently:

- scan source as authoritative truth;
- construct an authoritative graph;
- determine graph identity;
- determine Context Ref freshness;
- compute authoritative historical candidates;
- persist independent diagnostic truth.

Preferred boundary:

```text
CLI / MCP / Viewer / Node host
             |
             v
        Rust protocol
             |
             v
         Rust core
             |
             v
           SQLite
```

### 6.1 Compatibility oracle

The inherited JavaScript implementation may remain temporarily as a **compatibility/parity oracle**.

It may be used to validate Rust behavior during migration.

Example:

```text
historical JS result
        |
        | parity comparison
        v
Rust authoritative implementation
```

This does NOT make JavaScript production authority.

### 6.2 No silent JS fallback for new capabilities

New product capabilities MUST fail explicitly when Rust authority is unavailable.

Example:

```json
{
  "status": "unavailable",
  "reason": "rust-core-unavailable"
}
```

Do NOT silently fall back to JavaScript and create a second interpretation of repository truth.

Legacy inherited surfaces may retain explicit compatibility fallback temporarily only when required for migration, and such fallback must be:

- visible;
- attributable;
- tested;
- non-authoritative for new diagnostic capabilities.

The long-term direction is to remove JavaScript from repository-truth authority paths.

---

## 7. V1 target language

V1 diagnostic support is intentionally limited to:

- TypeScript
- TSX

JavaScript, Python, Rust, Go, Java, PHP, Svelte, C#, and other inherited adapters may remain in the repository for compatibility and historical baseline testing.

They are NOT part of the V1 diagnostic product contract.

Do not expand V1 diagnostic support to another language without an explicit update to this file.

### 7.1 TypeScript/TSX evidence

V1 may rely on deterministic Rust-owned evidence such as:

- source inventory;
- top-level declarations;
- imports;
- supported direct calls;
- supported route/entry facts;
- graph nodes and edges;
- flow projections;
- related-test evidence;
- source revision;
- graph version;
- graph delta;
- Context Ref current/stale state.

Unsupported dynamic behavior must remain explicitly unsupported.

Do not add heuristic claims merely to make a demo look more complete.

---

## 8. Product authority gate

Existing inherited code is substrate, not automatic roadmap scope.

Every new product change MUST trace directly to at least one of:

1. Diagnostic Context
2. Diagnostic Assertion
3. Historical Diagnosis
4. Diagnostic Packet
5. Rust/SQLite correctness, security, performance, compatibility, or infrastructure strictly required to support 1–4

If a proposed change cannot be traced to one of those categories, do not implement it without an explicit product-direction change.

This means inherited systems such as:

- generic workflow expansion;
- generic planning;
- semantic inference;
- multi-project expansion;
- extra language adapters;
- WebGL renderer work;
- unrelated agent integrations

do not become priorities merely because the code exists.

Use this rule:

> **Inherited capabilities are substrate, not roadmap commitments.**

---

## 9. Explicit V1 non-goals

Do NOT add unless this file is explicitly changed:

- mandatory LLM integration;
- built-in foundation model;
- generic RAG platform;
- generic enterprise knowledge graph;
- ontology engine;
- vector database as repository authority;
- multi-language diagnostic parity;
- runtime causality claims from static evidence;
- automatic root-cause truth claims;
- dynamic-dispatch guessing;
- reflection guessing;
- arbitrary business-intent inference;
- autonomous source mutation as a Flopeek core responsibility;
- arbitrary shell execution through diagnostic MCP tools;
- generic project-management expansion;
- generic workflow-engine expansion;
- multi-repository diagnosis;
- cross-project automatic graph edges;
- WebGL/renderer migration;
- new framework adapters unrelated to V1 TypeScript diagnosis.

---

## 10. Evidence model

Never collapse deterministic evidence, observation, hypothesis, finding, remediation, and verification into one truth class.

### 10.1 Deterministic evidence

Produced deterministically from source, graph state, or Git history.

Examples:

- node exists;
- supported import exists;
- supported direct call exists;
- flow contains a step;
- Context Ref is current;
- Context Ref is stale;
- edge was introduced between revisions;
- flow changed between revisions.

Where available, deterministic evidence must carry:

- project identity;
- graph version;
- source/Git basis;
- evidence class;
- source-safe reference;
- parser/derivation identity.

### 10.2 Observation

Something reported or measured.

Examples:

- regression test failed;
- timeout reproduced;
- human observed duplicate charge;
- CI recorded a failure.

Observation is not automatically a cause.

### 10.3 Hypothesis

A proposed explanation.

Example:

> Retry may repeat an external side effect.

Hypothesis is not deterministic truth.

A model-generated hypothesis remains an attributed model/agent assertion.

### 10.4 Finding

An audit/review conclusion supported by evidence.

A finding must carry:

- actor;
- basis;
- evidence references;
- status;
- verification state.

### 10.5 Remediation

A proposed or implemented correction.

Remediation is not proof that the problem is fixed.

### 10.6 Verification

Verification records whether an assertion or remediation has been:

- confirmed;
- rejected;
- superseded;
- implemented;
- regression-tested;
- verified.

---

## 11. Diagnostic Context

`DiagnosticContext` represents why engineering work exists.

Minimum semantic fields:

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
- supersedes/revision lineage.

V1 intent values should remain narrow:

- `diagnose`
- `audit`
- `verify-fix`

Do not turn Diagnostic Context into a generic project-management object.

---

## 12. Diagnostic Assertion

Use one versioned assertion model for interpreted engineering knowledge.

Recommended assertion kinds:

- `observation`
- `hypothesis`
- `finding`
- `remediation`
- `verification`

Recommended statuses:

- `proposed`
- `confirmed`
- `rejected`
- `superseded`
- `implemented`
- `verified`

Every assertion MUST be attributed.

Every assertion that claims technical support MUST reference evidence or explicitly report that evidence is unavailable.

AI/model output MUST NOT be stored as deterministic repository truth.

---

## 13. Version model

Repository state and engineering understanding have separate version lifecycles.

### 13.1 Graph version

`graphVersion` represents repository evidence state.

It changes according to repository/graph lifecycle rules.

### 13.2 Diagnostic revision

Diagnostic revision represents engineering-understanding state.

It may change when:

- a symptom is refined;
- an observation is added;
- a hypothesis is proposed;
- a finding is confirmed;
- remediation is recorded;
- verification is added;
- the diagnostic context is reconciled against a newer graph.

Adding a finding MUST NOT advance `graphVersion`.

### 13.3 Freshness

Persisted diagnostic knowledge that depends on repository state must be able to report:

- `current`
- `stale`
- `superseded`
- `unavailable`
- `unresolved`

Never silently reinterpret stale evidence as current.

---

## 14. Historical diagnosis

Historical diagnosis is a primary differentiating capability.

### 14.1 Goal

Given a current focused context and an optional last-known-good basis, Flopeek should identify historically relevant deterministic changes that may deserve human/agent diagnosis.

### 14.2 Input

V1 input:

- current TypeScript/TSX node or flow Context Ref;
- current graph/source basis;
- optional last-known-good Git revision or retained graph basis;
- bounded historical range.

### 14.3 Output

V1 output:

- deterministic historical candidate changes;
- relevance reasons;
- introduced/changed/removed status;
- still-present status where known;
- explicit omissions;
- truncation state;
- limitations.

Static historical analysis MUST NOT emit a root-cause truth claim.

### 14.4 Historical Candidate

`HistoricalCandidate` is deterministic output.

It may report that a change:

- occurred after last-known-good;
- touched focused context;
- lies in a current dependency/flow cone;
- changed a focused node;
- changed a focused edge;
- changed a focused flow;
- changed related-test structure;
- is still present now;
- was later removed.

It MUST NOT claim:

- "this commit caused the bug";
- "this is the root cause";
- runtime execution;
- business intent.

Use terms such as:

- `candidate-change`
- `historically-relevant-change`
- `requires-diagnosis`

### 14.5 Initial algorithm

Start deterministic and explainable.

For a bounded linear Git range:

1. resolve last-known-good;
2. resolve current revision;
3. enumerate a bounded commit sequence;
4. reuse cached historical snapshots;
5. compare adjacent graph states;
6. intersect graph deltas with focused current context;
7. score/rank candidate changes using declared deterministic rules;
8. return relevance reasons and limitations.

Possible ranking signals:

- edge introduced and still present;
- focused flow changed;
- focused node changed;
- change lies in current dependency cone;
- related-test structure changed;
- change occurred after last-known-good;
- change was later removed.

Any score must be reproducible and explainable.

### 14.6 Context change is not runtime causality

Historical narrowing identifies context change.

It does not prove runtime bug causality.

Use the term:

> **context-change diagnosis**

Do not market static historical analysis as an automatic debugger or root-cause oracle.

---

## 15. Bounded history

Never scan unbounded repository history by default.

V1 must define explicit limits for:

- maximum commits inspected;
- maximum snapshot bytes;
- maximum candidate changes;
- maximum candidate paths;
- maximum Context Refs;
- maximum diagnostic assertions;
- maximum diagnostic packet size.

When a bound is reached:

- stop deterministically;
- return `truncated: true`;
- state what was omitted;
- never pretend the result is complete.

Historical snapshots should be keyed by immutable Git revision.

---

## 16. Diagnostic persistence

New diagnostic persistence belongs in Rust/SQLite authority.

Do not create an independent JavaScript JSON truth store for new diagnostic capabilities.

Expected logical records include:

- diagnostic contexts;
- diagnostic revisions;
- diagnostic assertions;
- assertion-to-evidence references;
- historical candidate metadata where persistence is justified;
- verification records.

Requirements:

- transactional writes;
- schema/version validation;
- bounded records;
- explicit lineage;
- deterministic ordering where observable;
- no credentials;
- no private chain-of-thought;
- no raw source bodies in portable diagnostic records;
- no absolute machine paths in portable records.

---

## 17. Diagnostic Packet

Clients should consume bounded diagnostic context rather than entire repository history.

A Diagnostic Packet should be able to include:

- intent;
- symptom;
- expected behavior;
- current repository basis;
- last-known-good basis;
- focus Context Refs;
- current/stale resolution;
- compact current graph evidence;
- historical candidates;
- observations;
- hypotheses;
- findings;
- remediation state;
- verification state;
- acceptance criteria;
- unresolved questions;
- limitations;
- omissions;
- truncation state.

Evidence classes must remain separate.

The packet must not contain private chain-of-thought.

Rust should own authoritative packet semantics.

A host layer may serialize or transport the packet without changing meaning.

---

## 18. LLM policy

Flopeek MUST work without an LLM.

LLMs are optional consumers/reasoners.

LLMs may:

- explain evidence;
- propose hypotheses;
- draft findings;
- suggest remediation;
- summarize historical candidates;
- help humans interpret a Diagnostic Packet.

LLMs may NOT:

- create parser facts;
- determine graph identity;
- determine Context Ref freshness;
- rewrite deterministic history;
- silently promote hypotheses to deterministic evidence;
- become mandatory for graph construction;
- become mandatory for historical diagnosis;
- become mandatory for diagnostic persistence;
- become repository-truth authority.

Any model-generated assertion must be:

- attributed;
- classified;
- versioned;
- explicitly unverified until verified.

---

## 19. Active priorities

This section is the current product-delivery authority.

### P0A — Canonical repository authority

- establish `flopeek-context/flopeek` as canonical authority;
- preserve historical provenance;
- remove legacy repository authority assumptions;
- prevent accidental publication to historical destinations.

### P0B — Rust single-authority cutover for TypeScript/TSX

- Rust owns TypeScript/TSX repository discovery;
- Rust owns parsing;
- Rust owns graph construction;
- Rust owns graph/query authority;
- Rust owns Context Ref authority;
- remove silent JavaScript authority fallback from new capabilities;
- keep JavaScript only as compatibility/parity oracle where needed.

### P0C — SQLite persisted authority

- define one authoritative SQLite lifecycle;
- ensure graph and diagnostic persistence cannot create competing current authorities;
- keep migrations transactional and recoverable;
- make authority/failure states explicit.

### P1 — Diagnostic Context

- Rust schema/domain;
- SQLite persistence;
- current graph binding;
- last-known-good binding;
- revision lineage;
- attribution;
- tests.

### P2 — Diagnostic Assertion

- observation;
- hypothesis;
- finding;
- remediation;
- verification;
- evidence references;
- superseding lifecycle;
- tests.

### P3 — Historical Context Diagnosis

- bounded Git range;
- cached graph snapshots;
- deterministic adjacent comparison;
- focused-context intersection;
- explainable historical candidate ranking;
- stale/reconcile behavior;
- tests.

### P4 — Diagnostic Packet

- bounded composition;
- evidence separation;
- history candidates;
- assertions;
- omissions;
- limitations;
- host serialization.

### P5 — Human / Agent surfaces

Only after core contracts are stable:

- CLI;
- MCP;
- API/server;
- Viewer timeline;
- finding/remediation UX;
- agent handoff.

Do not start by redesigning UI.

---

## 20. Required TypeScript historical fixture

Create a deterministic Git-backed TypeScript/TSX fixture.

Minimum history:

- Version A: checkout/payment flow is last-known-good.
- Version B: retry path is introduced.
- Version C: unrelated change.
- Version D: timeout branch changes.
- Version E: current/bad state.

Tests must prove:

- unrelated changes are excluded or ranked low;
- retry-related change is historically relevant;
- timeout-related change is historically relevant;
- neither is labeled root cause;
- stale Context Refs are surfaced;
- last-known-good remains explicit;
- candidate evidence is reproducible;
- assertion lifecycle remains separate from deterministic evidence;
- Rust produces the authoritative result;
- no JavaScript repository-truth implementation is required.

Do not execute the target fixture application merely to prove the static historical contract.

Runtime verification, if later added, must remain a separate evidence class.

---

## 21. Test requirements

Every new diagnostic capability must include relevant tests for:

- happy path;
- invalid schema/input;
- stale Context Ref;
- wrong project;
- revision lineage;
- supersedes behavior;
- bounded history;
- truncation;
- deterministic ordering;
- SQLite transaction/recovery behavior;
- corrupted/incompatible store behavior;
- no-source-body persistence;
- no-credential persistence;
- TypeScript historical fixture;
- false-positive historical candidates;
- Rust authority;
- absence of silent JavaScript authority fallback.

When changing parser semantics, add parser fixture/parity tests separately.

Do not weaken existing applicable correctness tests merely to land new product behavior.

---

## 22. Machine-enforced invariants

Where practical, enforce these invariants in code/CI:

```text
canonical repository:
  flopeek-context/flopeek

target diagnostic language:
  TypeScript / TSX

core authority:
  Rust

persisted authority:
  SQLite

JavaScript repository authority:
  false for new product capabilities

LLM required:
  false

automatic static root-cause claims:
  false
```

CI/tests should detect regressions such as:

- mandatory LLM dependency added;
- new diagnostic graph truth implemented in JavaScript;
- silent JavaScript fallback introduced;
- historical output exposes `rootCause` as deterministic truth;
- V1 claims multi-language diagnostic parity;
- stale evidence is silently treated as current;
- package/release metadata points to historical authority.

---

## 23. Branch and change policy

`main` is the canonical protected branch.

Use short-lived branches.

Recommended patterns:

- `chore/canonical-authority`
- `feature/rust-authority-ts`
- `feature/diagnostic-context`
- `feature/diagnostic-assertions`
- `feature/historical-diagnosis`
- `feature/diagnostic-packet`
- `fix/<specific-correctness-problem>`
- `test/<specific-evidence-gap>`

Do not use agent/vendor names in branch names.

Avoid combining unrelated high-risk changes.

In particular, avoid combining:

- mechanical refactor + semantic change;
- parser semantic change + storage migration;
- SQLite migration + UI redesign;
- Rust authority cutover + unrelated feature expansion;
- compatibility cleanup + new diagnostic semantics.

---

## 24. Definition of done

A diagnostic capability is done only when:

- Rust owns authoritative behavior;
- SQLite owns persisted authority where persistence exists;
- behavior is deterministic where claimed;
- TypeScript/TSX scope is explicit;
- no LLM is required;
- no silent JavaScript authority fallback exists;
- evidence class is explicit;
- graph/source basis is explicit;
- stale state is explicit;
- history bounds are explicit;
- attribution exists for human/agent assertions;
- static history emits no root-cause truth claim;
- storage is transactional and validated;
- portable diagnostic metadata contains no source bodies, credentials, or private reasoning;
- relevant unit/integration/fixture tests pass;
- current capability is not confused with planned behavior;
- the historical TypeScript fixture demonstrates the capability.

---

## 25. Agent safety and scope rules

Agents MUST NOT:

- execute target repository application code merely to improve static evidence;
- send repository source to external services by default;
- add telemetry by default;
- add an LLM API-key requirement;
- expose arbitrary shell through diagnostic MCP tools;
- write target source through diagnostic truth APIs;
- infer hidden human intent when structured intent is absent;
- invent historical evidence;
- invent last-known-good;
- treat missing evidence as proof of absence;
- claim runtime behavior from static edges;
- call a historical candidate a cause;
- silently broaden beyond TypeScript/TSX V1;
- create a new JavaScript repository-truth implementation;
- introduce silent JS fallback for new capabilities;
- treat inherited features as roadmap commitments;
- restore historical repository authority;
- introduce another product/architecture/roadmap source-of-truth document.

When evidence is unavailable, say:

`unavailable`

When context is stale, say:

`stale`

When a historical change is only relevant, say:

`candidate`

not:

`cause`

---

## 26. Agent response discipline

When implementing or auditing Flopeek, classify conclusions using:

- **Deterministic evidence**
- **Observation**
- **Hypothesis**
- **Finding**
- **Remediation**
- **Verification**
- **Unknown / unavailable**

Do not collapse these categories into a single narrative truth.

For historical diagnosis always report:

- current basis;
- last-known-good basis if provided;
- range inspected;
- bounds/truncation;
- candidate changes;
- relevance reasons;
- unresolved dynamic/runtime behavior.

---

## 27. Final invariants

Preserve these invariants above all.

### Repository truth

> **Repository truth is computed deterministically. Engineering understanding is versioned and attributed. AI reasoning is optional and never silently promoted to repository truth.**

### Implementation authority

> **Rust is the single implementation authority for repository truth. JavaScript may transport or present that truth, but must not independently recreate it for new product capabilities.**

### Persistence authority

> **SQLite is the single persisted authority for Rust-owned repository and diagnostic state unless an explicit architectural decision changes this contract.**

### Historical truth boundary

> **Historical analysis identifies deterministic context-change candidates, not automatic runtime root cause.**

The product succeeds when a human, coding agent, or optional LLM can consume the same repository state and answer:

- What source state are we talking about?
- Why are we investigating this context?
- What deterministic evidence exists?
- What changed since last-known-good?
- Which historical changes are relevant candidates?
- What do humans or agents currently believe?
- Which conclusions are verified?
- Which context is stale?
- What remains unknown?
- What still requires source inspection or runtime verification?

That is the product boundary.

Keep it narrow.
