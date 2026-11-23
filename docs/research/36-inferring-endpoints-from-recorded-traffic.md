# Research: inferring endpoints and schemas from recorded traffic

**Status:** completed on 2026-11-23, and **Partial**. Only a few hosts resolved from the research session: GitHub raw files, GitHub pages, and web search. The docs.rs, arXiv, ar5iv, Redocly, npm, patents, and university hosts did not. Nothing was installed or run. Most tool facts below were read from a project's own README, changelog, or source file, but through a fetch tool that summarises the page; each fact is tagged:

- **[read]**: a project file was fetched and summarised in this session. Quotes are as the summary gave them.
- **[search]**: taken from a web-search summary; the page was not opened. Treat as a lead.

Feeds package E2 (capture record, stop, infer; [decision 0308](../decisions/0308-the-first-capture-records-reads-only-in-the-engines-own-browser-and-drafts-to-the-holding-folder.md)), the endpoint inference section of [`a2m-pack-web-probe`](../architecture/crates/a2m-pack-web-probe.md), and [section 401](../spec/4-building-tools/401-overlaying-tools-on-an-existing-system.md). This is brief 9 of [11-open-research.md](11-open-research.md), in part: only the inference half. The parts of the brief on anti-forgery tokens, GraphQL, WebSockets, server-rendered pages, and the terms risk of internal endpoints were not run (see "What could not be verified").

## Question

Existing open tools turn recorded traffic into an API description. How do they find path parameters, merge examples into one schema, tell an optional field from a required one, and detect pagination and sign-in? What error rates have they published? What does that say about the rules in decision 0308 and the web probe's inference table?

## Answer in short

- **Path parameters are found by format first, by variance second, by a person third.** Every tool read uses a format rule for numbers, UUIDs, hex strings and dates. The newer ones add a comparison across requests. The oldest one leaves it to the person. This matches the probe's tier one and tier two, and decision 0308 point 5.
- **Merging is by union of fields and types.** A field seen in some samples and not others is optional. A conflict of types becomes a list of types or a `oneOf`. No tool read decides on sample size.
- **Required means "present in every sample seen."** That is the rule of the schema library read and of one HAR tool's `observed` mode. That tool says in its own text that presence in every capture does not establish a contract. The other tools either never emit `required` or mark query parameters and headers optional by fixed rule.
- **Pagination and auth are thin.** One tool (traffic2openapi) names query-parameter pagination and header-based auth schemes. The changelogs of another tool list neither. Nothing read detects pagination from the response.
- **Published error rates are rare and are for other methods.** The one traffic-based figure found is for a research tool that drives a user interface, from a search summary only: 98% precision and 56% recall of endpoints on seven applications. No error rate was found for any HAR-to-OpenAPI tool. Their READMEs give none.
- **The one tool that builds a call graph from a HAR file uses a model.** It breaks rule 1 for a run and cannot be copied. What it shows is useful: a value that an action sends is found in the answer to an earlier request.

## What was read

| Source | Reached | How |
| --- | --- | --- |
| mitmproxy2swagger README, main script, schema helper | Yes | Raw files in its GitHub repository |
| mitm2openapi (Rust rewrite) README and changelog | Yes | Raw files in its GitHub repository |
| har-to-openapi README | Yes | Raw file in its GitHub repository |
| traffic2openapi README | Yes | Raw file in its GitHub repository |
| demystify README | Yes | Its GitHub page |
| Integuru README | Yes | Raw file in its GitHub repository |
| GenSON README | Yes | Raw file in its GitHub repository |
| Optic README | Yes | Raw file; it gives an overview and no method |
| Redocly `generate-spec`, openapi-devtools, Akita, Postman | No | Hosts did not resolve, or the file was not found. Only search summaries |
| ApiCarv paper, D2Spec paper, Respector paper | No | Hosts did not resolve. Only search summaries |
| Web searches | Yes | Technical terms only. Nothing from this repository was sent |

## 1. Path parameters

| Tool | How it decides a segment is a parameter | Source |
| --- | --- | --- |
| mitmproxy2swagger | A segment is a parameter when it matches a regular expression that the person can set with `--param-regex`. The default is `[0-9]+`, anchored to the whole segment. Candidates are written as `ignore:` lines that a person edits. The script numbers placeholders `{id}`, `{id1}`, `{id2}`. A parameter's type is `number` if its segment is all digits, else `string` | [read] README, main script, schema helper |
| mitm2openapi | Recognises UUIDs, hex, base58 and uppercase slugs, and "values that vary across requests". The changelog dates the variance rule and the extra formats to version 0.6.0, and says `--param-regex` was removed in 0.4.0 and returned in 0.6.0. Identifiers are validated since 0.2.0 | [read] README, changelog |
| har-to-openapi | Turns a UUID segment into a placeholder. A numeric segment becomes a parameter only when it is at least `minLengthForNumericPath` long. A path parameter is always required | [read] README |
| traffic2openapi | Numeric ids, UUIDs, hashes, and dates are replaced. Names come from context (`/users/123` gives `{userId}`) by about 180 resource patterns | [read] README |
| demystify | Says it "identifies path parameters automatically". It gives no method | [read] README |
| A patented tree method (a vendor's patent) | Builds a tree of observed segments. When a node's children vary too much, the siblings are compacted into one catch-all node. A decay-based confidence is kept | [search] patent summary; the patent text was not opened |

What this shows:

- **Format rules are universal, and they differ at the edge.** A short number (`/page/2`) is the common false positive. One tool needs a minimum length for numbers. That is a named, tunable guard, and the probe's tier one has none stated.
- **Variance across requests is the second tier in every tool that has one.** It is the probe's tier two, and what decision 0308 point 5 requires: two requests that differ in one part. The patent method adds a count: a node is compacted only when it has many different children. Two requests are the smallest possible evidence. None of the sources gives a count that is safe.
- **A wrong template is worse than a missing one.** A segment such as `/reports/summary` and `/reports/daily` can both be words, not values. The tools that need a person's edit (mitmproxy2swagger) do not guess. The probe's tier two rests on "answers whose tops hold the same names", which is a check the format tools do not make, and it is stricter.
- **The type of a parameter is read from one value.** mitmproxy2swagger types a path parameter by whether its segment is all digits, and a query parameter by its first value [read: schema helper]. One value is weak. The probe keeps a count of different values, by digest, which is better evidence.

## 2. Merging examples into a schema

- **Union.** GenSON merges two objects into one schema with the union of their properties. Conflicting types become a type list (`["integer", "string"]`). It uses `anyOf` only for kinds that differ in structure, such as an object and an array [read: README].
- **Per status code.** mitm2openapi has merged response schemas per status code since 0.4.0, unions array element schemas since 0.4.1, and merges request body schemas across several captures since 0.6.0, with `oneOf` [read: changelog].
- **A dictionary is not an object.** mitmproxy2swagger's helper turns an object into `additionalProperties` when all its keys are UUIDs or numeric strings, so that the keys are not read as field names [read: schema helper]. mitm2openapi "tightened the dictionary heuristic" in 0.4.1 [read: changelog]. The probe already withholds names that read as keys (more than 64, or not a plain name; [decision 0195](../decisions/0195-a-read-never-runs-where-the-page-may-write-and-a-page-keeps-only-its-structure.md)). The tools use a smaller test (all keys are ids). Neither is known to be complete.
- **Envelopes.** mitm2openapi 0.7.0 splits answers wrapped in a success flag into a success schema and a shared error schema, and merges every error body, not only the first [read: changelog]. A reported fix says "one outlier no longer overrides the rest" [search].
- **Examples are a leak.** mitmproxy2swagger warns that `--examples` and `--headers` "may expose sensitive information" [read: README]. har-to-openapi redacts by name and replaces numbers with `0` and booleans with `false`, and keeps at most 5 examples of at most 16,384 bytes when asked [read: README]. The probe keeps no values at all, which is the stricter choice (decision 0308 point 2).
- **Formats.** traffic2openapi detects the formats email, uuid, date-time, uri, and the two address kinds, and does not say how conflicting types are resolved [read: README].

## 3. Required and optional

| Tool | Rule | Source |
| --- | --- | --- |
| GenSON | `required` is "the keys that have appeared in every object it has seen". When none remain, the list is dropped | [read] README |
| har-to-openapi | Three policies. `legacy` (default) keeps inferred body properties optional and generated request bodies required. `optional` makes bodies and properties optional. `observed` marks a field required only when it appears in every relevant sample. An option adds `presentCount` and `presenceRatio` to the output. The README warns that presence in every capture "doesn't establish an API contract" | [read] README |
| mitmproxy2swagger | Path parameters required; query parameters and headers never. A body schema never gets a `required` list | [read] schema helper |
| traffic2openapi | Tracked by appearance across requests. No rule or threshold is stated | [read] README |
| mitm2openapi | Nothing in the changelog | [read] changelog |

What this shows:

- **The only rule in use is "in every sample".** It is sound as an upper bound: a field missing from one answer is surely optional. It is not sound as a lower bound. A field in two of two answers is called required on very little evidence. The one tool with an `observed` policy makes it opt-in and prints the counts beside it.
- **The probe's rule is the safe half.** The design says a field seen in some answers and not others is optional, with limitation `sampled`. It does not yet say what a field seen in all of them is. The tools suggest it should be "seen in all, n times", with n kept, and never a claim of required.
- **A tool that only reads has less at stake.** The first capture drafts reads only (decision 0308 point 3). For an output, a wrong `required` makes a checker too strict and stops a run with a named failure; it does not write anything. For an input (a parameter of a read), a wrong `required` makes an agent send an argument it need not.

## 4. Pagination and sign-in

- **Pagination.** traffic2openapi "identifies page/limit/offset/cursor patterns", from query parameters only, and tracks minimum and maximum values [read: README]. It states no response-side signal (a `Link` header, a `next` field, a `total`). No other tool read mentions pagination, and the changelog of one lists none [read]. The honest reading is that pagination is detected by parameter name, and that no source shows how often that is right.
- **Sign-in.** traffic2openapi recognises bearer tokens (with a check for a token in JWT form), basic auth, and key headers such as `X-API-Key`, and writes `securitySchemes`. It states that the list is not complete [read: README]. mitmproxy2swagger does not single out sign-in headers, and does not say whether it removes them [read: main script]. This is the area where the product's rule 4 matters: a tool that writes a header into a description has already held the value.
- **Dependency between requests.** Integuru, from a HAR file and a cookie file, finds the request that does an action, finds its dynamic parts (such as an account id), finds the earlier requests whose answers carry them, and repeats until every request depends only on another request or on the sign-in cookies [read: README]. It does this with a language model, and sends prompts and requests to a cloud model. It stops at 20 steps by default. It is version 0, and its input variables work only in graph generation [read]. The method is the one the probe's table calls "dependencies, not built". The model is not usable under rule 1, but the finding is: the link is a value in one request equal to a value in an earlier answer, and that comparison needs no model.

## 5. Error rates, where published

| Figure | What it measures | Source and trust |
| --- | --- | --- |
| 98% precision, 56% recall of endpoints, on seven open-source web applications | ApiCarv, a research tool that carries a user-interface test run and infers endpoints and path parameters from the traffic, then probes for more | [search] from a summary of the ICSE 2023 paper; the paper was not opened. Not a HAR tool, and it sends requests of its own |
| 81.3% precision and 80.6% recall of path templates (1,930 of 2,394) | D2Spec, which reads HTML documentation, not traffic | [search]. Not relevant to traffic; given only so it is not mistaken for it |
| 100% precision and 98.6% recall of methods; 50% recall of parameter constraints | Respector, from Java source code, not traffic. A later study reports errors in its ground truth | [search]. Not relevant to traffic |
| None | mitmproxy2swagger, mitm2openapi, har-to-openapi, traffic2openapi, demystify, Integuru | [read] none of their READMEs states an error rate. The demystify README calls its output "accurate" and gives no method |

The honest answer to "their error rates where published" is that the HAR tools publish none. The one traffic-based figure, if it holds, says precision is high and recall is low: what the tool reports is mostly right, and a lot is missed. That agrees with decision 0308's choice to draft only what was seen twice, and to list the rest as "seen and not drafted".

## What this means for the design

Reading the design against the sources:

1. **Decision 0308 point 3 is stricter than any tool read.** The tools draft a path after one request. The decision needs two requests with one shape. This is the safer rule, and the one figure found (high precision, low recall) points the same way.
2. **Decision 0308 point 5 matches the common second tier.** Two requests that differ in one part is the weakest form of the variance rule that mitm2openapi has used since 0.6.0. The probe adds a check that the answers have the same field names, and the tools do not.
3. **Three things in the probe's table have no stated rule, and the tools show a need for each:** what a field seen in every sample is; what a short number in a path is; and whether a header that carries a sign-in is ever written into a finding.
4. **A dependency finding can be built without a model,** by comparing digests of values, as the probe's table already says. Integuru shows the need and the cost of doing it with a model.

## What this changes in the design

All of these are proposals. The lead applies them.

1. **Proposal, [a2m-pack-web-probe](../architecture/crates/a2m-pack-web-probe.md), "Endpoint inference", the "Shapes" row.** Replace "a field seen in some answers and not others is optional with limitation `sampled`" with: "A field seen in some answers and not others is optional, with limitation `sampled`. A field seen in every answer is recorded as `seen_in_all` with the number of answers; it is never recorded as required, because presence in every sample is not evidence of a contract. The count of answers is kept."
2. **Proposal, same row, "Template, tier one".** Add: "A run of digits is a parameter only when it is at least four characters long, or when tier two finds it varying. A shorter number stays literal and is listed as a limitation." The four is a starting value for the first test to tune, not a measured figure.
3. **Proposal, same section, "Template, tier two".** Add: "A segment that varied is a parameter only when its different values are not all words of a plain name list that the endpoint's other paths also use as literal siblings. Two values is the least evidence; the finding says how many were seen." The test: two sibling paths such as a `summary` and a `daily` that are both literal words in other captures stay literal.
4. **Proposal, decision 0308, add point 7.** "A parameter's type is taken from every value seen, by the digest comparison, and never from the first. A parameter seen with one value is typed `text` and says so. No `required` is inferred from a capture; the held draft asks the person which of its parameters are required." This is a decision for the lead to word; the evidence is that mitmproxy2swagger types from the first value and the tools that infer `required` say it is not a contract.
5. **Proposal, decision 0308, add to the held draft's question (point 4).** "Which of the drafted parameters may be left out, and does the list that this read returns have more pages?" Pagination was found by name only, in one tool, with no published accuracy, so it is asked and never assumed.
6. **Proposal, spec [section 401](../spec/4-building-tools/401-overlaying-tools-on-an-existing-system.md), capture.** Add: "Inference from a capture is judged by what it misses as well as by what it gets wrong. The proof of package E2 states both: the reads drafted that are not reads the software means to be called, and the reads in the test application's pages that were not drafted." Reason: the only published figure is high precision and low recall.
7. **Proposal, [a2m-pack-web-probe](../architecture/crates/a2m-pack-web-probe.md), the dictionary rule.** Add to "What a finding keeps": "An object whose keys are all numbers or identifiers by format is a dictionary, kept as one entry of type `map`, whatever its size." The tools use this smaller test, and the probe's 64-key test would otherwise name the first sixty-four ids of a small list.
8. **Proposal, no change but a record.** The product does not use a model to find dependencies between requests (rule 1). Record in the "Dependencies" row that the method is the digest comparison and that an equal digest of one value in two answers is the only evidence it takes.

## What could not be verified

- **Every claim marked [search].** The ApiCarv, D2Spec, and Respector figures, the patent method, the Redocly and openapi-devtools behaviour, and the mitm2openapi error-body fix come from search summaries.
- **Which of the tools' README claims are true when run.** Nothing was installed. "Identifies path parameters automatically", "180+ resource patterns", and "safely merged" are the projects' own words.
- **Exactly how traffic2openapi decides required and conflicts.** Its README states none. Its source was not read.
- **Akita and Postman's traffic inference, Optic's capture method, and Redocly's `generate-spec`.** Not reached or no method on the page.
- **GraphQL, WebSockets, anti-forgery tokens, server-rendered pages, schema validation of undocumented answers, the stability and terms risk of calling internal endpoints, and capture through the DevTools protocol.** These parts of brief 9 were not run. The brief should stay open for them, with the inference half marked answered here.
- **Any measured error rate for a HAR tool.** Found none. A first measurement would come from package E2's own proof against the test application, where the true endpoints are known.

## Sources

Read (GitHub repository files, 2026-11-23):
- [mitmproxy2swagger](https://github.com/alufers/mitmproxy2swagger): README, `mitmproxy2swagger.py`, `swagger_util.py`
- [mitm2openapi](https://github.com/Arkptz/mitm2openapi): README, CHANGELOG
- [har-to-openapi](https://github.com/jonluca/har-to-openapi): README
- [traffic2openapi](https://github.com/grokify/traffic2openapi): README
- [demystify](https://github.com/tfmorris/demystify): README
- [Integuru](https://github.com/Integuru-AI/Integuru): README
- [GenSON](https://github.com/wolverdude/GenSON): README
- [Optic](https://github.com/opticdev/optic): README (overview only)

Known only from search results:
- [Carving UI Tests to Generate API Tests (ApiCarv), arXiv 2305.14692](https://arxiv.org/abs/2305.14692)
- [Automatically Extracting Web API Specifications from HTML Documentation, arXiv 1801.08928](https://arxiv.org/abs/1801.08928)
- [Generating REST API Specifications through Static Analysis (Respector), ICSE 2024](https://conf.researchr.org/details/icse-2024/icse-2024-research-track/137/Generating-REST-API-Specifications-through-Static-Analysis)
- [Generating Accurate OpenAPI Descriptions from Java Source Code, arXiv 2410.23873](https://arxiv.org/abs/2410.23873)
- [Rest application programming interface route modeling, US 11586487](https://patents.google.com/patent/US11586487)
- [Redocly `generate-spec`](https://redocly.com/docs/cli/commands/generate-spec)
