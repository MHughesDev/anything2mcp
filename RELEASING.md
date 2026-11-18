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
2. The version is set in all places (`Cargo.toml`, `bindings/python/pyproject.toml`, and the npm launcher's `package.json` with its optional dependencies), and `python3 tools/docs_audit.py` reports no findings.
3. [CHANGELOG.md](CHANGELOG.md) has a section for the version, with the date, and "Unreleased" is empty again.
4. Anything the release promises is proven by a test that ran in CI on the commit being released.

## Publishing

The first version to publish is 0.1.0, once the first hour works from a published package ([decision 0242](docs/decisions/0242-the-owner-takes-the-recommended-answer-to-every-open-question.md)); the manifests carry 0.0.1 until then. Publishing cannot be undone. A published version can be yanked, and never replaced.

Only the release workflow publishes, with short-lived credentials that the registry issues to that one workflow file. No token is stored, and nobody publishes from their own machine.

There are two ways to start it:

- **Push a version tag.** `git tag v0.1.0 && git push origin v0.1.0`, on a commit that is on `main`.
- **Start it by hand.** In the repository's Actions tab, run the "Release" workflow on `main` and type the version to publish. Left empty, the run builds and checks everything and publishes nothing, which is how to rehearse a release.

The workflow builds eight platform wheels and a source distribution with maturin, checks all nine files together, and then publishes them to PyPI through one job that waits on the `pypi` environment. What it publishes is the real pre-alpha package, not a placeholder.

For npm it takes the binary out of each wheel (so the npm and pip packages carry the same binary, built once), puts it in that platform's package, packs the eight platform packages and the launcher, and checks the nine together: one version, the launcher naming exactly the eight at that version, the right `os` and `cpu`, one executable binary in each, nothing outside the `files` list, and the launcher installed from the packed files and run under both names. Then one job waiting on the `npm` environment publishes the eight platform packages and the launcher last, with npm's trusted publishing and provenance and no token. **If it stops halfway**, some platform packages are on the registry and the launcher is not. Re-run the failed job (Actions, Re-run failed jobs): it asks the registry for each version first and skips one that is already there, so it finishes the set. A version already published is never replaced; if one of the published binaries was wrong, publish a higher version. The npm and PyPI publish jobs are independent: either can be re-run alone.

The workflow also writes a bill of materials, a list per target of the Rust crates compiled into the binary with their licences, and keeps it as the run's `bill-of-materials` artifact. It does not publish to crates.io, which the product is not published to ([decision 0242](docs/decisions/0242-the-owner-takes-the-recommended-answer-to-every-open-question.md)), does not sign any artifact or attest the binaries, and makes no release page, no standalone archives or installers, and no list of third-party licence texts; see [publishing setup](docs/plan/publishing-setup.md). Before the first npm publish, the owner has setup to do there.

Either way the workflow refuses to publish a version that is not the package's own, or a commit that is not on `main`.

The order the design asks for, once more than one registry is involved: the release page, PyPI, npm, then everything that installs from those. Today PyPI and npm are published by separate jobs that do not wait on each other.

## After a release

1. Install it in a clean environment by each route and run the command.
2. Mark the name as held in [publishing setup](docs/plan/publishing-setup.md) if this was a registry's first release.
3. Add the journal entry.

## When a release is bad

Yank it from each registry, say why in the changelog, and publish a fixed version with a higher number. A published version is never overwritten and never deleted.

## Who may release

A maintainer, or an agent working for one whose access allows it, once the checklist above passes. An agent never handles a registry token ([AGENTS.md](AGENTS.md)). An agent whose access cannot start a workflow or push a tag prepares the release and says so; a maintainer then starts it.
