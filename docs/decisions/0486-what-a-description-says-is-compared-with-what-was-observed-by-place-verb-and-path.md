# 0486. What a description says is compared with what was observed, by place, verb, and path

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-25

**Builds** package E3, described against observed, which [decision 0397](0397-what-the-first-release-needs-of-the-packages-left-and-what-waits-until-after-it.md) moved after the first release and [decision 0479](0479-agents-keep-building-while-0-1-waits-and-0-1-is-released-from-its-prepared-commit.md) lets an agent build while the release waits. It **carries out** the table "Comparing what is described with what is observed" of spec section [218](../spec/2-kernel/218-discovery-and-findings.md), which named the four results and left open what is compared, by which key, and where. It **keeps** the findings, their levels, and their sources as they are: the comparison raises no finding above `discovered`, and writes no finding at all. It **keeps** the scout's compare stage of [decision 0170](0170-the-scout-identifies-each-surface-with-one-discovery-job-and-stores-each-stage.md), the drafter over a copy of the target's folder, and adds the comparison beside it.

**Changes** the provider contract, by addition (`ComparisonKey`, `KeySegment`, `KeyRule`, `Registrar::key_rule`, `Comparison`, `Compared`, `compare_findings`, `Registry::compare`); `a2m-core` (two vocabularies, `ComparisonResult` and `NotCompared`); the HTTP pack (a key rule for each of the two finding kinds it owns the form of); the scout's stage 3 in `a2m-actions`; spec sections [218](../spec/2-kernel/218-discovery-and-findings.md) and [704](../spec/7-project/704-readiness.md); the [technical reference](../reference/technical-reference.md); the designs of [`a2m-provider`](../architecture/crates/a2m-provider.md), [`a2m-pack-http`](../architecture/crates/a2m-pack-http.md), and [`a2m-actions`](../architecture/crates/a2m-actions.md); the roadmap (E3 done).

## Plan, stated before the build

The slice proves three sentences against the test application, which serves an OpenAPI description at `/api/openapi.json` and pages that ask the JSON interface for their data: an endpoint the description states and no observation saw is `inferred`; one that is seen and described is at the level the observation earned, `discovered`; and one that is seen and nowhere described is named, by its verb and its path.

The alternative weighed against the documented design: section 218 calls the comparison "a discoverer of its own". Built that way, it would read the stored findings of a target and write `matched` or `described_only` findings back. It was not taken. A finding is an observation or a claim about the system, with a subject the system has; a comparison is neither, it is a statement about two sets of findings, and writing it as one would put a second, derived copy of each endpoint in the store whose level would have to be kept in step with its parents. The comparison is a pure function over findings, run where the findings are read, and its result is kept with the stage that ran it. Section 218 is corrected to say so.

A second alternative: the kernel reads the `method` and `path` of each finding's `found` itself, since both packs' forms carry them. Not taken: the form of `found` is its owning pack's ([decision 0062](0062-words-that-arrive-with-packs-are-registered-names-and-packs-share-data-never-code.md)), and knowledge of one kind of system lives in its pack (rule 8). The kernel holds the key's shape and the matching; the pack that owns a finding kind gives the key of a finding of it.

## Decision

1. **What is compared.** Findings of a kind for which a pack registered a key rule. The side of a finding is its source, not its kind: `live_system` is the observed side; `published_description` and `artifact` are the described side; `third_party` and `reasoning` are on neither and are not compared. A finding of a kind with no key rule is not compared and not counted. In this build the HTTP pack gives keys for `described_operation` (its description import) and for `endpoint` (which its own `http_surface` makes, and which the web probe's `web_capture` and `web_use` make in the same form, for the `web` surface kind the HTTP pack owns the kind for).

2. **By which key.** A `ComparisonKey` is three parts: the **place**, the origin of the address (scheme, host, port), compared as text; the **verb**, the method in upper case; and the **path**, cut at `/` into segments, empty segments dropped, each either a literal or a placeholder (a segment that is wholly `{…}`, whatever its name). A query is not part of a key. A described key and an observed key **fit** when the place and the verb are equal, the segments are as many, and each pair of segments is two equal literals, or has a placeholder on the described side. A placeholder on the observed side fits only a placeholder on the described side: the observation generalised over values, and one literal path of a description is not shown by it. An observed key that fits several described keys belongs to those with the most literal segments, so that a described `/orders/summary` takes what was seen at `/orders/summary` before `/orders/{id}` does.

3. **What each case yields.** One entry for each described key (described findings with equal keys are one entry), and one for each observed key that belongs to none.

   | Result | When | Level | Holds |
   | --- | --- | --- | --- |
   | `matched` | A described key that an observed key belongs to | The strongest level of the observed findings that belong to it; never above `discovered` | Both sides' finding identifiers |
   | `described_only` | A described key no observed key belongs to, at a place the observation covered | `inferred` | The described findings, and the coverage of the observation of the place |
   | `observed_only` | An observed key that belongs to no described key | The observed findings' strongest level | The observed findings; its verb and path name it |
   | `unknown` | A described key at a place no observation covered (`not_observed`) or covered with coverage `unknown` (`coverage_unknown`); or a finding the rule gave no key for (`no_key`) | `inferred` | The findings, and the reason, from the closed list `NotCompared` |

   A place is covered when an observed finding there that has a key carries coverage `complete` or `partial`; its coverage is the stronger of those it carries. `described_only` is never read as unused or broken, and `observed_only` never as undocumented by intent (section 218's table, unchanged).

4. **Which public interface changes**, all by addition: in `a2m-provider`, the types `ComparisonKey` (with `ComparisonKey::parse`) and `KeySegment`; the trait `KeyRule`, one method, `key(&Finding) -> Option<ComparisonKey>`; `Registrar::key_rule(kind, rule)`, refused, `policy`/`effect_not_declared`, for a kind the manifest does not list under `finding_kinds`, and for a second rule of one kind; `Registry::compare(&[Finding]) -> Comparison`, which routes each finding to the rules of its kind and calls `compare_findings`, the pure function, which any caller with keys can use; `Comparison` and `Compared`. In `a2m-core`, the vocabularies `ComparisonResult` and `NotCompared`, held to section 218 by a test. The manifest, the `Discoverer` and `Describer` traits, `DiscoveryScope`, and the stored findings are unchanged.

5. **Where it runs.** The scout's stage 3 (compare) runs it over the findings stage 2 stored, whether or not the call is made in a project, and keeps it under `described_against_observed`: the counts by result, and each entry with its result, its level, its finding identifiers, its reason or coverage, and its verb and path under the mark of the system's words (decision 0171). The report shows it, as it shows the whole stage; its `entries` are one of the report's lists that `scout.report` cuts to their first entries and reads whole with `part` (`compared.described_against_observed.entries`), so that a large description does not make the answer large. Each entry that is not `matched` opens one question in the unknowns ledger, asked once, scoped to the entry's first finding: for `described_only`, whether the endpoint works as described; for `observed_only`, what it is; for `unknown`, that it could not be compared and why. The question names no path, so it quotes nothing the system said.

## What the build showed

- The comparison is described against the whole origin. The test application's description states its JSON interface only, so the pages a capture loads at the same origin are `observed_only` beside the requests they make. That is true (no description states them) and section 218 already says it must not be read as undocumented by intent; a description that states its own base path (a `servers` entry) could narrow the place, which is not built.
- A request a capture saw once keeps its literal path, and it fits a described template: the summary pages' totals matched `/api/orders/{order_id}/totals` at `discovered`.
- A scout of an HTTP target alone compares the description with the surface's own look at its root, which covers the origin: every described operation is `described_only`, and the root's `GET` and `OPTIONS` are `observed_only`. The first hour's scout therefore asks one question more for each, about a dozen; no test counted them.

## Choices

- **A function, not a discoverer.** Given above: a comparison is not an observation.
- **The source decides the side.** The alternative was a side named by each key rule. Not taken: the level and source rules of section 218 already say who vouches for a finding, and one finding kind may arrive from either side (an `endpoint` the HTTP pack's description reader could one day make from code is `artifact`, the same kind).
- **The observed placeholder does not fit a described literal.** The alternative was to let it, as a partial match. Not taken: it would report `matched` for a literal path no request was seen at, which claims more than was observed (section 218, rule 3).

## Proof

| Claim | Test |
| --- | --- |
| The four results, the key's fit and its most-literal preference, the side by source, the level of each result, and the reasons of `unknown` | `crates/a2m-provider/tests/compare.rs` |
| A key rule for a kind the manifest does not own, or a second for one kind, is refused; `Registry::compare` routes by kind and leaves out a kind with no rule | `crates/a2m-provider/tests/compare.rs` |
| The vocabularies are section 218's | `crates/a2m-core/tests/documents.rs` |
| Against the test application, through the product's registry: the description read by `http_description` and two summary pages and a lengths page looked at by `web_capture`; the totals the pages ask for are `matched` at `discovered`; every other described endpoint is `described_only` at `inferred`; the undescribed lengths request, seen once, is `observed_only`, named `GET /api/orders/A-100/lengths`; the pages themselves, which the description of the JSON interface does not state, are named `observed_only` too | `crates/anything2mcp/tests/described.rs` |
| A scout of the test application keeps the comparison in its stage 3 and its report, and opens a question for each entry that is not `matched` | `crates/anything2mcp/tests/described.rs` |
