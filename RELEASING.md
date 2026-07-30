# Releasing oafmt

This document describes a future authorized stable release. Preparing this
repository does not publish crates, create or push a tag, create a GitHub
release, change secrets, or modify `kokjinsam/homebrew-tap`.

## Prerequisites

- The release commit is on the default branch and the working tree is clean.
- Cargo package metadata and `Cargo.lock` are current.
- Rust 1.85 passes the MSRV check.
- Rust 1.97.1 passes formatting, maximum Clippy, tests, build, documentation
  with warnings denied, cargo-deny, package inspection, install smoke tests,
  `dist generate --check`, and `dist plan`.
- `dist` is exactly 0.32.0.
- crates.io credentials are available only for the manual crate publications.
- GitHub Actions has the separately configured token required to write the
  Homebrew formula to `kokjinsam/homebrew-tap`.

## Publish crates.io packages

Publish serially in this exact order:

1. Publish `oafmt-syntax`.
2. Publish `oafmt-oas`.
3. Wait until both versions are visible through the crates.io index.
4. Publish `oafmt-core`.
5. Wait until that version is visible through the crates.io index.
6. Publish `oafmt`.
7. Wait until that version is visible through the crates.io index.

Run each package's locked dry-run immediately before its authorized
publication. Dependent package dry-runs and publications must wait because
Cargo resolves the versioned dependencies from crates.io; local `path`
dependencies do not make an unpublished dependency available to the registry
verification step.

## Tag and binary release

After all four crates are visible and installable from crates.io:

1. Re-run the release gates and confirm `dist generate --check` is clean.
2. Create the stable tag `v0.1.0` at the verified release commit.
3. Push that tag only after explicit authorization.
4. Let the generated release workflow build the four configured archives and
   SHA-256 files, create the GitHub release, and publish the generated Homebrew
   formula to `kokjinsam/homebrew-tap`.
5. Do not manually edit generated workflow sections or the generated formula.

Pull requests run only `dist plan`; they do not build, upload, publish, or
announce a release. Prereleases are not published to Homebrew.

## Verify the release

- Confirm all four crates report version 0.1.0 on crates.io.
- Install `oafmt` into a clean temporary Cargo root and smoke-test YAML and
  JSON input.
- Confirm the GitHub release has all four archives and matching `.sha256`
  files, then download, verify, extract, and smoke-test one archive per
  supported target where runners are available.
- Run `brew install kokjinsam/tap/oafmt`, smoke-test the binary, and confirm the
  formula points to the released archives and checksums.

## Recover from a partial release

Crates.io versions are immutable and cannot be replaced. If a crate publication
succeeds but a later step fails, keep version 0.1.0 as published, fix the
remaining blocker without republishing it, wait for registry visibility, and
resume with the next unpublished crate. If released code itself is wrong,
prepare a new patch version rather than reusing or moving `v0.1.0`.

If binary release CI or Homebrew publication fails after all crates are
published, do not republish crates or move the tag. Correct the authorized
release infrastructure and rerun the failed workflow or publish the same
generated formula only after confirming it references the existing immutable
GitHub assets. Record exactly which surfaces succeeded before resuming.
