# Releasing oafmt

Release work requires separate authorization. Use `VERSION` for the Cargo
version and `vX.Y.Z` for its matching stable tag.

## Publish crates

Run the locked release gates and each package's locked publication dry-run.
Publish serially in this order:

1. `oafmt-syntax`
2. `oafmt-oas`
3. wait until both are visible in the crates.io index
4. `oafmt-core`
5. wait until it is visible in the crates.io index
6. `oafmt`
7. wait until it is visible and installable from crates.io

Registry waits are required because Cargo verifies versioned dependencies
against crates.io; local path dependencies do not make an unpublished package
visible.

After all crates are available, rerun the release gates, create `vX.Y.Z` at the
verified release commit, and push it only when authorized. The generated
release workflow builds archives and checksums, creates the GitHub release, and
publishes the generated Homebrew formula. Do not hand-edit generated workflow
sections or the formula.

## Verify

- Install `oafmt` `VERSION` into a clean Cargo root and smoke-test YAML and
  JSON formatting.
- Confirm every expected GitHub archive and `.sha256` file exists; download,
  verify, extract, and smoke-test the available target archives.
- Install `kokjinsam/tap/oafmt`, smoke-test it, and confirm the formula
  references the released archives and checksums.

## Recover

Published crates.io versions are immutable. If publication stops partway,
record what succeeded, wait for registry visibility, and resume with the next
unpublished crate. Never republish or replace `VERSION`; faulty released code
requires a new patch version.

If the binary release or Homebrew publication fails after the crates are
published, do not republish crates or move `vX.Y.Z`. Repair the authorized
release infrastructure and rerun only the failed release step against the
existing immutable assets.
