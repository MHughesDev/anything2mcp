# Publishing setup

Where each package name stands and how releases will authenticate. Recorded on 2026-11-15. No token is stored in this repository or anywhere in these documents, and none should be.

## Status of the names

| Registry | Name | Status | What remains |
| --- | --- | --- | --- |
| crates.io | `web2mcp` | **Held.** A placeholder, version 0.0.1, is published | Publish the real crate as a later version |
| npm | `w2mcp` | **Held, with a step pending.** The name is registered to the owner's account. Version 0.0.1 was submitted and is waiting in npm's staging area | The owner approves or rejects the staged version on npmjs.com, which needs two-factor sign-in |
| PyPI | `web2mcp` | **Not held yet.** A pending trusted publisher is configured, which does not reserve the name | Publish a first release from the workflow below; only that secures the name |

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

## Tokens

- The registry tokens used to publish the placeholders were single-purpose and should be revoked by the owner. They are not needed again once trusted publishing is in place.
- If a token is ever needed for a release step, it belongs in the repository's GitHub Actions secrets, entered by the owner. It never goes in a file, a commit, a document, or a chat with an agent.
- An agent building this project must not ask for a registry token to be pasted into the conversation. It prepares the release and the owner runs or approves the publishing step.
