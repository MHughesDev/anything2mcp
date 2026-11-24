# 0313. The place of an address is derived from its scope and the machine's subnets

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-23

**Changes:** the designs of [`a2m-provider`](../architecture/crates/a2m-provider.md) and [`a2m-pack-web-browser`](../architecture/crates/a2m-pack-web-browser.md); the [technical reference](../reference/technical-reference.md); the [glossary](../reference/glossary.md). Builds package N2b, the second piece of [decision 0223](0223-a-network-is-a-place-a-recognized-network-and-a-reach.md)'s place, after the scope of [decision 0240](0240-one-typed-reading-of-where-an-address-is-and-one-question-of-a-plain-link.md) and the machine's own state of [decision 0243](0243-the-machine-reads-its-own-network-state-and-sends-nothing.md). It sends nothing and only restricts: no check refuses anything it allowed before.

## Context

Decision 0223 names a place for every destination: `this_machine`, `local_network`, `public_internet`, `via_gateway`, or `unknown`. Decision 0240 built the half that the address's literal gives (`AddressScope`) and said a place also needs the machine's state. Decision 0243 built the reading of that state, `a2m-netstate`. This record joins them.

## Decision

1. **`NetworkPlace`, in `a2m-provider`.** The five names above, a closed enum with `as_str` and `ALL`. It lives in `a2m-provider`, as decision 0223 placed the derivation, and not in `a2m-core` as that record sketched the type: the derivation needs `a2m-netstate`, a foundation that `a2m-provider` may see, and nothing below the provider needs a place yet. The name is `NetworkPlace` because `Place` already means a place in a file in `a2m-files`.
2. **`MachineState`**, the subnets of the interfaces that are up, each an address with its prefix, read by `a2m-netstate::interfaces()` and never taken from an input. A machine whose interfaces cannot be read is `MachineState::unread()`; a loopback address names no subnet; a subnet of prefix length 0 is no subnet.
3. **The derivation, `NetworkPlace::of(&ScopedHost, &MachineState) -> Placed`**, where `Placed` carries the place and the reason in words. In order:
   - A loopback address written plainly as an IP address is `this_machine`, whatever the state; one written with a zone or inside IPv6 is `unknown` (the rule of decision 0188 and the question `may_leave_this_machine`).
   - A name, an unreadable address, the unspecified address, and a multicast address are `unknown`. A name is never resolved (decisions 0188 and 0192).
   - With the state unread, everything else is `unknown`.
   - An address inside the subnet of an interface that is up is `local_network`, in whatever range the address is (a documentation range on a lab network is the lab's own).
   - Otherwise a public address is `public_internet`; link-local, private, unique local, shared, documentation, and reserved addresses on no attached subnet are `unknown`, because a route to them is a fact this reading does not hold.
   - `via_gateway` is in the vocabulary and is never given: no gateway can be named until the reach (N3) and the proxy of decision 0208 are built. `NetworkPlace::is_direct` is false for it only.
4. **`Destinations::place_of(address, &MachineState)`** derives the place from the scope read when the set was made. There is no setter and no constructor that takes a place, so no input can say where a destination is (section 502). The allow-list still compares addresses as written and decides nothing from a place.
5. **`Origins::places(&MachineState)`**, in `a2m-pack-web-browser`: the place of each origin a request may go to, with the reason. The guard still compares origins and does not consult the place.

## What the work showed (H-43 and H-46)

- **H-43, on one system.** Over the 103 spellings of the shared corpus, written bare and as URLs, and under a machine whose state is unread and one whose subnets are given, every address has a place, no name has any but `unknown`, and `this_machine` is exactly the plainly written loopback addresses (`every_destination_of_the_corpus_has_a_place_derived_from_its_scope_and_the_machine`). On the machine the tests ran on (Linux), an address of each of its own interfaces that is up and states a prefix is `local_network` (`the_machine_places_an_address_of_its_own_up_interface_on_its_local_network`); this sandbox's interface carries an address in a documentation range, which a first version read as `unknown` and the test caught, so the subnet is now asked before the special-purpose ranges. Windows and macOS are not yet run; their interface readers are those of N1. The test application is `this_machine` by its own address, and its other loopback name, `localhost`, is `unknown`: the cost of never resolving a name on the run path is that every destination written as a name is `unknown`. Whether that is acceptable waits for a decision that makes a place decide something.
- **H-46, two of the four checks.** The web origin guard is not absorbed. An origin is finer than a place: two origins can share one place and the tab must still refuse the other, and a place is coarser than what the guard compares. So the guard keeps its comparison and gains the place beside it. The proxy rule is not absorbed either: it is a launch rule (`--no-proxy-server`, decision 0197), and its place, `via_gateway`, has nothing to be derived from until a person can name a gateway. No line count shrank: `place.rs` is new code of about 230 lines before its tests, and the guard's `origin.rs` grew by one method. With the five checks of N2a, the register's count is: one of four (the loopback literal) is one question over one type; the guard and the proxy are not. The register's status stays open and a note is added.

## Alternatives weighed

- **The type in `a2m-core`**, as decision 0223 sketched. Refused: `a2m-core` would need the machine's state, and the one reader of it is a foundation above `a2m-core`. Nothing below `a2m-provider` asks for a place.
- **A place carried by `Destinations`, fixed when the set is built.** Refused: the machine moves between networks, so a place derived at build time would be stale; deriving at the moment of the check keeps it true.
- **Resolve a name once to place it.** Refused by decisions 0188, 0192, and section 502: a name is the resolver's to send anywhere.
- **Make the guard refuse a tab whose origin's place is `unknown`.** Refused: every test and every real surface written as a name is `unknown`, so it would close the web pack; a check that decides from a place belongs with the reach (N3), where the person says which places are in force.

## What this does not do

| It does not | Kind |
| --- | --- |
| Decide anything from a place. The guard and the allow-list decide as before | Not built yet: the reach (N3) is what makes a place decide |
| Give `via_gateway` | Not built yet: needs a person-named gateway (decision 0208) |
| Recognize a network, or read a route | Not built yet: decision 0223's network, and the routes of N1, are not joined |
| Place a name | Choice (decisions 0188 and 0192) |

## Revisit if

Windows or macOS reads an interface's prefix differently; a place must decide something and `unknown` is too common to be a stop; or a route table is needed to place a private address that is not on an attached subnet.
