# 0452. An asserted read of a web surface is a page request, matched by one shared shape and shown at publish

**Status:** Accepted. Design decision, made by the design architect with the lead. The owner had not read this record when it was merged. Nothing in it is built. It changes two public contracts: the provider contract gains a module, and the file-kind interface gains a method.

**Date:** 2026-11-24

**Changes:** how package E7 is built, which becomes four packages (E7a to E7d); where [decision 0392](0392-a-page-that-reads-by-a-method-that-may-write-is-learned-from-a-persons-watched-use-and-read-through-an-asserted-request.md) put the assertion (its points 4 and 5 said the HTTP pack's entry; this record corrects that against the code); the `page_request` file kind of [`a2m-pack-web-ui`](../architecture/crates/a2m-pack-web-ui.md); the safe requests of [`a2m-pack-web-browser`](../architecture/crates/a2m-pack-web-browser.md); `a2m-provider`, `a2m-files`, `a2m-local`, `a2m-pack-http`.

## Context

Decision 0392 said the web pack reads the HTTP pack's endpoint entry to learn which requests a person asserted. Read against the code on 2026-11-24, that cannot be built as written, and four facts decide the shape of E7:

1. **A web surface's requests are not in `endpoints/`.** The web pack has its own file kind, `page_request`, in `page_requests/` (`crates/a2m-pack-web-ui/src/requests.rs`): "the requests a page makes from inside itself, in the form of entries of the endpoint catalog". A section of the compiled target belongs to one file kind and one surface kind, so an `endpoint` entry is compiled for an `http` surface and a `page_request` for a `web` one. The page request already has `method`, `path`, `page`, `effect`, `request_schema`, and `request_encoding`. Its rule today is that `GET` alone reads.
2. **The closed shape of a body exists twice.** `crates/a2m-pack-http/src/shape.rs` and a private `Shape` in `crates/a2m-pack-web-ui/src/requests.rs` each hold the same closed subset of JSON Schema. The browser base, which must check a body in its interception, can depend on neither pack.
3. **The tab does not see a request's body.** `PausedRequest` in `crates/a2m-pack-web-browser/src/protocol.rs` keeps the address, the method, and the headers' names.
4. **Nothing shows an asserted read at publish, in either pack.** The review summary (`crates/a2m-files/src/review.rs`) is built from what a lowering states about one binding. An asserted read is a statement about a surface's request, whatever tool's page sends it, and the HTTP pack's `read_asserted` is not on the page either.

## Decision

1. **One shape, in the provider contract.** `a2m-provider` gains a public module `shape`: the closed subset (`type`, `nullable`, `properties`, `required`, `additionalProperties`, `items`, `enum`), its reading from a located document with the same refusals, and one check of a value against it. Both copies are removed and both packs use it. No behaviour of either pack changes; their tests are the proof.
2. **`read_asserted` is a key of the page request.** It has the word and the meaning of the HTTP entry's key. A page request marked `read` whose method is not `GET` compiles only with `read_asserted: true`; `read_asserted` on any other entry is refused. An asserted read either gives a `request_schema` whose top level says `additionalProperties: false`, or gives none, which says the request carries no body. The lint and the lowering refuse anything else under the pack's existing code for a page request that is not valid. A drafter never writes the key. Rule 8 holds: a web surface's requests are stated in `page_requests/` and nowhere else.
3. **The base matches a safe request by template and body.** `SafeRequest` becomes the method, the origin, the path as a template (segment for segment; a placeholder matches one segment), and the body: none, or a shape with its encoding (`json` or `form`). `PausedRequest` gains the body the browser hands the interception. A request passes an observing tab only when its method is `GET`, `HEAD`, or `OPTIONS`, or it matches one safe request in all four parts. A body is failed when the browser says there is one and does not give it whole (a stream, a file part, a body past the browser's own limit), when it is longer than 1 MiB, when it does not parse in the declared encoding, or when it does not fit the shape. A form body is read as a flat object of texts. Nothing of a body is stored, logged, or put in a failure. The match by prefix is removed; no caller used it.
4. **The drivers fill the list under decision 0270's rule.** At prepare, the `ui` and `page_http` drivers read the compiled target's `page_request` section of the surface and hand the tab its asserted reads when the compiled target is a published version and the posture is `Live`, or the run's environment is of kind `test`. Otherwise the list is empty, as today. The probe and the capture source always hand an empty list. When the tab fails a request whose method and path match an asserted read of the compiled target while the list was empty, the stop's next step names the page request and says to publish it.
5. **A file kind says what it asserts, and the publish page shows it.** `FileKind` gains `asserts(&self, doc) -> Vec<AssertedRead>`, empty by default: the entry's name, its method, its path, and its body's field names with their kinds and any `enum`. The review summary gains `asserted`, one item for each, with its file kind and surface and one fixed sentence: the person's word is the only evidence that this request changes nothing. The page shows them with the sign-ins, before the tools. The `page_request` kind and the HTTP pack's `endpoint` kind both answer. This closes the same gap for the HTTP pack.

## What a capture drafts, corrected

Decision 0392, point 4, drafts "an endpoint entry" for a request by another method. For a web surface that entry is a page request, with `page` set to the screen whose page sent it, which the capture's yield names. The rest of that point stands.

## Alternatives

| Alternative | Why not |
| --- | --- |
| The web drivers read the HTTP pack's `endpoints/` section, as decision 0392 said | No such section exists for a web surface, and an entry there has no `page` |
| Keep two shapes and add a third in the base | Three statements of one rule, and the one in the interception is the one a wrong assertion passes through |
| The shape in `a2m-core` | It is the form of a body a pack checks, which is the contract's business; the core holds the shared words |
| State an asserted read on each binding's statement | The tab lets it through for every read of the surface, so it is not one binding's, and a `ui` binding names a screen and no request |

## Proof

As the roadmap rows E7a to E7d give it. In sum: both packs' tests pass over the shared shape; at the base, a request passes only on all four parts and the stand stores nothing for each that fails; through the product, a screen whose rows come by `POST` stops under the drafts naming its page request and reads from a published version; the publish page shows each asserted read of both packs with the sentence.

## Revisit if

A browser is found to hand the interception no body for an ordinary structured request; or the provider contract gains the posture that tells a try from a shadow, which would let point 4 try a published asserted read in production.
