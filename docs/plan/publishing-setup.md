# Publishing setup

Where each package name stands and how releases authenticate. First recorded on 2026-11-15; rewritten on 2026-11-17 for the name Anything2MCP ([decision 0056](../decisions/0056-one-name-everywhere-anything2mcp.md)). No token is stored in this repository or anywhere in these documents, and none should be.

## Status of the names

A name is held only when a release is published under it. An account on a registry, or a publisher configured in advance, reserves nothing.

| Registry | Name | Status on 2026-11-17 | What remains |
| --- | --- | --- | --- |
| PyPI | `anything2mcp` | **Not held.** The registry shows no project of this name. The owner reports the registration done; the first publish from the release workflow is the test | Start the release workflow for version 0.0.1; see "Publishing a release" below |
| crates.io | `anything2mcp` | **Not held.** The registry shows no crate of this name | A first publish. crates.io offers trusted publishing only to a crate that already exists, so the first publish needs the owner's account once |
| npm | `anything2mcp` | **Not held.** The registry shows no package of this name | A first publish, by the owner's account once; trusted publishing can be configured after it |

The status column is what each registry's public index answered on that date from the build session. It cannot see a private or pending registration.

## The earlier names

| Registry | Name | State | What to do |
| --- | --- | --- | --- |
| crates.io | `web2mcp` | The owner's placeholder, version 0.0.1 | Leave it. When `anything2mcp` is published, the owner may publish one more version whose description points to the new name |
| npm | `w2mcp` | The owner's placeholder, version 0.0.1 | Leave it. The owner may mark it deprecated with a message naming `anything2mcp` |
| npm | `web2mcp` | Another project's package | Nothing; it was never ours |
| PyPI | `web2mcp` | Never published | Nothing. No alias is maintained |

## How releases authenticate

The design calls for short-lived credentials issued to the release pipeline, never stored tokens ([spec section 41](../spec/41-packaging-and-release.md)).

| Registry | Arrangement | What the owner sets up |
| --- | --- | --- |
| PyPI | Trusted publishing from GitHub Actions | A pending publisher for the project `anything2mcp`: owner `MHughesDev`, repository `anything2mcp`, workflow `release.yml`, environment `pypi`. A publisher set up before the repository was renamed names the old repository and must be replaced |
| crates.io | Trusted publishing from GitHub Actions, once the crate exists | After the first publish: add the publisher in the crate's settings, naming the same repository and workflow |
| npm | Trusted publishing from GitHub Actions, once the package exists | After the first publish: add the publisher in the package's settings, naming the same repository and workflow |

**Constraints this places on the build.**

- The release workflow must be the file `.github/workflows/release.yml`. PyPI will refuse a publish from any other file name.
- The publish job runs in a GitHub environment named `pypi`. Adding required reviewers to it, in the repository's settings, makes GitHub ask a person before every publish.
- When the crates.io and npm publishers are added, they name the same workflow file.

## Publishing a release

[RELEASING.md](../../RELEASING.md) has the steps. In short:

- `bindings/python/` holds the pip package. At version 0.0.1 it is a placeholder with no working software, and says so.
- `.github/workflows/release.yml` builds and checks the package on every pull request that touches it.
- It publishes in two cases only: a tag `v` and the version is pushed on a commit that is on `main`; or the workflow is started by hand on `main` with the version to publish typed in. In both cases it refuses a version that is not the package's own.
- **Publishing cannot be undone.** A version on PyPI can be yanked and never replaced. The owner has asked for the placeholder to be published as soon as it can be ([journal, 2026-11-17](../journal/2026-11-17-the-rename-and-the-rules.md)).
- After a publish, check `pip install anything2mcp` in a clean environment and mark the name as held in the table above.

A build session cannot push a tag. It can start the workflow by hand, which is why the second route exists.

## Tokens

- The registry tokens used to publish the earlier placeholders were single-purpose and should be revoked by the owner.
- A first publish to crates.io or npm needs the owner's account once. If a token is used for it, it belongs in the repository's GitHub Actions secrets, entered by the owner. It never goes in a file, a commit, a document, or a chat with an agent.
- The agreed secret names are `CARGO_REGISTRY_TOKEN` for crates.io and `NPM_TOKEN` for npm. A workflow reads them by those names and must fail with a clear message when one it needs is missing.
- An agent building this project must not ask for a registry token to be pasted into the conversation.
