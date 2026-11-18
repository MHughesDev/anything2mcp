# 0240. One typed reading of where an address is, and one question of a plain link

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-18

**Changes:** the designs of [`a2m-core`](../architecture/crates/a2m-core.md), [`a2m-auth`](../architecture/crates/a2m-auth.md), [`a2m-pack-http`](../architecture/crates/a2m-pack-http.md), [`a2m-pack-mcp`](../architecture/crates/a2m-pack-mcp.md), [`a2m-pack-stream`](../architecture/crates/a2m-pack-stream.md), [`a2m-pack-data`](../architecture/crates/a2m-pack-data.md), and [`a2m-provider`](../architecture/crates/a2m-provider.md); the [technical reference](../reference/technical-reference.md). Builds the first, restricting half of [decision 0223](0223-a-network-is-a-place-a-recognized-network-and-a-reach.md)'s place, as [owner decision 0222](0222-the-product-is-aware-of-the-network-its-machine-is-on.md) asks and [decision 0242](0242-the-owner-takes-the-recommended-answer-to-every-open-question.md) (point 10) orders first: a slice that sends nothing and only restricts where a request may go.

## Context

Five functions asked one question, "does a plain link to this address stay on this machine?", in five copies of nearly the same code: `link::loopback` in `a2m-auth`, `plain_to_another_machine` in `a2m-pack-http` and in `a2m-pack-mcp`, `wire::plain_tcp_address` in `a2m-pack-stream`, and `postgres::server_of` in `a2m-pack-data`. Each follows decision 0188's rule: only a loopback address written as an IP address, never a name. Decision 0222 asks that where a thing is be known and checked in one way. This record is the first piece of that: one reading, one question, and the allow-list `Destinations` carrying what the reading says.

Before anything changed, each of the five was pinned by a table test over one shared corpus of 103 address spellings, `crates/a2m-core/tests/address-spellings.txt`, which holds each function's verdict for each spelling as the code stood.

## What the five copies disagreed on

| Spelling | What each said | Kept or changed |
| --- | --- | --- |
| `127.1`, `127.0.1`, `0x7f.0.0.1`, `0x7f000001`, `2130706433`, `017700000001`, `127.000.000.001`, `127.0.0.01`, `127.0.0.1.`, `%31%32%37.0.0.1`, the same digits in full width, ideographic full stops, `user@127.0.0.1` | The three that read a URL (`a2m-auth`, the HTTP pack, the MCP pack) take the host as the URL parser writes it, `127.0.0.1`, and count each as loopback; it is also what they send. The stream and data packs parse with the standard library's strict parser and refuse each | Kept. Each judges the address it sends, and the stricter readers stay stricter. (`0127.0.0.1` is `87.0.0.1` to the URL parser, an octal part, and every check refuses it) |
| `[::1%1]:8080`, a loopback address with a zone | The stream pack allowed it: its socket-address parser takes a numeric zone, and the zone does not change `is_loopback`. The URL parser cannot read a zone, and the data pack's address parser refuses one | Tightened in the stream pack (below) |
| An address the URL parser cannot read | The MCP pack answered by its first letters: plain to another machine when it starts with `http://`, not when it does not. The other URL readers are never asked, because their callers refuse such an address first | Tightened in the MCP pack (below) |
| `::1` with no brackets and no port; `127.0.0.1:5432` | The data pack takes a bare address (its port is a field of its own) and refuses one with a port; the stream pack needs a port; a URL needs brackets around IPv6 | Kept: each is the form its surface writes |

`::ffff:127.0.0.1` (IPv4 inside IPv6), `0.0.0.0`, `::`, `localhost`, `localhost.`, any spelling of `localhost` in capitals, `app.localhost`, and every other name were refused by all five, and still are.

## Decision

1. **One reading, in `a2m-core`.** `AddressScope` is a closed vocabulary of what kind of place an address is, read from its literal alone: `this_machine`, `unspecified`, `link_local`, `private_use`, `carrier_grade_nat`, `unique_local`, `multicast`, `documentation`, `reserved`, `public`, `name`, and `unreadable`. `ScopedHost` reads a host (`of_ip`, `of_socket_address`, `of_host`, `of_destination`) and keeps its scope, its IP address, its name as written, whether it was written as IPv4 inside IPv6, and whether it carried a zone. An IPv4 address inside IPv6 is read as the IPv4 address it means. `this_machine` is a loopback literal only, never a name (decision 0192). A name is kept as a name. A spelling only a lenient parser widens (`127.1`), malformed text, and anything that is not a network address (a path) are `unreadable`, never a guess (rule 7). It lives in `a2m-core` because the layering table allows nothing else: `a2m-auth` is layer 4 and cannot see `a2m-provider` (layer 5), where decision 0223 would have put the derivation.
2. **One question.** `ScopedHost::may_leave_this_machine` is false only for a loopback address written plainly as an IP address: not a name, not IPv4 inside IPv6, not an address with a zone. The five call sites now ask it, each after its own parser, so each still judges the address it sends. A plain link that carries a credential is allowed only where it is false, as before.
3. **`Destinations` carries the scope.** `Destinations::new` reads each address with `ScopedHost::of_destination` as it builds the set, and `Destinations::scope_of` returns it. There is no other constructor and no setter. The allow-list still compares addresses as written: nothing it allowed or refused has changed. `destinations_of` in the engine is unchanged; it builds the set from the published plan's surface addresses, the environment's overrides, and the operation's manifest, and takes no input, so no input can add a destination or set a scope (section 502). A test in the engine runs a tool whose input holds an address and shows the address is not among the destinations and each scope is the one its own address gives.
4. **The name.** `a2m_files::Place` (a place in a file) and `a2m_files::Fingerprint` already exist, and `a2m_core::Address` is a finding's subject. `AddressScope` takes "scope" in the sense IPv6 uses for an address's reach (link-local, global), and is not the place decision 0223 describes: that place is derived from the address *and* the machine's own state (`local_network`, `via_gateway`), which nothing here reads. A place, when it is built, can be derived from a scope and the machine's state.

### The tightenings

Nothing that was refused before is allowed after: a test in each of the five crates keeps the old function, word for word, beside the new one, and checks every spelling of the corpus (the HTTP and MCP packs also under `https://`, and the MCP pack with no scheme and with `HTTP://`). Two things that were allowed are refused now.

- **Stream pack: a loopback address with a zone** (`[::1%1]:4222`). A zone names an interface; what a connection to `::1` through another interface does is set by no standard read here, and the other four checks never allowed one. The corpus cell says `away<here`, and `a_loopback_address_with_a_zone_was_allowed_and_is_refused` holds it.
- **MCP pack: an address its URL parser cannot read**, whatever it starts with (`HTTP://[::1` was "not plain to another machine"). Nothing can be told of such an address, so it counts as plain to another machine. Such an address cannot be connected to either, so the visible effect is at most an earlier refusal, `link_not_encrypted`, for a surface that has a session.

### Where the ranges come from

The ranges are those the Rust core library classifies, read from its source, `library/core/src/net/ip_addr.rs` (S37 of [research note 28](../research/28-how-a-network-can-be-seen-from-one-machine.md)), on 2026-11-18. That source cites each standard: RFC 1122 (127.0.0.0/8), RFC 4291 (`::1`, `fe80::/10`, `ff00::/8`, IPv4-mapped and the deprecated IPv4-compatible forms), RFC 1918, RFC 6598 (100.64.0.0/10), RFC 3927, RFC 4193, RFC 5771, RFC 5737, RFC 3849 and RFC 9637 (IPv6 documentation), RFC 2544 with erratum 423, RFC 5180 with erratum 1752, RFC 1112, RFC 919, and the IANA special-purpose address registries for the rest of its `is_global` list. The RFCs and the IANA registries themselves could not be opened from where this was written (the network refused them); the classification follows the library's code, including its unstable `is_global`, `is_shared`, `is_benchmarking`, and `is_reserved`, which are re-written here because they are not stable. Each range has a test at its first and last address and just outside, in `crates/a2m-core/src/scope.rs`. A range the registries change later is a change to this reading and its tests.

## What this does not do

| It does not | Kind |
| --- | --- |
| Resolve a name. A name is kept as written; nothing here asks a resolver | Choice (decisions 0188 and 0192): a name is the resolver's to send anywhere. Resolving a device's name once is [decision 0214](0214-ways-through-the-refusals-that-keep-a-person-from-their-own-machine.md) part 4: not yet |
| Send anything, or ask the machine anything. A scope is read from the text | Choice: the reading needs nothing sent. Reading the machine's own state is the next network slice: not yet |
| Recognize a network, or keep a reach | Not yet: decision 0223's network and reach are Proposed and not built |
| Decide anything from a destination's scope. The allow-list decides exactly as before | Not yet: the scope is there for the checks to come |
| Treat `localhost` as this machine. It is refused wherever it was refused | Choice (decisions 0188 and 0197). The fixed spelling of decision 0214 part 3 is Proposed and not built; `ScopedHost` keeps the name as written, so building it is one rule over `name()` |
| Change the proxy rule. No client takes a proxy from its surroundings | Choice (decisions 0197 and 0201). A proxy the person names is owed by decision 0208: not yet |
| Re-express the web origin guard (`Origins`) over the type | Not yet: it compares origins, not where an address is, and is another package |

## Alternatives weighed

- **Leave the five copies and add a sixth check where `Destinations` is built.** The smallest change. Refused: the copies had already drifted (a zone, an unreadable address), and a sixth would be one more to drift.
- **The type and the derivation in `a2m-provider`, as decision 0223 sketched.** Refused by the layering table: `a2m-auth`, one of the five, is below it.
- **A URL parser in `a2m-core`, so that `of_destination` reads `127.1` as the URL packs do.** Refused: `a2m-core` would gain a dependency for a lenient reading, and the strict reading never says `this_machine` where the lenient one would not, which is the direction that is safe.
- **An outside crate for the ranges.** None is needed: the standard library parses, and the ranges are a few lines each.
- **Count IPv4 inside IPv6 as this machine once it is read as IPv4.** Refused: it would allow what all five refused, and decision 0188 names it as not loopback.

## Revisit if

A place is built that needs the machine's state (decision 0223); the IANA registries add a range; `localhost` is made a fixed spelling (decision 0214 part 3); or a URL reader is found whose host differs from the address it sends.
