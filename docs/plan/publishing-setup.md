# Publishing setup

Where each package name stands and how releases will authenticate. Recorded on 2026-11-15. No token is stored in this repository or anywhere in these documents, and none should be.

## Status of the names

| Registry | Name | Status | What remains |
| --- | --- | --- | --- |
| crates.io | `web2mcp` | **Held.** A placeholder, version 0.0.1, is published | Publish the real crate as a later version |
| npm | `w2mcp` | **Held.** A placeholder, version 0.0.1, is published | Publish the real package as a later version |
| PyPI | `web2mcp` | **Not held yet.** A pending trusted publisher is configured, which does not reserve the name. The workflow and a placeholder package now exist in this repository; nothing has been published | Push the tag `v0.0.1`; see "Publishing the placeholder" below. Only that secures the name |

The placeholders contain a description, the Apache-2.0 license, and a link to this repository. They were built outside this repository and are not part of it.

## How releases authenticate

The design calls for short-lived credentials issued to the release pipeline, never stored tokens ([spec section 41](../spec/41-packaging-and-release.md)). That is set up or available on all three registries.

| Registry | Arrangement | State |
| --- | --- | --- |
| PyPI | Trusted publishing from GitHub Actions | Configured as pending: repository `MHughesDev/web2mcp`, workflow `release.yml`, any environment |
| crates.io | Trusted publishing from GitHub Actions | Not configured yet. Now that the crate exists, the owner can add it in the crate's settings on crates.io |
| npm | Trusted publishing from GitHub Actions | Not configured yet. Possible once the package has a published version |

**Constraints this places on the build.**

- The release workflow must be the file `.github/workflows/release.yml`. PyPI will refuse a publish from any other file name.
- The first release from that workflow should come early in Phase 0, as a placeholder, because the PyPI name is not secure until then.
- When the crates.io and npm trusted publishers are added, they should name the same workflow file.

## Publishing the placeholder

Added on 2026-11-16 with slice 0.2 ([decision 0046](../decisions/0046-the-pip-name-is-held-by-a-placeholder-published-on-a-version-tag.md)).

- `bindings/python/` holds a placeholder pip package, version 0.0.1. It contains no working software and says so.
- `.github/workflows/release.yml` builds and checks it on every pull request that touches it. It publishes only when a tag `v0.0.1` is pushed on a commit that is on `main`.
- **The publishing step is pushing that tag.** It cannot be undone: a version published to PyPI can be yanked and never replaced. The owner pushes the tag, or tells an agent in so many words to push it.
- The publish job runs in a GitHub environment named `pypi`. Adding required reviewers to it, in the repository's settings, makes GitHub ask a person before every publish.
- After the publish, check `pip install web2mcp` in a clean environment and mark the name as held in the table above.

## Tokens

- The registry tokens used to publish the placeholders were single-purpose and should be revoked by the owner. They are not needed again once trusted publishing is in place.
- If a token is ever needed for a release step, it belongs in the repository's GitHub Actions secrets, entered by the owner. It never goes in a file, a commit, a document, or a chat with an agent.
- The agreed secret names are `CARGO_REGISTRY_TOKEN` for crates.io and `NPM_TOKEN` for npm. A workflow reads them by those names. Whether the owner has added them can be seen in the repository's settings, not from these documents; a workflow must fail with a clear message when one it needs is missing.
- An agent building this project must not ask for a registry token to be pasted into the conversation. It prepares the release and the owner runs or approves the publishing step.
