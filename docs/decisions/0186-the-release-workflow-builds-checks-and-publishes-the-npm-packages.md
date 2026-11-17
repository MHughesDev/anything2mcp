# 0186. The release workflow builds, checks, and publishes the npm packages

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Changes:** [decision 0135](0135-the-wheel-carries-the-binary-and-a-thin-client-and-npm-uses-platform-packages.md), which said the npm packages are built from the release's binaries but left the workflow without an npm job. It also fills in item 4 of "What `release.yml` lacks" in [research note 21](../research/21-wheel-and-npm-packaging.md).

## Context

The release workflow built and checked the pip package and published it through one gated job. The npm side (a launcher and eight platform packages, with assembly and install tests in `bindings/npm/`) was tested in CI but never built by the release workflow, and nothing published it. The workflow has never run, so everything below is written from the tools' documentation and from a local run of the checks on Node 22 and npm 10.9 with a placeholder binary.

## Decision

1. **Each platform package takes its binary out of the wheel of the same target.** The job `npm-platform` (eight matrix entries, all on `ubuntu-latest`) downloads `pip-wheel-<target>`, unpacks the one `anything2mcp` or `anything2mcp.exe` under `*.data/scripts/`, runs `assemble-platform.js`, and runs `npm pack`. Nothing is compiled twice, and the npm and pip packages carry byte-identical binaries, which is what spec section 605 asks ("Nothing is recompiled per package"). Packing needs no native runner, because the binary is only copied.
2. **The whole npm set is checked together** by the job `npm-package`, through the new `bindings/npm/scripts/check-set.js`, which reads the nine packed files, not the folders they came from: nine files with the expected names; one version in all, equal to the Cargo workspace version; the launcher's optional dependencies exactly the eight platform packages at exactly that version and no other dependency; each package's `os`, `cpu`, and `libc` matching its key and its Rust target; one binary in each platform package, non-empty, with the executable bit except on Windows; no file outside a package's `files` list; and the launcher installed from the packed files with the registry address pointed at a closed port, so the runner's own platform package must come from the file, with both command names run and the version found in what they print.
3. **A publish job for npm, `publish-npm`,** gated as `publish-pypi` is: it needs `decide` and `npm-package`, runs only when `decide` says to publish, and runs in an environment named `npm`. It has `id-token: write` and no other permission change. It uses npm's trusted publishing (npm 11.5.1, installed from the registry, since Node 22's own npm is older) with `--provenance`, and no token. It publishes the eight platform packages and then the launcher, in one shell loop that stops at the first failure, so the launcher is never published before a package it names. Before each package it asks `npm view <name>@<version> version`: a version that is there is skipped, a 404 means publish, any other error stops the job. A second run after a partial failure therefore finishes the set.
4. **A bill of materials per target** by the job `bill-of-materials`: `cargo tree` of the product crate for each of the eight targets, normal dependencies only, with licence and repository, from `Cargo.lock`, uploaded as the artifact `bill-of-materials`. It uses no tool that was not already in use (`cargo` is the toolchain). It lists crates, not licence texts.

## Alternatives weighed

- **Build the binary again for npm** (the CI `bindings` job does so, on three systems). Eight more builds of up to an hour, and a binary that is not the one in the wheel, so one release would hold two binaries to audit and sign. Rejected. The cost of taking it from the wheel is that npm packing waits for the wheel job and an npm package cannot exist without its wheel building; that is already so for the release as a whole.
- **Share the binary through a separate artifact** that the `wheels` job uploads. It would avoid unzipping, but maturin builds inside a container for the Linux targets and the target folder's place there is not documented in the action; the wheel is the file we ship, so it is the safer source. Rejected.
- **One publish job for all nine packages, against one job per package.** One job per package would show each in the interface, but each would need the `npm` environment approval (nine approvals), and the order platform-first would need `needs` chains that a skipped-package rerun complicates. One job with a loop and a skip of what is already published gives one approval and a rerun that finishes the set. Chosen.
- **`npm publish` against `npm stage publish`.** Staged publishing waits for a maintainer with two-factor authentication, which stops an unattended release, but research note 21 records that a publisher made after 2026-09-03 allows only staged publishing unless the owner also allows `npm publish`. The job uses `npm publish`, behind the `npm` environment's approval, which is the same gate as PyPI's; the owner may choose staged publishing later by changing one line and the publisher's setting.
- **npm's publish gated behind PyPI's** (spec 605 lists PyPI before npm). Rejected: the two jobs are independent so that a failure at one registry can be rerun alone ("Re-run failed jobs"). The order in 605 is kept for what installs from what: neither depends on the other.

## What is owed

- **Nothing here has run in CI.** Unproven: that the Windows wheels hold `anything2mcp.exe` under `*.data/scripts/`; that `npm publish <file> --provenance` accepts a packed file; that npm skips optional dependencies it cannot fetch in the install check on the runner's npm (it did so on npm 10.9 locally); that `cargo tree --target <triple>` works for targets that are not installed (it should: it resolves, it does not compile).
- **The owner's setup** is listed in [publishing setup](../plan/publishing-setup.md): the npm organization, a first publish of each of the nine names (trusted publishing can only be set on a package that exists), a trusted publisher on each, the `npm` environment.
- **Not built:** crates.io (blocked by `publish = false` and the unpublished `a2m-*` crates, no decision yet); signing of any artifact and build provenance for the binaries themselves (npm's provenance covers the npm packages; PyPI's attestations cover the wheels); the release page with archives, checksums, and installers; third-party licence texts, and a bill of materials in an exchange format (SPDX or CycloneDX). The last needs a new tool (`cargo-cyclonedx`, `cargo-about`, or `cargo-sbom`), and **which one is left open**: none is chosen here because a new tool needs the same checks as a new dependency. Spec section 605 still says "no npm job" under "Today"; it should be updated with this record.

## What would reopen it

A second binary appearing in any package (the check would pass it if each package held one), the wheel layout changing so the binary is no longer under `*.data/scripts/`, or the registry dropping npm's `publish` path for trusted publishers.
