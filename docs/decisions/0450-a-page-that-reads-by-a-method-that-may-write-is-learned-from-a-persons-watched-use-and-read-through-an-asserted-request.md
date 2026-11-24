# 0450. A page that reads by a method that may write is learned from a person's watched use and read through an asserted request

**Status:** Accepted. Design decision, made by the design architect under [decision 0294](0294-the-owner-hands-the-open-questions-to-the-agents.md). The owner had not read this record when it was merged. Nothing in it is built.

**Date:** 2026-11-24

**Changes:** the scope of roadmap package E2 and four new rows (E2a, E7, E8, E9); the proof of FT6; spec sections [203](../spec/2-kernel/203-the-provider-contract.md), [218](../spec/2-kernel/218-discovery-and-findings.md), and [301](../spec/3-packs/301-the-web-pack.md); [SCOPE.md](../../SCOPE.md); the glossary; hypotheses H-50 to H-53. Supersedes point 1 of [decision 0308](0308-the-first-capture-records-reads-only-in-the-engines-own-browser-and-drafts-to-the-holding-folder.md) and amends its points 3 and 4. Carries out point 10 of [decision 0390](0390-the-open-questions-of-2026-11-24-are-settled-by-the-lead-and-the-first-release-is-what-progress-counts-toward.md): it is the record that says where a capture joins the provider contract.

## Context

The product's main case is a web application with a sign-in and no published description ([the Build Order](../plan/build-order.md), Stage 3). Three facts about that case were read from the code and the records on 2026-11-24. Nothing was run for this record.

1. **Every read of the web pack runs in an observing tab, and an observing tab fails every request whose method is not `GET`, `HEAD`, or `OPTIONS`** ([decision 0195](0195-a-read-never-runs-where-the-page-may-write-and-a-page-keeps-only-its-structure.md)). The browser base can let a named request through (`SafeRequests` in `crates/a2m-pack-web-browser/src/tab.rs`), and all three of its callers pass an empty list: the `ui` driver, the `page_http` driver, and the probe. No target file can name such a request. So a page that asks for its own data with `POST` is read by no mode of the web pack: a `ui` read stops as `policy`, `page_write_refused`; a scout sees the screen empty and reports the refused request as a limitation; a `page_http` read of it is not drafted. The test `a_read_never_runs_in_a_tab_that_can_write_in_any_posture` shows the stop. The next step the failure gives is to read the record another way.
2. **Asking for data with `POST` is ordinary.** A search form, a query endpoint that takes its question in a body, a procedure-call interface, and a page that posts its whole form back to draw the next page of a grid all do it. How large a share of the applications a person uses this is, nobody on this project has counted; H-50 is that count. It is large enough that the HTTP pack already carries an answer for its own mode: `read_asserted` ([decision 0249](0249-the-http-pack-reads-in-bounded-work-and-sends-nothing-that-may-write-outside-a-live-run.md), [decision 0270](0270-the-http-pack-sends-an-asserted-read-only-from-a-published-version-and-bounds-what-it-leaves-running.md)).
3. **Decision 0308 put the first capture in the observing tab.** The probe's `web_capture` already records, in that tab, every `GET` a page's own script makes. A capture there sees what a scout already sees, and for the pages of fact 1 it sees nothing. [Section 301](../spec/3-packs/301-the-web-pack.md) had the other design all along: a watching tab in a visible window, where the person acts and nothing is intercepted. It waited on a visible window, which package 2.3 has since built.

So package E2 as scoped would be built, FT6 would pass on an application chosen because its pages read with `GET`, and the main case would still be out of reach for every application that does not.

## Decision

### 1. A person's use is captured in a watching tab

A capture of a person's use runs in a **watching tab**: a tab of the engine's own browser (rule 5), in the visible window package 2.3 built, under the session the person signed in with. This supersedes point 1 of decision 0308.

- **The engine sends nothing in a watching tab.** It has no input method, starts no navigation, and makes no request of its own. It listens to what passes and reads the page's structure. Every request in the tab is the person's, so none is failed for its method: a save the person makes is the person's act ([section 218](../spec/2-kernel/218-discovery-and-findings.md)).
- **The window is held to the origins the surface declares,** as the sign-in window is ([decision 0254](0254-a-person-signs-in-in-a-window-of-the-engines-own-browser-and-the-web-pack-stops-on-every-declared-sign-of-expiry.md)). Only exchanges with the surface's own origin are recorded.
- **What is recorded of a request** is what point 2 of decision 0308 says, for every method: the method, the path by structure, the names of the query's entries, the status, the names and kinds of a structured answer's fields, and the names of the headers that carried a credential. Of a body it sends: its media type, and for a structured body the names and kinds of its fields to one level, by the rule that withholds names that look like data (decision 0195, B3). No value of a body, a query, or an answer is stored, so nothing a person types is.
- **A snapshot is taken of each screen the person visits,** by structure, as `web_snapshot` takes one, when the page's address changes and its own requests have ended or a bound of ten seconds has passed. The count of snapshots is bounded and the report says when the bound was hit.
- **The scout never opens a watching tab.** Exploring stays as it is: an observing tab, nothing pressed, every request that may write failed.

### 2. A capture is a discoverer that a person drives

A capture joins the provider contract as a discoverer, with one declaration and one signal. No new trait.

- **The manifest.** A discoverer declares `driven_by`, `engine` or `person`. Left out, it is `engine`, so no manifest in the repository changes meaning.
- **The registry** never gives a discoverer driven by a person to a scout or to `discover.run`. It is started only as a capture job.
- **The scope.** `DiscoveryScope` gains the capture's end: a signal the kernel sets when the person stops. The discoverer returns when the signal is set or a bound is hit, whichever is first, and its findings pass the same sink, the same redactor, and the same check against the manifest as any discoverer's. A discoverer driven by the engine is handed a signal that is never set.
- **The port.** A discoverer driven by a person is handed a `SandboxPort` that refuses every step. It reaches the system through nothing of the kernel's; what it watches is the pack's own (for the web pack, the watching tab).
- **Who starts it.** Anyone who may build may ask for a capture with `capture.start`, an agent included. Recording begins only when the person begins it on the engine's own page, past the presence prompt, where they are told what is kept; this is the form a watch and a sign-in already take ([decision 0283](0283-watches-reach-an-agent-through-the-registry-and-their-person-confirms-each-on-the-page.md)). The person ends it there, or by closing the window; `capture.stop` asks for the end and may be called by the one who asked. A capture that outlives its bound is ended and says so.
- **Which side of the kernel boundary.** The kernel holds the capture job's life, that only its person begins it, its bounds, and the storing of findings. The pack holds what is watched and what a finding of it is. Nothing here is the web pack's alone: a pack for another kind of system offers a capture by declaring a discoverer driven by a person.

The alternative weighed was a second trait with `start` and `stop`. It would state the discoverer's rules a second time (findings only, the manifest's kinds, the redactor, the highest level), and the two statements would drift.

### 3. What a capture is evidence of

An exchange seen in a capture is `discovered`: the system answered. An endpoint's path with a placeholder, and every draft made from a capture, is `inferred`, with the limitation `sampled`. This is section 218's rule, and it replaces the last sentence of point 4 of decision 0308, which called the capture itself `inferred`. Every draft from a capture still goes to the holding folder.

### 4. A request by a method that may write is drafted as an endpoint with a question, never as a read

This amends point 3 of decision 0308, whose rule for `GET` stands. For a request the capture saw with any other method, answered with a success and a structured body, at least twice with one shape:

- an endpoint entry is drafted into the holding folder, with an open question in place of its effect: whether every request this page sends by that method to that path only reads;
- no operation is drafted from it;
- the question quotes the method, the path by structure, how often it was seen, and the screens whose pages sent it.

A person answers by writing `effect: read` and `read_asserted: true` and releasing the draft ([decision 0298](0298-a-draft-with-an-open-question-waits-in-a-holding-folder.md)). The drafter never writes `read_asserted`, as decision 0249 says. A request seen once, or answered in two shapes, is listed as seen and not drafted, with the reason.

**Knowledge stays in one place (rule 8).** The assertion is the endpoint catalog's `read_asserted`, the key the HTTP pack already reads. The web pack reads the same entry. There is no second list of safe requests in a screen model or a setting.

### 5. An observing tab lets through the asserted reads of a published version

The web pack's drivers hand an observing tab the asserted reads of the compiled target they run, as its safe requests, under the rule the HTTP pack already keeps:

| Where the read runs | An asserted read is let through |
| --- | --- |
| A live run of a published version | Yes |
| A try or a shadow of a published version, in an environment of kind `test` | Yes |
| A try under the drafts | No. The stop is `policy`, `page_write_refused`, and its next step names the endpoint and says to publish its assertion |
| Discovery, a scout's exploring | No. Decision 0249: a discovery sends none |

- **A request matches an asserted read** by its method, the surface's origin, and the entry's path as a template, segment for segment. A longer path does not match; the base's match by prefix is narrowed to this. A method override in the request is judged as decision 0270, point 2, judges it.
- **The publish page names each asserted read** as a statement the person makes by publishing: the method, the path, and that the engine's browser will let pages send it while a tool reads. It is shown with the sign-ins, before the tools.
- **Everything else an observing tab refuses, it still refuses,** and a read whose page sends a request no entry asserts still stops as `page_write_refused`.

With this a `ui` read of such a page works as any `ui` read does: the page is opened, the page's own request passes, and the value is read from the screen.

### 6. A read binding may send a declared body

A read through `page_http` or `http` over an asserted endpoint may send a body the entry declares: a map from each declared body field to an expression over the operation's inputs, typed as an update's `body` is. Decision 0249 left this as "not yet"; a search that takes its filter in a body cannot be called without it. It is a package of its own (E8) and no part of the first capture.

### 7. One address that carries reads and writes is not asserted

An assertion speaks of every request by that method to that path. Where one address takes both a question and a change by the same method (a query endpoint, or a form posted back whole), that statement is false, and a person must not make it. The question of point 4 says so in its words. Such an application is reached through `ui` writes and the reads of its screens that need no such request, and no further, until a request can be pinned by its body: the endpoint entry names the body field that tells a read from a write and the values or digests that are reads. That is package E9, held behind H-51, because whether a pin by digest holds across an application's releases is not known.

## What this does not change

- Rule 5. The capture is in the engine's own browser; nothing is placed between another program and the network.
- A capture drafts no write (decision 0308, point 6).
- A read never runs where the page may write: the acting tab is still a write's alone. What changes is that a person may say, for one request, that it does not write, and say it where a publish shows it.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Keep decision 0308: capture in the observing tab | It records what the probe already records and nothing of the pages this record is about |
| Let an observing tab send any request to its own origin | It ends "discovery cannot write" for the web pack |
| Let through a request whose path holds a word such as "search" or "query" | A guess (rule 7), and a wrong one writes |
| A list of safe requests in the screen model or a setting | A second place for what an endpoint is (rule 8), and a second rule beside the HTTP pack's |
| The person confirms each assertion on the engine's page as the capture ends, and a draft may then be tried | Less for the person to do: they have just made the requests and know which only searched. It needs a new stored statement and a second rule beside the published version's. Kept as H-52: measure the steps from the end of a capture to the first read that returns, in FT6, and take this up if the count is the complaint |
| Reach such pages through the `http` mode by hand | It works today for a person who can read the traffic themself. It is the opposite of how little a person must do |

## Proof

- **E2a:** a stub discoverer driven by a person is refused to a scout; a capture job asked for by an agent records nothing until the prompt that stands for a person is passed; it ends at the signal and at its bound; a planted credential is in nothing stored.
- **E2:** in the test application, a page that makes its own requests, used through the watching tab by the test as a person would: held drafts for the `GET` reads seen twice; for a search behind `POST`, a held endpoint with the question and no operation; a save the test makes is stored by the application once and drafted as nothing; the application's log shows no request the engine made.
- **E7:** a screen of the test application that asks for its rows with `POST`: under the drafts the read stops and names the endpoint; from a published version that asserts it the read returns what the application's control interface holds; a second request of that page, asserted by no entry, still stops the read; the publish page shows the assertion.
- **FT6:** two applications, one of them with a screen that reads by `POST` from an address that carries no write. Reads drafted from a capture run and match the software's own database.

## Revisit if

H-50 counts few such applications among those a field test may choose from; a field test shows a person asserting a request that wrote; or the provider contract gains a posture that tells a try from a shadow (decision 0249, owed), which would let a published asserted read be tried in production.
