# Security policy

Anything2MCP sits between AI agents and real systems, holds signed-in sessions, and carries out writes. A flaw in it can do real damage, and reports are taken seriously.

## Reporting a vulnerability

**Do not open a public issue for a security problem.**

Report it privately through GitHub: on the repository's **Security** tab, choose **Report a vulnerability**. This opens a private advisory that only the maintainers can read.

If that option is not shown, open an ordinary issue that says only that you have a security report to make, with no details, and a maintainer will arrange a private channel.

Please include:

- what you found, and where;
- the steps to reproduce it, or a proof of concept;
- what an attacker could gain;
- the version or commit you tested.

## What to expect

This is a small project. We aim to acknowledge a report within seven days, to tell you what we found within thirty, and to credit you in the release notes unless you would rather not be named. Please give us a reasonable time to fix a problem before you publish it.

## Supported versions

No working version has been released. Until one is, fixes are made on `main` only. From the first release on, the latest minor version receives security fixes; this page will say when that changes.

## What counts

The product makes specific promises, and a way to break one is a vulnerability:

- a secret, such as a cookie, token, or password, reaching a tool's caller, a log, a trace, a capture, or an error message;
- a write that happens without being recorded first, without being read back, or twice after a resume;
- an approval that can be given, forged, or replayed by an agent, or reused for a different change;
- an input that changes where a request goes, which file is read, or which command runs;
- one user's session being used by anyone but that user or an agent acting for them;
- a plug-in or provider doing something its manifest did not declare;
- discovery writing to the system it is inspecting;
- anything that lets a caller run code the product did not intend to run.

Also in scope: the release pipeline, and anything that would let a tampered build be published.

## What does not count

- A system that the product is pointed at behaving badly. Report that to its owner.
- Using the product against a system without the right to. That is misuse, not a flaw in the product, and the product is designed not to help: it does not get past sign-in, licence, or bot protections.
- Findings that need an attacker to already control the user's machine or operating-system account: a hostile program that replaces the engine, changes its files, or reads its memory ([the limit](docs/spec/2-kernel/222-front-doors-and-the-local-engine.md#what-may-never-be-done-from-a-local-command)). An agent that runs as the user and gets a write approved, a version published, or a protected setting loosened without a person passing the operating system's presence prompt is not such a finding: it counts, as above.

## The design

The security model is written down: [the security model](docs/spec/5-security/501-security-model.md), [defending against a manipulated agent](docs/spec/5-security/502-defending-against-a-manipulated-agent.md), [when the engine itself fails](docs/spec/2-kernel/224-when-the-engine-itself-fails.md), and [effects, hazards, and consent](docs/spec/2-kernel/213-effects-hazards-and-consent.md). A gap in the design is as welcome a report as a bug in the code.
