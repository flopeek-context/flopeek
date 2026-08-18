# Release status

Public release is intentionally disabled while canonical publication authority
is pending.

The Flopeek product name, `flopeek` package and CLI names, `.flopeek` metadata,
`fp://` Context Refs, and schema identifiers are preserved. Historical approval
records and release evidence do not authorize publication from this canonical
repository.

The current safeguards are:

- `package.json` is private and has no `publishConfig`;
- `prepublishOnly` always invokes the legacy-publication blocker;
- both imported approval records remain `not-approved`;
- the former promotion workflow is read-only and contains no tag, npm publish,
  dist-tag, or GitHub Release mutation;
- routine Dependabot version updates are disabled while GitHub security updates
  remain available;
- CI runs `npm run verify:import-safety`.

Release work may resume only after a maintainer establishes canonical package
and GitHub Release destinations, credentials, provenance, and explicit approval
records for `flopeek-context/flopeek`. It must not reuse the imported records.
