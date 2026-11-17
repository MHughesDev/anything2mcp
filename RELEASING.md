# Releasing

How a version of Anything2MCP is numbered, built, and published. The design behind it is [spec section 605](docs/spec/6-running-it/605-packaging-and-release.md); the state of each registry is in [publishing setup](docs/plan/publishing-setup.md).

## Version numbers

- **One number for everything.** The binary, every crate, the pip package, and the npm packages carry the same version. It is set in `Cargo.toml` under `[workspace.package]` and in `bindings/python/pyproject.toml`, and the audit refuses a difference.
- **Semantic Versioning.** `MAJOR.MINOR.PATCH`. A patch fixes; a minor adds; a major may break.
- **Before 1.0, a minor version may break.** Until the first stable release, `0.MINOR` is where incompatible changes go, and the changelog says so plainly each time.
- **Interfaces have their own versions.** Target files carry a schema version, the REST and local interfaces are versioned in their paths, and the MCP revisions served are listed in settings. A product release can leave all of them unchanged.
- **What counts as breaking:** a target file that used to compile and no longer does; a tool, command, route, or SDK method removed or renamed; a failure code changed; a default that makes the product less safe. A change in how a run is carried out is not breaking if the result and the evidence are the same.

## Before a release

1. `main` is green on all three operating systems.
2. The version is set in both places, and `python3 tools/docs_audit.py` reports no findings.
3. [CHANGELOG.md](CHANGELOG.md) has a section for the version, with the date, and "Unreleased" is empty again.
4. Anything the release promises is proven by a test that ran in CI on the commit being released.

## Publishing

Publishing cannot be undone. A published version can be yanked, and never replaced.

Only the release workflow publishes, with short-lived credentials that the registry issues to that one workflow file. No token is stored, and nobody publishes from their own machine.

There are two ways to start it:

- **Push a version tag.** `git tag v0.1.0 && git push origin v0.1.0`, on a commit that is on `main`.
- **Start it by hand.** In the repository's Actions tab, run the "Release" workflow on `main` and type the version to publish. Left empty, the run builds and checks everything and publishes nothing, which is how to rehearse a release.

Either way the workflow refuses to publish a version that is not the package's own, or a commit that is not on `main`.

The order, once more than one registry is involved: crates.io, the release page, PyPI, npm, then everything that installs from those.

## After a release

1. Install it in a clean environment by each route and run the command.
2. Mark the name as held in [publishing setup](docs/plan/publishing-setup.md) if this was a registry's first release.
3. Add the journal entry.

## When a release is bad

Yank it from each registry, say why in the changelog, and publish a fixed version with a higher number. A published version is never overwritten and never deleted.

## Who may release

A maintainer, or an agent working for one whose access allows it, once the checklist above passes. An agent never handles a registry token ([AGENTS.md](AGENTS.md)). An agent whose access cannot start a workflow or push a tag prepares the release and says so; a maintainer then starts it.
