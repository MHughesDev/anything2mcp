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
2. The version is set in all places (`Cargo.toml`, `bindings/python/pyproject.toml`, and the npm launcher's `package.json` with its optional dependencies), and `python3 tools/docs_audit.py` reports no findings. The engine's OpenAPI document carries the version, so the saved copy is fetched again from the built program and both clients regenerated ([bindings/README.md](bindings/README.md)); CI fails until it is.
3. [CHANGELOG.md](CHANGELOG.md) has a section for the version headed `## [VERSION] - YYYY-MM-DD`, listing what changed, and a link line `[VERSION]: ...` at its foot, and "Unreleased" holds no entry again. Where `contracts/next-breaks` lists a break, the section says so. `python3 tools/release_files.py changelog VERSION` says whether it is ready; the release workflow refuses to publish a version it is not ready for, and the section becomes the release page's notes.
4. The notices file is current: `python3 tools/notices.py check` (CI's dependencies job runs it on every pull request). After a change to `Cargo.lock`, `python3 tools/notices.py write` writes `THIRD-PARTY-NOTICES.txt` and its copy in `bindings/python/`.
5. The release keeps its contract ([decision 0437](docs/decisions/0437-what-a-release-publishes-is-kept-and-every-later-build-is-checked-against-it.md)): `python3 tools/contracts.py write VERSION`, on the commit to be released, writes `contracts/VERSION/` from the tree (the API description of the built actions, the failure codes with their classes, the closed and open vocabularies of `a2m-core`, the settings keys that are built with their types, a key marked not built being left out until its part is built ([decision 0455](docs/decisions/0455-a-release-keeps-only-the-settings-it-built-and-the-failure-codes-something-raises.md)), and the compiled target of the `example` target with its hash, which it compiles with cargo) and moves `contracts/next-breaks` into it as its `breaks`. Commit the folder before the tag; it is never changed after. The tool refuses when the tree breaks a kept release with a break not listed, and refuses a listed break in a version that may not break: before 1.0 only a minor version may, from 1.0 only a major one. The release workflow refuses a version whose folder is not there or not whole.
6. Anything the release promises is proven by a test that ran in CI on the commit being released.

### A break on purpose

Every build is checked against every kept release: CI's documents job runs `python3 tools/contracts.py check`, and `a2m-core`'s test `kept_contracts` loads each kept compiled target with its hash. The check names each break on one line, such as `code.removed:some_code` or `input.required:docs.get.topic`. A break made on purpose is listed in `contracts/next-breaks`, one line each, as the check names it, then a space and the number of the decision record that made it; a `#` begins a comment. The check passes a listed break and nothing else, and a listed break that does not happen is a problem too. Additions pass without a listing.

## Publishing

The first version to publish is 0.1.0, once the first hour works from a published package ([decision 0242](docs/decisions/0242-the-owner-takes-the-recommended-answer-to-every-open-question.md)); the manifests carry 0.0.1 until then. Publishing cannot be undone. A published version can be yanked, and never replaced.

Only the release workflow publishes, with short-lived credentials that the registry issues to that one workflow file. No token is stored, and nobody publishes from their own machine.

There are two ways to start it:

- **Push a version tag.** `git tag v0.1.0 && git push origin v0.1.0`, on a commit that is on `main`.
- **Start it by hand.** In the repository's Actions tab, run the "Release" workflow on `main` and type the version to publish. Left empty, the run builds and checks everything and publishes nothing, which is how to rehearse a release.

The workflow builds eight platform wheels and a source distribution with maturin and checks all nine files together. What it publishes is the real pre-alpha package, not a placeholder.

For npm it takes the binary out of each wheel (so the npm and pip packages carry the same binary, built once), puts it in that platform's package, packs the eight platform packages and the launcher, and checks the nine together: one version, the launcher naming exactly the eight at that version, the right `os` and `cpu`, one executable binary in each, nothing outside the `files` list, and the launcher installed from the packed files and run under both names. Later, one job waiting on the `npm` environment publishes the eight platform packages and the launcher last, with npm's trusted publishing and provenance and no token. **If it stops halfway**, some platform packages are on the registry and the launcher is not. Re-run the failed job (Actions, Re-run failed jobs): it asks the registry for each version first and skips one that is already there, so it finishes the set. A version already published is never replaced; if one of the published binaries was wrong, publish a higher version. The npm and PyPI publish jobs are independent: either can be re-run alone.

It also takes the binary out of each wheel once more and packs a standalone archive per target (a `.tar.gz` for Linux and macOS, a `.zip` for Windows) with `a2m`, `LICENSE`, and the notices file. Then it gathers every file of the release in one place: the eight archives, the eight wheels and the source distribution, the nine npm packages, a bill of materials per target in CycloneDX (the one maturin writes into each wheel), the notices file, and `SHA256SUMS` over all of them, and checks the set: every file there and no other, each checksum its file's own, each package carrying the licence texts as the tree has them, and for each target the same binary in its wheel, its npm package, and its archive (`tools/release_files.py`, [decision 0464](docs/decisions/0464-the-release-carries-archives-a-notices-file-and-attested-provenance-and-a-check-holds-the-workflow-to-every-piece.md)). All of this runs on every pull request that touches the packages or the release files, and publishes nothing.

When it is to publish, the job `publish-release`, waiting on the `release` environment, checks the files against their checksums again, attests the build provenance of every one (signed with the run's short-lived credential; nobody holds a key), and makes the release page `vVERSION` with every file and the changelog's section as its notes, marked a pre-release before 1.0. Started by hand, it makes the tag on the commit it checked. Run again, it adds a file the page lacks and never replaces one. Only then do the PyPI and npm jobs run. To check a downloaded file: `sha256sum --check --ignore-missing SHA256SUMS`, then `gh attestation verify FILE --repo MHughesDev/anything2mcp`.

The workflow also keeps a plain list per target of the Rust crates compiled into the binary, with their licences, as the run's `bill-of-materials` artifact. It does not publish to crates.io, which the product is not published to ([decision 0242](docs/decisions/0242-the-owner-takes-the-recommended-answer-to-every-open-question.md)), and makes no installers, Homebrew or winget entries, or container image; see [publishing setup](docs/plan/publishing-setup.md). Before the first publish, the owner has setup to do: the `release`, `pypi`, and `npm` environments, PyPI's pending publisher, and npm's first publish and publishers.

Either way the workflow refuses to publish a version that is not the package's own, a commit that is not on `main`, a version with no whole kept contract in `contracts/`, or a version the changelog is not ready for. `python3 tools/release_files.py workflow` (in CI's documents job) fails when a job of the release is missing or a job that may write is not gated.

The order the design asks for: the release page, PyPI, npm, then everything that installs from those. PyPI and npm each wait for the release page, and not for each other, so either can be re-run alone.

## The first release, 0.1.0

Everything an agent can build for it is built; these steps are the owner's, because each acts on a registry or on the repository's settings ([publishing setup](docs/plan/publishing-setup.md) has the detail):

1. Create the GitHub environments `release`, `pypi`, and `npm`, each with required reviewers if a person should approve each publish.
2. On PyPI, add the pending publisher for `anything2mcp` (workflow `release.yml`, environment `pypi`).
3. On npm, create the organization `anything2mcp`, publish each of the nine packages once from a rehearsal run's `npm-package-*` artifacts, and add a trusted publisher to each (workflow `release.yml`, environment `npm`), allowing `npm publish`.
4. Rehearse: run the "Release" workflow by hand on `main` with no version, and see every job pass, `release-files` among them.
5. Set the version to 0.1.0 in every place, write the changelog's section, keep the contract (steps 2 to 5 of "Before a release"), merge, and push the tag `v0.1.0` or start the workflow by hand with `0.1.0`.
6. Approve `publish-release`, then `publish-pypi` and `publish-npm`, and do the steps below.

## After a release

1. Install it in a clean environment by each route (pip, npm, and an archive from the release page) on Linux, macOS, and Windows, and run the command under both names.
2. Mark the name as held in [publishing setup](docs/plan/publishing-setup.md) if this was a registry's first release.
3. Add the journal entry.

## When a release is bad

Yank it from each registry, say why in the changelog, and publish a fixed version with a higher number. A published version is never overwritten and never deleted.

## Who may release

A maintainer, or an agent working for one whose access allows it, once the checklist above passes. An agent never handles a registry token ([AGENTS.md](AGENTS.md)). An agent whose access cannot start a workflow or push a tag prepares the release and says so; a maintainer then starts it.
