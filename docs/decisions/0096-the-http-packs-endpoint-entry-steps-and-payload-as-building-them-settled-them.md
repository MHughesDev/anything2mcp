# 0096. The HTTP pack's endpoint entry, steps, and payload, as building them settled them

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Settles:** what the design of `a2m-pack-http` and [spec section 303](../spec/3-packs/303-the-http-pack.md) left open about the `endpoints/` file kind, the steps of the `http` mode, and the pack's payload in the plan. It changes the interface sketch of the crate's design.

## Context

Package 4.1 built the `http` mode against the test application. The file formats listed an endpoint's fields without saying how a status is declared, where a shape is written, or how a value leaves an answer. The provider contract was built before the pack, and fixed what a driver is handed.

## Decisions

1. **A shape is written inside the endpoint entry, in a closed subset of JSON Schema.** `request_schema` and `response_schema` hold `type`, `nullable`, `properties`, `required`, `additionalProperties`, `items`, and `enum`. Any other word is refused where it is written, with its line. The alternative was a file beside the entry and an outside validator. The loader hands a pack only YAML documents of its own kind, and a rule the pack does not check must not look checked; a validator crate would bring a large tree for words nobody may write.
2. **An endpoint declares its answers under `responses`.** `ok` and `absent` are lists of statuses. `refused`, `conflict`, and `unavailable` are lists of a status with an optional `when`, a map from a top-level field of the body to the text it must hold, because one status often means two things. A status, or a body, that matches nothing declared is `unknown_variant`, `unexpected_status`.
3. **The pack reads and writes JSON itself.** A small reader keeps each number as its text, so a decimal never passes through a float, and refuses a name written twice. The alternative, the `arbitrary_precision` feature of the JSON crate the workspace already has, changes that crate's number type for every crate in the build.
4. **A placeholder is one whole segment of a path.** `/orders/{order_id}` is a template; `/orders-{order_id}` is refused. A value that fills one is refused when it is empty, a dot segment, or holds a separator or a control character, and every other byte outside letters, digits, `-`, `.`, `_`, and `~` is percent-encoded. The template holds no scheme, host, or port: the origin is the surface's address in the run's environment, read when the driver prepares.
5. **What each step does.** `request` builds and sends one request and holds the answer; it reports a lookup's declared `absent` status as absent, a sign of expiry as `session_expired`, and a redirect that leaves the surface as `policy`, `surface_not_declared`. `validate` judges the held answer's status and shape. `extract` takes one typed value for each output of the operation, from the answer's field of the same name or the path the binding's `extract` map gives. A read lowers to all three in `act`. A write lowers to `request` in `act`, marked as the commit point, `validate` in `confirm`, and all three in `read_back`, each marked as a lookup. The alternative, one step that sends and judges, would hide from the trace whether a refusal came from the system or from the shape.
6. **The driver refuses to send an endpoint that writes from a step that is not a commit point.** The handle checks the token only on a step marked `commits`; without this rule a step built by hand could reach a write through the read path.
7. **No redirect is followed, and no proxy is used.** Spec section 502 allows a redirect within the declared surfaces; following none is narrower and needs no rule for a write that is redirected. A same-origin redirect is judged by `validate` like any status. A proxy from the machine's environment would be a destination the plan does not name.
8. **A `probe` step is the read client.** A discoverer is handed only the sandbox port, which takes a step. The pack's discoverers build one kind of step, `probe`: a GET, HEAD, or OPTIONS request to a literal path of the surface, with no body and no credential. The driver refuses any other method in it.
9. **The version of a record is remembered by the prepared client.** An endpoint states where a version is read and where it is sent. The `extract` step of a lookup remembers the version under the record's key, and the next write to that key sends it and forgets it. The alternative, an expression that reads the lookup's result, does not exist in the plan's expression scope.
10. **TLS is not in this build.** A surface whose address starts with `https` is refused when the driver prepares, with `input`, `tls_not_enabled`, and the driver's health says the same. The client, `reqwest` with only its `json` feature, was already in the workspace; no outside crate was added.
11. **Lint codes carry the pack's name and a dot,** as the compiler requires: `http.secret_literal`, not `http_secret_literal`.

## What follows

- The file formats and spec section 303 list `responses`, `signed`, `max_response_bytes`, and the binding's `extract` map.
- Not built, and so not in the manifest: the `api` mode, the `token` step, the edge's answers, uploads and downloads, pagers, identity by a stamp or a search, the digest and integrated Windows exchanges, GraphQL, OData, and the drafting rule for endpoints seen in capture.

## What would reopen it

A service whose answers cannot be told apart by a status and a top-level field; a path that needs a placeholder inside a segment; or the web pack needing a word in a shape that this subset lacks.
