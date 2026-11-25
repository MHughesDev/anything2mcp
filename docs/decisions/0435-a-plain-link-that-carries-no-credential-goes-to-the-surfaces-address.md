# 0435. A plain link that carries no credential goes to the surface's address

**Status:** Accepted. Design decision, made by the worker on field test 7's findings under the lead's brief. The owner had not read this record when it was written. It narrows the stream pack's use of [decision 0197](0197-no-client-takes-a-proxy-from-the-environment-and-stand-only-protocols.md)'s rule to the case that rule was made for; no public interface changes.

**Date:** 2026-11-25

**Changes:** `a2m-pack-stream` (`src/wire.rs`, `src/nats_wire.rs`, `src/example_wire.rs`, the sentence of `transport_refused` in `src/codes.rs` and `src/driver.rs`); the design of `a2m-pack-stream`; [spec section 309](../spec/3-packs/309-the-stream-pack.md); the generated table of failure codes in the [technical reference](../reference/technical-reference.md).

## Context

In field test 7 (finding 43) a broker that asks for no sign-in, at its address on the run's network, which is where a broker in a container of its own or on another machine is, was refused before anything was sent as `transport_refused`, "would carry the session in plain text". The surface had no sign-in, and its session carried no credential. Every later step of the test needed the broker moved into the product's own network.

The records say why the rule is there. [Decision 0188](0188-credentials-over-plain-links-drafted-workbooks-and-published-device-maps.md), point 1, is titled "A credential never crosses an unencrypted link to another machine", and refuses a plain link only for a pass whose session takes the credential. Decision 0197 (finding ST-1) brought that rule to the stream pack because `example-broker/1` sent the token over plain TCP to any address, and the code said the same: "Plain TCP carries the session's credentials in the clear, so it goes to this machine's own loopback address and nowhere else." [Decision 0240](0240-one-typed-reading-of-where-an-address-is-and-one-question-of-a-plain-link.md) made the check one question, `ScopedHost::may_leave_this_machine`, "a plain link that carries a credential is allowed only where it is false". The stream pack asked it of every connection, with a credential or without one. The HTTP and MCP packs ask it only of a request that carries one. Nothing in the records says that a broker with no sign-in must be on this machine.

## Decision

1. **A plain connection that carries the session's credential goes to this machine's loopback address only, as before.** `wire::plain_tcp_address` is unchanged and still pinned over the shared corpus of address spellings: an IP literal in `127.0.0.0/8` or `::1`, never a name, never a zone, never IPv4 written inside IPv6.
2. **A plain connection that carries none goes to the surface's address.** `wire::plain_tcp_dial(address, with_credential)` is the one check both protocols ask before they dial. With no credential it takes an IP address with its port, or a host name with its port, which the resolver turns into addresses when the connection opens. It refuses, before anything is resolved or sent, an unspecified address (`0.0.0.0`, `::`), a multicast address, an address with a zone, IPv4 written inside IPv6, a spelling only a lenient parser widens (`127.1`), a port of 0, and anything else that is not one of those forms.
3. **A server that asks for TLS is refused at any address,** with or without a credential, until this build speaks TLS (it does not). When it does, a connection over TLS may carry a credential to an address off this machine; that is the TLS client's record to make.
4. **The reach of a target does not change.** The address is the surface's, which a person registered for the target ([decision 0413](0413-the-addresses-a-person-registered-for-a-target-are-those-given-to-target-add-and-those-of-every-version-a-person-published.md), [section 502](../spec/5-security/502-defending-against-a-manipulated-agent.md), rule 2); the driver dials only a surface among the run's destinations, a channel's address is a subject and never a network address, and the broker's own list of other servers is not followed. No step input can move a connection.
5. **The sentence of `transport_refused`** now says what is refused: a credential in plain text to an address off this machine, a broker that asks for TLS, or an address no connection can be made to; and its next step names the three ways on, the loopback address, a broker with no sign-in, or a build that speaks TLS.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Keep the loopback rule for every connection | It refuses what it was not made to refuse: a broker with no sign-in shows anyone on its network what it shows this program, and the product's reach is the person's machine and every network it is on (the North Star; decision 0277) |
| With no credential, an IP address only, never a name | A broker in a container of its own is reached by its name on the container network; the HTTP pack already sends a request with no credential to a name. With no credential, what a name leads to puts no secret at stake; where it leads is the surface's, which a person registered |
| A surface field that accepts plain text, as the HTTP pack's `unencrypted_credentials: accepted` | That is for a credential on a plain link, which this record does not allow; with no credential there is nothing to accept |
| Build TLS first | It is an outside crate to verify and a client to write; it is still owed, and this record does not stand in its way |

## Proof

Each test failed before the change, against the pack's own stands.

- `a_broker_that_asks_for_no_sign_in_is_reached_at_an_address_off_the_loopback`, in `crates/a2m-pack-stream/tests/nats.rs`: a surface at a name that leads to the test server, with no session, reads a retained key as the server holds it, and the server heard one connection that presented nothing.
- `plain_tcp_that_carries_no_credential_goes_to_the_surfaces_address`, in `tests/driver.rs`: the same over `example-broker/1`, against the test broker.
- `a_connection_that_carries_no_credential_may_go_to_another_address`, a unit test of `wire.rs`: the forms taken and refused with no credential, and each address off the loopback refused with one.
- `a_credential_reaches_the_server_in_connect_and_plain_text_goes_to_loopback_only` (`nats_wire.rs`) and `a_token_goes_in_plain_text_only_to_the_loopback_address` (`example_wire.rs`): with a token or a user and password, a name that leads to the server, a documentation address, and another name are refused before the server hears anything.
- `a_server_that_refuses_the_session_or_asks_for_tls_or_is_not_on_this_machine_is_named` (`tests/nats.rs`): a server that asks for TLS is refused at a name as at the loopback address.

## What would reopen it

A broker that hands out something secret to a client that presents nothing, such as a session token in its first answer; TLS, built; or a session whose credential is placed after the connection opens.
