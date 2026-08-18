# Release status

Public release is intentionally disabled while identity isolation is pending.

The imported package name `flopeek`, CLI name, `@flopeek/*` native package
scope, legacy approval records, and legacy release evidence are compatibility
material only. They do not authorize publication from this repository.

The current safeguards are:

- `package.json` is private and has no `publishConfig`;
- `prepublishOnly` always invokes the legacy-publication blocker;
- both imported approval records remain `not-approved`;
- the former promotion workflow is read-only and contains no tag, npm publish,
  dist-tag, or GitHub Release mutation;
- routine Dependabot version updates are disabled while GitHub security updates
  remain available;
- CI runs `npm run verify:import-safety`.

Release work may resume only in the dedicated identity-isolation change after a
maintainer supplies the new package, CLI, repository, cache, environment,
Context Ref, server, native-package, and release identities. That change must
create new approval records; it must not reuse the imported records.
