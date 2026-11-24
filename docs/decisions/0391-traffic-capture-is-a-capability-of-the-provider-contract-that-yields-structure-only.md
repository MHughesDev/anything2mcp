# 0391. Traffic capture is a capability of the provider contract that yields structure only

**Status:** Accepted. Design decision; it changes a public contract, the provider manifest and the extension points of [`a2m-provider`](../architecture/crates/a2m-provider.md). Carries out point 10 of [decision 0390](0390-the-open-questions-of-2026-11-24-are-settled-by-the-lead-and-the-first-release-is-what-progress-counts-toward.md). The owner had not read this record when it was merged.

**Date:** 2026-11-24

**Changes:** the provider manifest's `[points]` (a new key, `traffic_capture`), the extension points and the registry of [`a2m-provider`](../architecture/crates/a2m-provider.md), the observing tab of [`a2m-pack-web-browser`](../architecture/crates/a2m-pack-web-browser.md), the `web_capture` discoverer and the manifest of [`a2m-pack-web-probe`](../architecture/crates/a2m-pack-web-probe.md), the technical reference, spec section [301](../spec/3-packs/301-the-web-pack.md), roadmap package E2.

## Context

Decision 0390 point 10 settled the shape: traffic capture joins the provider contract as a capability a pack may offer and declares in its manifest; a pack that does not offer it is asked nothing; what a capture yields passes the redactor before it leaves the pack (rule 4) and keeps structure only ([decision 0195](0195-a-read-never-runs-where-the-page-may-write-and-a-page-keeps-only-its-structure.md)). Until now the web probe's endpoint inference read the page's own requests from the base's `ObserveTab::exchanges`, a path private to the web pack: no other pack could offer a capture, and nothing in the kernel stood between what a pack recorded and what reached a finding.

## Decision

1. **A pack offers traffic capture by listing surface kinds under `points.traffic_capture`** in its manifest, and registering one `CaptureSource` with `Registrar::traffic_capture`. A pack that lists none and registers one is refused whole, `policy`/`effect_not_declared`, as for any undeclared point; one that lists none is never asked. `Registry::captures_for(kind)` gives a `CaptureHandle` for each pack whose manifest lists the kind, and `CaptureHandle::open` refuses a surface of any other kind before the pack hears of it.
2. **The interface, as built:**

   ```rust
   pub trait TrafficCapture: Send + Sync {
       fn start(&self);
       fn take_recorded<'a>(&'a self, until: Deadline) -> BoxFuture<'a, CapturedTraffic>;
   }
   pub trait CaptureSource: Send + Sync {
       fn open<'a>(&'a self, surface: &'a SurfaceRef, deadline: Deadline)
           -> BoxFuture<'a, Result<Box<dyn TrafficCapture>, Failure>>;
   }
   pub struct CapturedTraffic { pub requests: Vec<CapturedRequest>, pub dropped: usize, pub awaited: usize, pub withheld: usize }
   pub struct CapturedRequest {
       pub method: String, pub origin: String, pub path: String,
       pub query: Vec<String>, pub query_left_out: bool,
       pub credential_headers: Vec<String>, pub set_headers: Vec<String>, pub headers_left_out: bool,
       pub status: u16, pub media_type: String,
       pub fields: Option<BTreeMap<String, String>>, pub names_withheld: bool,
   }
   pub async fn take_capture(capture: &dyn TrafficCapture, until: Deadline, redact: &dyn Redact) -> CapturedTraffic;
   pub fn kept_by_structure(raw: CapturedTraffic, redact: &dyn Redact) -> CapturedTraffic;
   ```

   A yield has no field a value could be put in: no query value, header value, cookie, or body.
3. **Every yield passes the contract's check, whatever the pack did.** `take_capture` is the one way a yield is read, and `Capturing::take`, through the registry, applies it with the host's redactor. A request whose method is not capitals, or whose origin is not a scheme and an authority with no user part, or either of which holds a secret the redactor knows, is withheld whole and counted in `withheld`. A path is cut at a query or a fragment (`query_left_out`). One predicate, `is_captured_word`, holds every path segment, every name of a query's parameter or a header, and each part of a field's name: ASCII letters, digits, `_`, `-`, and `.`, not dots only, at most 32 characters (`LONGEST_CAPTURED_WORD`), at most three digits in all (`MOST_CAPTURED_DIGITS`: `v2`, `oauth2`, `s3`, and `A-100` are kept; an identifier, a date, or a token is not), and no run of more than 15 hexadecimal digits (`LONGEST_HEX_RUN`, a second guard). The check holds by itself, so that a pack that failed to flag an identifier, a session token, or a dictionary keyed by identifiers does not pass it as structure. A segment that fails, or holds a secret, becomes `{value}`, or stays a placeholder `{name}`; a name that fails, or holds a secret, is not kept, and `query_left_out`, `headers_left_out`, or `names_withheld` says one was there. A field's kind is one of eight words, or the field is not kept. The pack's own scrubbing stays the first line; this is the floor under every pack.
4. **The web pack offers it from its observing tab.** `ObserveTab` implements `TrafficCapture` in the base (`start` is `record_exchanges`, `take_recorded` maps `PageExchanges`). The web probe pack declares `traffic_capture = ["web"]` and registers a source that opens an observing tab on the surface's own address, after the check every address of a discovery passes (decision 0144), with recording started before the page loads. `web_capture` reads its tab's requests through `take_capture` with the discovery's redactor, so endpoint inference consumes the contract's form and no web-pack-private path; what the check withheld is a limitation of the page.

## Alternatives

| Alternative | Why not |
| --- | --- |
| A manifest flag alone, with the probe still reading the tab's own type | The kernel would declare a capability it could not call, and nothing of the contract would stand between a pack's recording and a finding |
| A capture as a kind of discoverer, yielding findings | A discoverer's findings are the scout's evidence; a capture is what discoverers read. Endpoint inference would then parse a finding's text back into requests |
| Trusting each pack to redact, with no check in the contract | Rule 4 would rest on every pack's author. The check costs one pass over names already short, and a test shows it removes what a careless pack yields |
| A source per surface kind, each its own point | A pack holds one way to observe; the kinds it covers are data in its manifest, as a discoverer's are |

## Proof

- `a_capture_keeps_structure_and_no_secret_whatever_the_pack_yields` (`crates/a2m-provider/tests/capture.rs`): a yield with a secret in its path, query, headers, fields, field kinds, and origins keeps one request by structure; three are withheld and counted; the result is unchanged by a second pass.
- `identifiers_a_pack_failed_to_flag_are_not_kept_as_structure` (the same file): through the registry, with a redactor that knows none of them, a UUID, a 24-character token, and the order number `100245` in path segments become `{value}`, a hexadecimal token as a query's name and in a header's name is dropped with `query_left_out` and `headers_left_out`, and a map keyed by identifiers (`8812.name`) is dropped with `names_withheld`; `a_kept_word_has_few_digits_and_no_long_hexadecimal_run` holds the predicate's cases.
- `a_pack_that_declares_no_capture_is_never_asked` (the same file): of three packs, the one that declares is the only handle; one that registers without declaring is refused, `effect_not_declared`; a surface of an undeclared kind is refused before the pack is asked; the counters of the other two stay at zero.
- `a_capture_of_a_pages_own_requests_yields_structure_and_no_planted_secret` (`crates/a2m-pack-web-probe/src/tests/capture.rs`), in Chrome against the test application: a page whose script sets a cookie and fetches with a bearer token, a token in the path, a word-shaped secret in the path, and a secret in the query yields, through the registry's handle, both requests with their method, origin, path with `{value}`, the query's names, `authorization` and `cookie` named as credential headers, and the answer's names and kinds; neither planted secret is in the yield, in the `web_capture` findings over the same page, or in any log file.

## Not done

- No action handler opens a capture through the registry yet: `capture.start` and `capture.stop` (decision 0308) and a person's own use in a watching tab are still to come. The registry's handle is built and tested; the probe's discoverer uses its own tab through `take_capture`.
- The check is of form and of the redactor's known secrets. A value of a word's form that the redactor does not know is kept as structure: a short code, a key such as `A-100`, and a random token of letters with three digits or fewer. A token of 24 characters drawn from 62 holds about four digits on average, so the digit rule catches most and not all of them; the pack's own rules (decision 0195) remain the first line.
