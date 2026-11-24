# 0412. A `url` input matches only a destination that names its scheme, host, and port as written

**Status:** Accepted. Design decision. The owner had not read this record when it was merged. Carries out package 9.3p.

**Date:** 2026-11-24

**Changes:** [section 502](../spec/5-security/502-defending-against-a-manipulated-agent.md) (its map: the `url` type is built); the [file formats](../reference/file-formats.md) (`url` among the built-in types); the designs of `a2m-files` (the code `kernel.url_without_destination`) and `a2m-engine` (the guard, the module `destination`, and `surface_not_declared` in its table).

## Context

Section 502 says an input that is an address uses a `url` field type that accepts only the destinations its operation declares. Until now the type was plain text, so a crafted address passed to a tool reached whatever pack the binding named. A destination in `permissions.destinations` is one exact address with an optional scheme and an optional port, as the compiler's `scope_invalid` check holds it. What "accepts" means had to be chosen: how an address an agent gives is read, and what makes it equal to one a person declared.

## Decision

1. **The engine holds every `url` input before anything is prepared or sent.** For an operation, against its own destinations; for a pipeline, its own `url` inputs against the destinations of the operations it calls, and each call again against its own operation's. Any other address is `policy`, `surface_not_declared`, and the message names the input, never its value.
2. **An address is read strictly.** `scheme://host[:port]`, then nothing or a path, query, or fragment; printable ASCII only, no space, no `\`; no `@` and no `%` in the host part; a host of letters, digits, dots, and hyphens, or a literal in brackets. Anything else is refused, never repaired.
3. **It matches by equality of three parts as written.** The scheme, the host without regard to case, and the port, each the destination's. No port is taken to be its scheme's usual one, and no two spellings of one machine are made equal: an address literal matches only itself, and a number some programs read as an address is a name.
4. **Only a destination that names its scheme can be matched.** Which schemes a destination with none would allow is not written anywhere, and naming them in the engine would be knowledge of one kind of system in the kernel. The compiler refuses an operation with a `url` input and no such destination, and a pipeline with one whose operations declare none, as `kernel.url_without_destination`; the compiled field type keeps the name `url`, which is how the engine knows the input.
5. **A redirect is not followed by the guard.** Where a request goes after it starts is held by the destinations the driver is prepared with, which never include an input, and by each pack's own refusal of a redirect that leaves them (the HTTP pack's is tested).

## Alternatives weighed

- **Make the usual port equal to none (`https://h` equal to `https://h:443`).** Not taken: it needs a table of schemes and ports in the kernel, and refusing the explicit form costs a person nothing but writing the destination as they will use it.
- **Let a destination with no scheme match `http` and `https`.** Not taken, for the same reason, and because a plain link would pass where the person may have meant only an encrypted one.
- **Parse with a general URL library and compare what it makes of both.** Not taken: a library repairs and normalises (percent-decoding, numeric hosts, a trailing dot), and each repair is a way for two spellings to differ between the guard and whatever later reads the address.
- **Check at compile time only.** Not possible: the value is the agent's, at run time.

## Consequences

- A target whose `url` input named a destination without a scheme does not compile until the destination is written with one.
- On a server, private and loopback addresses refused unless declared is after the release, as section 502 says; on one machine a declared loopback literal is matched like any other.
