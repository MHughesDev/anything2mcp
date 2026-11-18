# 0208. The first release reaches what the person's machine reaches, and serves on that machine only

**Status:** Accepted. Owner decision, given in conversation on 2026-11-18 and written down by the lead agent. Only the owner changes it.

**Date:** 2026-11-18

**Sharpens:** [decision 0169](0169-the-first-release-is-for-one-machine.md), which set the first release's scope to one machine.

## Context

Decision 0169 said the first release is for one machine and that the server follows it. It did not say what "on the person's machine" means for what the product may reach, or for who may reach the product. The lead agent had asked the owner whether the desktop pack belongs in the first release and whether the first release may go out with no proxy support.

## Decision

The owner's words: "Whenever I said I want this to run on the user's machine, that means should be able to access anything that the person's computer is able to access and keep the MCP, a local host, only on their local machine, not over a network or for a server. So we're just imagining people using operating systems like Windows, Mac OS, Linux, etc., then being able to wrap an MCP either, say, around an application installed on their operating system or something like a website or literally just any website or public endpoint that their computer can reach."

Two rules follow.

1. **What the product reaches.** On Windows, macOS, and Linux, a person can build tools over whatever their own computer can reach: an application installed on it, a website, a public endpoint. The ten rules still decide how: openly, with no stealth and no bypass, with every write verified, and only where the person is entitled to be.
2. **Who reaches the product.** The MCP server and every other door are on the person's machine only. Nothing listens for another machine. There is no network door and no server in the first release.

## Consequences

- **The desktop pack is in the first release, on all three operating systems.** Its first slice reads on Linux only ([decision 0198](0198-the-desktop-pack-reads-linux-applications-through-a-look-only-back-end.md)). Windows and macOS back ends, and writes, are owed before the first release.
- **A machine that reaches the outside through a proxy must still be served.** Today no client takes a proxy from its surroundings and no proxy can be set ([decision 0197](0197-no-client-takes-a-proxy-from-the-environment-and-stand-only-protocols.md), [decision 0201](0201-a-resume-is-never-lost-the-browser-takes-no-proxy-and-no-product-build-speaks-a-stands-protocol.md)). That was right as a safety fix and is not enough as a product: a proxy the person names on purpose, in protected settings, is owed. It is never taken silently from an environment variable, and never used for an address on the machine itself.
- **Limits that keep a person from something their machine can reach are to be looked at again**, each on its merits: the command pack's refusal of a program in a folder a group can write to, and the key that holds an action word in a web address. A limit that protects the person stays, with a way through that the person chooses and is shown.
- **Nothing listens beyond the machine.** As built, the engine's channel is a socket or a named pipe of the person's own, the MCP door is a program the client starts, and the approval page listens on the loopback address only. The setting `server.listen` exists in the schema and nothing reads it. A rule test that no product code opens a listener on any address but loopback, and a named refusal of the `server` settings in this release, are owed.
- The server, the runner over a network, and shared installations stay after the first release, as decision 0169 says.

## Alternatives weighed

None by the agents: this is the owner's statement of what the product is. The lead had recommended that the desktop pack's Windows and macOS halves follow the first release; the owner's words put applications installed on the person's operating system at the centre of it.
