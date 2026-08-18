# Contributing to Flopeek

Thank you for improving Flopeek. Contributions must preserve its core rule:
static parser evidence, runtime observations, human verification, and agent
proposals are different evidence classes.

## Before opening a pull request

1. Explain the supported syntax, framework pattern, or Viewer behavior being
   changed.
2. Add a deterministic fixture or a pinned, auditable external scope. Do not
   infer runtime execution or business intent from static topology.
3. Keep repository/UI copy in English and state unsupported forms explicitly.
4. Run the smallest relevant Rust test first, then the workspace gates:
   `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
   and `cargo test --workspace --all-targets --locked`.
5. Keep new persisted authority in SQLite; JSON is for contracts, exports, and
   fixtures only.

## Parser and graph changes

Do not add regex-based source parsing when an existing AST/compiler adapter can
carry the fact. New edges need parser identity, source evidence, confidence,
and tests that distinguish supported syntax from excluded syntax.

## Security and privacy

Do not include credentials, source bodies from private repositories, local
paths, provider transcripts, or generated `.flopeek` cache state in a pull
request. Follow [SECURITY.md](SECURITY.md) for vulnerability reports.

## Scope

Flopeek is a technical-evidence tool. It does not execute scanned target
applications, and a contribution must not silently turn a static fact into a
runtime, ownership, or business-process claim.
