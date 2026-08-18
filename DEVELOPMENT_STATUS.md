# Development status

**Status: WIP — Rust/TypeScript authority established; diagnostic domain next.**

- Source baseline: `badsleepyday/flopeek-core` branch `development` at
  `72a95fe1a6497683e96e90872438cd3c83b7272f`.
- Baseline role: immutable initial source snapshot only.
- Canonical repository: `flopeek-context/flopeek`, the sole active authority.
- Product identity: Flopeek names and compatibility identifiers are preserved.
- Public package and GitHub release: blocked.
- Publication authority: pending canonical destinations, credentials, and new
  approvals; historical approvals are non-authoritative.
- P0B implementation: Rust is the default repository-truth authority for the
  TypeScript/TSX V1 scope; missing Rust fails explicitly without JS fallback.
- P0C persistence: authoritative graph state uses SQLite; `graph.json` is not
  written by the Rust authority path.

`AGENTS.md` is the single human-readable authority for product boundary,
architecture, priorities, and agent behavior. Other strategy documents are
legacy or generated references subordinate to it.
