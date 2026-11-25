# Publishing setup

Where each package name stands and how releases authenticate. First recorded on 2026-11-15; rewritten on 2026-11-17 for the name Anything2MCP ([decision 0056](../decisions/0056-one-name-everywhere-anything2mcp.md)). No token is stored in this repository or anywhere in these documents, and none should be.

## Status of the names

A name is held only when a release is published under it. An account on a registry, or a publisher configured in advance, reserves nothing.

| Registry | Name | Status on 2026-11-17 | What remains |
| --- | --- | --- | --- |
| PyPI | `anything2mcp` | **Not held.** The registry shows no project of this name. The owner reports the registration done; the first publish from the release workflow is the test | Start the release workflow for version 0.1.0, the first version, once the first hour works from a published package ([decision 0242](../decisions/0242-the-owner-takes-the-recommended-answer-to-every-open-question.md)); the manifests say 0.0.1 until then. See "Publishing a release" below. It publishes the real pre-alpha wheels and the source distribution, not a placeholder |
| crates.io | `anything2mcp` | **Not held.** The registry shows no crate of this name | Nothing: the product is not published to crates.io ([decision 0242](../decisions/0242-the-owner-takes-the-recommended-answer-to-every-open-question.md), point 16). The workspace keeps `publish = false` |
| npm | `anything2mcp` | **Not held.** The registry shows no package of this name | An npm organization named `anything2mcp`, then a first publish by the owner's account once of each of the nine packages (the launcher and the eight `@anything2mcp/<os>-<cpu>` packages); trusted publishing can be configured after it. The workflow now builds, checks, and publishes the nine packages ([decision 0186](../decisions/0186-the-release-workflow-builds-checks-and-publishes-the-npm-packages.md)); it has never run |

The status column is what each registry's public index answered on that date from the build session. It cannot see a private or pending registration.

## The earlier names

| Registry | Name | State | What to do |
| --- | --- | --- | --- |
| crates.io | `web2mcp` | The owner's placeholder, version 0.0.1 | Leave it. When `anything2mcp` is published, the owner may publish one more version whose description points to the new name |
| npm | `w2mcp` | The owner's placeholder, version 0.0.1 | Leave it. The owner may mark it deprecated with a message naming `anything2mcp` |
| npm | `web2mcp` | Another project's package | Nothing; it was never ours |
| PyPI | `web2mcp` | Never published | Nothing. No alias is maintained |

## How releases authenticate

The design calls for short-lived credentials issued to the release pipeline, never stored tokens ([spec section 605](../spec/6-running-it/605-packaging-and-release.md)).

| Registry | Arrangement | What the owner sets up |
| --- | --- | --- |
| PyPI | Trusted publishing from GitHub Actions | A pending publisher for the project `anything2mcp`: owner `MHughesDev`, repository `anything2mcp`, workflow `release.yml`, environment `pypi`. A publisher set up before the repository was renamed names the old repository and must be replaced |
| npm | Trusted publishing from GitHub Actions, once the package exists; the workflow's job asks for it and provenance, and holds no token | The steps under "Before the first npm publish" below |

**Constraints this places on the build.**

- The release workflow must be the file `.github/workflows/release.yml`. PyPI will refuse a publish from any other file name.
- The publish job runs in a GitHub environment named `pypi`. Adding required reviewers to it, in the repository's settings, makes GitHub ask a person before every publish.
- The npm publish job runs in a GitHub environment named `npm`, and has `id-token: write` on that job alone. Required reviewers on it make GitHub ask a person before every npm publish.
- The job that attests every file and makes the release page, `publish-release`, runs in a GitHub environment named `release`, with `contents: write`, `id-token: write`, and `attestations: write` on that job alone; the PyPI and npm jobs wait for it ([decision 0999](../decisions/0999-the-release-carries-archives-a-notices-file-and-attested-provenance-and-a-check-holds-the-workflow-to-every-piece.md)). **The owner creates the environment `release`** in the repository's settings, with required reviewers if a person should approve the release page; a job naming an environment that does not exist creates it without protection. Its attestations need no setup: they are signed with the run's short-lived credential, and nobody holds a key.

## Before the first npm publish

The owner makes each of these; the workflow cannot, and an agent does not hold a token.

1. **Create the npm organization `anything2mcp`** on npmjs.com (it holds the eight `@anything2mcp/<os>-<cpu>` names).
2. **Publish each of the nine names once from the owner's account** (the eight platform packages, then the launcher), because npm offers trusted publishing only to a package that exists. Get the files from a rehearsal run of the release workflow (started by hand with no version): the artifacts `npm-package-*` hold the nine packed files. Publish with `npm publish <file> --access public`, two-factor authentication on, and no token saved to a file. The workflow's own first publish then skips these versions, so the next version is the first it publishes.
3. **On each of the nine packages, add a trusted publisher** (package settings on npmjs.com, or `npm trust`): repository `MHughesDev/anything2mcp`, workflow file `release.yml`, environment `npm`. A publisher expires if it is not used within 2 days of being made, so add it just before the first release the workflow publishes. The repository URL in each `package.json` must match the repository exactly (it does).
4. **Allow `npm publish` on each publisher.** Publishers made after 2026-09-03 allow only `npm stage publish` unless `npm publish` is also allowed; the job uses `npm publish`. (Choosing staged publishing instead means changing the job and the setting together.)
5. **Create the GitHub environment `npm`** in the repository's settings, and add required reviewers to it if a person should approve each publish. As with `pypi`, a job naming an environment that does not exist creates it without protection.
6. Once a release has published by this route, **set "disallow tokens"** on each package so that nothing but the workflow can publish it.

## Signing, the release page, and what is not built

- **crates.io.** No job, and none is planned: the product is not published there ([decision 0242](../decisions/0242-the-owner-takes-the-recommended-answer-to-every-open-question.md)).
- **Signing.** Every file of a release has a build provenance attestation (`actions/attest-build-provenance`), made by `publish-release` from `SHA256SUMS`; it has never run. No file is signed with a key, and `self.update`'s check of a signature is not built.
- **The release page and standalone installs:** the archives, the checksums, and the release page are built (the page only when publishing); the shell and PowerShell installers, Homebrew, winget, the container image, and the offline bundle are not.
- **Bill of materials.** A CycloneDX bill per target, taken from the one maturin writes into each wheel, is a file of the release, and the plain list of crates per target stays as a run artifact. The licence texts are in `THIRD-PARTY-NOTICES.txt`, written by `tools/notices.py` and carried by the wheels, the npm platform packages, and the archives ([decision 0999](../decisions/0999-the-release-carries-archives-a-notices-file-and-attested-provenance-and-a-check-holds-the-workflow-to-every-piece.md)).

## Publishing a release

[RELEASING.md](../../RELEASING.md) has the steps. In short:

- `bindings/python/` holds the pip package: the real pre-alpha software, one maturin wheel for each of eight platforms and a source distribution. It is no longer a placeholder ([decision 0135](../decisions/0135-the-wheel-carries-the-binary-and-a-thin-client-and-npm-uses-platform-packages.md)).
- `.github/workflows/release.yml` builds the eight wheels and the source distribution, the nine npm packages and the eight standalone archives from the wheels' binaries, on every pull request that touches the packages or the release files, checks each set together, and then checks every file of the release as one set with its checksums. It has run on pull requests; it has never published.
- It publishes in two cases only: a tag `v` and the version is pushed on a commit that is on `main`; or the workflow is started by hand on `main` with the version to publish typed in. In both cases it refuses a version that is not the package's own. It then attests every file and makes the release page behind the environment `release`, and after that publishes to PyPI and to npm in two separate jobs, each behind its own environment.
- **Publishing cannot be undone.** A version on PyPI can be yanked and never replaced. The owner has asked for the name to be held as soon as it can be ([journal, 2026-11-17](../journal/2026-11-17-the-rename-and-the-rules.md)); a publish now sends the real pre-alpha wheels, not a placeholder.
- After a publish, check `pip install anything2mcp` in a clean environment and mark the name as held in the table above.

A build session could do neither on 2026-11-17: its access to GitHub does not let it push a tag or start a workflow. Both routes are therefore the owner's for now. The manual start needs no terminal: Actions, Release, Run workflow, on `main`, with the version typed in.

## Tokens

- The registry tokens used to publish the earlier placeholders were single-purpose and should be revoked by the owner.
- A first publish to npm needs the owner's account once. If a token is used for it, it belongs in the repository's GitHub Actions secrets, entered by the owner. It never goes in a file, a commit, a document, or a chat with an agent.
- No workflow reads a registry token today: the release workflow publishes with trusted publishing and stores none. If a job ever needs one, the name is `NPM_TOKEN` for npm, and the job fails with a clear message when the one it needs is missing.
- An agent building this project must not ask for a registry token to be pasted into the conversation.
