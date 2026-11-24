# 0407. An observing tab reads a safe request's body from the interception's parts, and refuses every reading two systems could differ on

**Status:** Accepted. Design decision, made by the worker who built package E7b, under the lead's brief. The owner had not read this record when it was merged. It changes a public interface of [`a2m-pack-web-browser`](../architecture/crates/a2m-pack-web-browser.md), `SafeRequest`, as [decision 0394](0394-an-asserted-read-of-a-web-surface-is-a-page-request-matched-by-one-shared-shape-and-shown-at-publish.md) says it must.

**Date:** 2026-11-24

**Changes:** `SafeRequest` and `SafeRequests::allows`, which move from `src/tab.rs` to `src/safe.rs`; `PausedRequest` in `src/protocol.rs`; the test application's case 39. It builds point 3 of decision 0394 and settles what that point left to the builder.

## Context

Decision 0394, point 3, says what a safe request is: a method, an origin, a path template, and a body that is none or a closed shape of the provider contract with `json` or `form`, matched in all four parts, a body failed when the browser does not give it whole, past 1 MiB, not parsing, or not fitting. It does not say where the bytes come from, what a request carries beside its path and body that a system may read, or what a template that is not well formed means. Each is a choice between real alternatives, and each was decided toward the refusal, because this is the boundary that keeps an observing tab from writing (decision 0195), and a request the tab lets through is one the person's word alone vouches for (decision 0392, point 5).

What the browser gives was measured, on Chrome for Testing 153 on Linux, by a throwaway test that printed the interception's event for a `POST` of each form of body and was then removed:

| Body | `hasPostData` | `postDataEntries` | `postData` |
| --- | --- | --- | --- |
| A text, `URLSearchParams`, a `FormData` of texts, a `Blob`, an `ArrayBuffer`, by `fetch`; a text by `XMLHttpRequest` and by `sendBeacon` | `true` | Every part, with its bytes in base64 | The same, as text |
| A `FormData` with an in-memory file | `true` | Three parts, each with its bytes | The same |
| Two mebibytes of text | `true` | Whole | Whole |
| Bytes that are not UTF-8 (`7b ff fe 7d`) | `true` | Exact | `{}`: the two bytes dropped |
| A `ReadableStream` (`duplex: 'half'`) | `true` | One part with no bytes | Absent |
| An empty text, and no body | Absent | Absent | Absent |

A file a person picks from the disk was not measured.

## Decision

1. **The bytes are the interception's parts, never `postData`.** `postData` drops what is not UTF-8, so a body checked through it is not the body sent. The parts are decoded from base64 by a decoder of the crate's own, in `src/protocol.rs`, which takes only standard base64 with its padding. A part with no bytes, a flag with no parts, parts with no flag, or an event of any other form is a body not given whole. The body is kept as the event's own values (`HeldBody`), so that a form not expected fails the request rather than failing to read the event, which would leave the request paused until its deadline. Its debug form is a count of parts.
2. **An empty body is no body.** The browser gives the two alike, so a safe request with a body refuses an empty one, and one with none accepts it.
3. **Nothing a system may read beside the path and the body is let vary.** A request whose address carries a query, even an empty one, or a fragment, matches no safe request: a template holds no query, and a system may read a query as the request's meaning (`?op=archive`). A request that carries a header overriding its method (decision 0270, point 2, the three names) matches none, whatever the header names: the stricter reading of that decision, since the method a safe request names is the one asserted.
4. **The media type is part of the body.** A `json` body is sent with one `Content-Type` of `application/json`, a `form` one with `application/x-www-form-urlencoded`, either with no parameter but `charset` of UTF-8. A body that parses as JSON but is sent as text is refused, because the system reads it by the type it was sent with.
5. **A reading that two systems could differ on does not parse.** A JSON object or a form that names one member twice is refused: one system takes the first and another the last, and the check saw only one. So is a form entry with no `=`, an empty name, an escape that is not one, and text that is not UTF-8. JSON escapes are read, so `"archive"` is `archive` and fails its `enum`.
6. **What is not well formed matches nothing.** A template that does not begin with `/`, holds a query, or has a segment with a brace that is not a whole `{name}` of letters, digits, and `_`; an origin not written as the browser writes it; and a shape whose top level is not an object closed by `additionalProperties: false`, or is nullable. Package E7c's lint refuses these where a person writes them; the base refuses them again, so that a caller's mistake closes the tab rather than opening it.
7. **A placeholder takes one plain segment.** Not an empty one, not `.` or `..` in any spelling, and none that holds an escaped `/` or `\`, or a `\`, so that no system can read it as more than one segment. A literal segment is equal byte for byte.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Read `postData` | It drops bytes, measured; what was checked would not be what was sent |
| A base64 crate | One more dependency for twenty lines, on the path of every safe request; the decoder is tested beside its encoder |
| Ignore a query, as decision 0392 compares only the path | A query is a second place a system reads what a request means; a request a person asserted was made without one |
| Judge an override as decision 0270 does, letting one that names a safe method through | An override names a method other than the one the person asserted; nothing gains from passing it |
| Read the body by the declared encoding whatever its `Content-Type` | The system reads it by its type, not by the entry's |
| Keep the last of two members of one name, as `serde_json` does | A system that keeps the first reads another request than the one checked |
| Leave a shape that is not closed to E7c's lint alone | The base is the boundary; one check where it is enforced is the one that must hold |

## Proof

`an_observing_tab_lets_through_a_safe_request_only_on_method_origin_template_and_body` (`crates/a2m-pack-web-browser/src/tests/observing.rs`), against the test application's `/cases/rows-by-post`: with no safe request the page's search is failed and reported, and the stand hears nothing; with the search asserted the rows arrive, and the same search as a `Blob`, an `ArrayBuffer`, and an `XMLHttpRequest`'s text passes; thirteen requests are each failed before they leave and reported (an `archive` body against `enum: [search]`, a field the shape does not name, a field of another kind, a name given twice, a body that is not JSON, one of another media type, one over the bound, a stream, a longer path, a query, another method, a method override, and a request no one listed), and the control interface shows no row changed and no body heard of them; the stand does archive by the same route when asked directly. The unit tests of `src/safe.rs` cover each rule above.

## Revisit if

A browser hands the interception a body in another form, or the file a person picks arrives whole; or a system is found whose read takes a query beside its body, which point 3 refuses.
