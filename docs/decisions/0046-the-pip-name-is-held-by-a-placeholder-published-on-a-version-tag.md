# 0046. The pip name is held by a placeholder, published only when a version tag is pushed

**Status:** Accepted by the agent, under the owner's instruction of 2026-11-16 to keep working without review. The owner had not read this record when it was merged. The publishing step itself waits for the owner's explicit word.

**Date:** 2026-11-16

## Context

The name `web2mcp` on PyPI is not held until a first release is published from `.github/workflows/release.yml`, the file the pending trusted publisher names ([publishing setup](../plan/publishing-setup.md)). The real pip package ships the Rust binary ([decision 0025](0025-pip-and-npm-packages-ship-the-binary-with-a-thin-sdk-no-in-process-bindings-yet.md)), and that binary does nothing yet. A published version can never be replaced, and the publishing setup leaves the publishing step to the owner.

## Decision

- The first release is a placeholder: a pure-Python package in `bindings/python/` that holds the name, says it contains no working software, and links to the repository. Its version is 0.0.1, the same as the placeholders on crates.io and npm.
- It is built with `flit_core`, pinned to one exact version, with file times taken from the commit. Two builds of the same commit give identical files.
- `release.yml` builds, checks, and installs the package on every pull request that touches it, and publishes nothing.
- Publishing happens only when a tag of the form `v<version>` is pushed. The workflow refuses a tag that does not equal the package's version or that is not on `main`. The publish job runs in a GitHub environment named `pypi` and uses PyPI's trusted publishing, so no token exists.
- Pushing that tag is the publishing step. An agent does not push it without the owner's explicit word.
- Every package carries one version. The workspace's version in `Cargo.toml` and the pip package's version must be equal, and `tools/docs_audit.py` checks it, along with the copy of the licence that the pip package has to carry.

## Consequences

- The workflow file exists under the name PyPI expects, and its build half is proven on every change, before the first real release depends on it.
- Version 0.0.1 on PyPI will be used up by the placeholder. The first real release takes a later number.
- The workflow's outside parts are: `actions/checkout`, `actions/upload-artifact`, and `actions/download-artifact` (MIT); `pypa/gh-action-pypi-publish` (BSD-3-Clause); and the build tools `build` (MIT), `twine` (Apache-2.0), and `flit_core` (BSD-3-Clause). Each action is pinned by commit and each tool by version.
- The licence text is in the repository twice, because a pip package can only include files inside its own folder. The audit keeps the two identical.
- If the owner adds required reviewers to the `pypi` environment, GitHub itself asks a person before every publish.
- This is not the real release pipeline. Wheels that carry the binary, the choice of `dist` or hand-written workflows, signing, and the other registries remain open, as spec section 41 and research brief 28 say.

## Alternatives considered

- Publish the placeholder by hand with a token, as was done for the other two registries: the name would be held, but trusted publishing would stay unproven, and a token would have to exist.
- Build the placeholder with maturin now, since the real package will use it: it would publish a binary that does nothing, and it tests packaging questions that belong to research brief 28.
- Publish from a manual run of the workflow instead of a tag: a tag ties the release to one commit and one version, and spec section 41 already starts the pipeline from a tag.

## Revisit if

The real pip package is ready, or PyPI's rules for trusted publishing change.

## Spec sections

- [41-packaging-and-release.md](../spec/41-packaging-and-release.md)
- [29-repository-crates-and-packaging.md](../spec/29-repository-crates-and-packaging.md)
