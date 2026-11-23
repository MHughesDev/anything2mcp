# Research: locator drift and repair

**Status:** **Partial**, written on 2026-11-23. Brief 20 of [11-open-research.md](11-open-research.md). Most hosts for the papers did not resolve in the research session (see "How this was read"). The figures from papers come from search-result summaries of the papers, not from the papers. Pages that did load are the Playwright guide and best-practices file, the Healenium repository and its README, and the ReproBreak repository README. Nothing was installed or run. Each claim is tagged **read** (the page was opened) or **search** (a search summary only).

## Question

How often do UI locators break across releases of an application? Which locator strategies survive best? How do existing open tools detect drift and propose a repair without a model at run time (rule 1)? What follows for package 9.1 (canaries, drift detection, the repair loop)?

## Answer in short

- **Locators are the main thing that breaks.** Two studies of test suites run against successive releases of web applications put locator breakage at 70% or more of all breakage: over 70% (record/replay tests, Hammoudi et al., ICST 2016) and 74% (2,672 tests over 86 releases of four applications, VISTA, FSE 2018). **Search** only. The subjects were older Selenium-style suites, mostly of small or student-built applications, so the rate for a modern application is not known.
- **Attribute locators broke most.** The 2016 study reports over 50% of all breakages came from attribute-based locators (ids, names, classes). **Search** only. Robula+, an algorithm that picks robust XPath, cut locator fragility by 90% against absolute XPath and 63% against the locators a recorder writes (Leotta et al., 2016). **Search** only. So how a locator is written changes how long it lives by a large factor.
- **No study found compares role-and-name against test ids, text, or structure on breakage rates.** The newest dataset (ReproBreak, 2026) holds 9,572 locator changes from 359 Playwright and Cypress repositories and 449 breaks that can be re-run, but its README gives no count by locator type. **Read.** The ranking below rests on the tools' own guidance and on reasoning, not on a measurement. The product should measure it (proposal 6).
- **Every mature open tool repairs by similarity and none is safe on a write path by itself.** Healenium keeps the last working locator and its page neighbourhood, and on a failed lookup picks a candidate with a score above a threshold. Research tools (WATER, VISTA, Similo) score candidates over many element properties. The documented failure is a repair that lets the test pass on the wrong element. **Search** for the failure; **read** for Healenium's threshold setting.
- **The product's design already holds the safe half.** It never heals at run time, it fails on an ambiguous match, and it has a fingerprint on every field and action. What it lacks is a deterministic way to propose the repair at authoring time, a drift signal that appears before a hard failure, and a record of how each repair candidate was scored. Proposals are at the end.

## How this was read

| Source | Reached | How |
| --- | --- | --- |
| Playwright locator guide and best-practices file on `raw.githubusercontent.com` | Yes | Opened; read through a small summarizing model |
| Healenium web library repository page and README | Yes | Opened; the README does not describe the algorithm |
| ReproBreak repository page | Yes | README only; no per-type counts in it |
| arxiv.org, people.ece.ubc.ca, tsigalko18.github.io, sepl.dibris.unige.it, playwright.dev, blog.acolyer.org, alphaxiv.org, emergentmind.com | **No** | Hosts did not resolve in this session; they need allowing in the network settings before the papers can be read |
| Web searches | Yes | Used to find the papers and tools. Every figure taken from a search summary is marked **search** |

Searches used generic technical terms only. Nothing from this repository was sent.

## Part A: how often locators break

| Finding | Source | Tag |
| --- | --- | --- |
| Locators are the main cause of fragility: 74% of all breakages, over 2,672 test cases and 86 releases of four web applications. The paper's visual repair fixed about 81% of breakages | [VISTA, FSE 2018](https://people.ece.ubc.ca/amesbah/resources/papers/fse18-vista.pdf) | search |
| Over 70% of breakages in record/replay tests come from locator fragility, over 50% from attribute-based locators. Repair applied after every fine-grained version works better than after a big jump (the WATERFALL result) | [Hammoudi et al., ICST 2016](https://tsigalko18.github.io/assets/pdf/2016-Hammoudi-FSE.pdf) (the URL carries the paper's own file name) | search |
| A model-based repair reports an 84% average repair rate over six applications and handles added and removed pages | Semter, FSE 2023, via the [search listing](https://unpaywall.org/10.1145%2F3611643.3616324) | search |
| Robula+ lowers locator fragility 90% against absolute XPath and 63% against recorder locators | [Leotta et al., 2016](https://sepl.dibris.unige.it/publications/2016-leotta-JSEP.pdf) | search |
| 13 empirical studies of test repair exist as of the 2019 review | [Systematic review](https://arxiv.org/pdf/1909.10750) | search |
| 9,572 locator changes were mined from 359 Playwright and Cypress repositories; 449 breaks were reproduced in Docker | [ReproBreak](https://github.com/rub-sq/ReproBreak) | read |
| Playwright says CSS and XPath "can be tied to the DOM structure", that long chains and position picks are bad practice, and that a position pick may click an element you did not intend | [Playwright locators](https://raw.githubusercontent.com/microsoft/playwright/main/docs/src/locators.md) | read |

How far to trust these. The studies compare different benchmarks, so the numbers cannot be put side by side. They measured locators written as ids, names, and XPath, not role-and-name. They measured tests that failed, not the harder case for this product: a locator that still matches, on the wrong element. No number found says how often a modern application changes its accessible names or its test ids in a release. The platform profile's `drift_risk` ([section 301](../spec/3-packs/301-the-web-pack.md)) is a judgment without data behind it.

## Part B: which locator strategies should survive

No measurement settles this. What the sources say, and what follows from it:

| Strategy | What the sources say | Likely failure |
| --- | --- | --- |
| `{role, name}` | Playwright lists it first: it "reflects how users and assistive technology perceive the page", and role locators should carry the name ([read](https://raw.githubusercontent.com/microsoft/playwright/main/docs/src/locators.md)). The product's own run found every field of its test application this way (H-02, one small application) | A reworded label or a changed role. Both are visible to a person, so a release note or a diff catches them |
| `{label}`, `{placeholder}`, `{title}` | Same list, below role | A reworded label. A label shared by two controls is `locator_ambiguous` |
| `{text}` | Same list | A reworded string; a language or an A/B variant changes it |
| `{test_id}` | Playwright: "the most resilient way of testing", yet "not user facing" ([read](https://raw.githubusercontent.com/microsoft/playwright/main/docs/src/locators.md)). Testing Library's order is not read here (that page returned 404), so no claim is made about it | Survives a restyle and a reword. Disappears only when the team removes it, and a team that never added one gives none |
| `{css}` and structure | Playwright calls both fragile and gives a class-chain example that "breaks if a designer changes the class names" ([read](https://raw.githubusercontent.com/microsoft/playwright/main/docs/src/best-practices-js.md)). The 2016 study ties most breakage to attribute locators ([search](https://tsigalko18.github.io/assets/pdf/2016-Hammoudi-FSE.pdf)) | Any markup change. Already allowed only last and flagged |
| Generated ids | Rejected by the platform profile | Change on every build or load |

Two observations that follow. The strategies fail on different releases: a reword breaks role, label, and text together, a restyle breaks css and class only, and a framework upgrade breaks generated ids. So an ordered list that mixes a content locator and a contract locator (a test id) survives more kinds of release than a list of content locators alone. That is a design claim, not a measured one. And a locator is only as good as the fingerprint that guards it, because the dangerous break is the match that moves to another element, not the miss.

## Part C: how open tools detect drift and propose repair

| Tool | How it finds a replacement | Safety it states | Tag |
| --- | --- | --- | --- |
| [Healenium](https://github.com/healenium/healenium-web) (Apache-2.0, 202 stars, 447 commits as rendered) | Wraps the driver. On a failed lookup it compares the page with the page stored at the last working lookup, by a tree-comparison algorithm described by third parties as a weighted longest common subsequence. The proxy form stores reference selectors, healing records, and reports in a database | A `score-cap` threshold (its README example is 0.6), `recovery-tries` for the number of candidates, `heal-enabled` and `@DisableHealing` to turn healing off, a report plugin, and an IDE plugin that writes the healed locator back into the test code. The README does not say healing needs approval | Repository and README **read**; algorithm **search** (third-party blogs; the project's own explanation page was not reached) |
| WATER | Looks for an identical element by five attributes (id, XPath, class, link text, name), then for similar nodes by other attributes | A 2023 paper says its fixed attribute order is a heuristic that can fail | **search** |
| VISTA | Matches the old and new page by appearance | Reported as about 81% of breakages repaired | **search** |
| Similo | One weighted score over 14 element properties for each candidate, so a property that no longer identifies the element uniquely still helps. It failed to find the target in 72 of 598 cases, against 146 for the multi-locator baseline | Weights were set from experience or learned; the follow-up replication tries other similarity functions | **search** ([paper](https://arxiv.org/pdf/2208.00677)) |
| Multi-locator (Leotta et al.) | Five single locators that each pick one element; vote | Needs agreement | **search** |
| Robula+ | Generates a robust XPath at record time, by attribute priority | Reduces breakage; does not repair | **search** |

What these have in common, all deterministic: a stored reference of the element at the last good time; a candidate set; a weighted similarity score over named properties; a threshold. None of them needs a model. Some newer products add a model as a second step; the sources found for that are marketing pages and blogs, and none was used here.

### The failure mode: healing the wrong element

Practitioner writing and one 2026 case study agree on the risk, and call it a false heal: a repair that lets the test run but no longer checks the intended behaviour. Examples given: several "Delete" buttons and the engine picks another; a "Cancel" button chosen for "Submit"; an agent that removed a failing test from the suite. They recommend a human approve repairs on release-critical paths, financial actions, and account changes, and treat the healer as a candidate generator that prepares a reviewable patch. A simulation reporting a fourfold rise in lost coverage is from a vendor-style piece and is not relied on. All **search**: [SD Times](https://sdtimes.com/software-testing/the-hidden-risk-in-self-healing-test-automation-a-governance-blueprint-for-digital-banking/), [devops.com](https://devops.com/why-self-healing-tests-need-a-deployment-gate/), [a case study of autonomous test repair](https://arxiv.org/pdf/2605.01471).

For this product the point is sharper than for a test suite. A test that heals onto the wrong element gives a false pass. A tool that does, on a write path, writes to the wrong record, and rule 3 then reads back the wrong thing as if it were the right one. The read-back checks the value in the element that was found, not that it was the right element. So the product's refusal to heal at run time, and its fingerprint, are not over-caution. The sources do not contradict them.

## Part D: what a deterministic repair proposer can look like here

This is a design sketch from the sources, not a tested design.

1. **A reference is stored at the last verified run.** The screen model's field or action keeps, beside its locators, a small record of the matched element: role, accessible name, label text, test id, tag, the section it sat in, near text, the sibling order among elements of its role, and its position in the accessibility tree. This is data in the screen model, not a locator. The trace record already keeps which locator matched and the fingerprint score ([section 217](../spec/2-kernel/217-traces-evidence-and-repair.md)).
2. **On drift, a candidate set is built from the failure snapshot.** Every element in the fingerprint's section, or in the whole snapshot if the section is gone, is scored against the reference by a weighted sum over those properties, with fixed weights written down in the platform profile. The output is a ranked list with the score of each property, not one answer.
3. **The proposer emits a patch, not a repair.** It proposes a new locator for the top candidate by writing the most specific role-and-name, label, or test-id form that matches that one element in the failure snapshot, and checks that the new locator matches exactly one element there and passes the old fingerprint or a proposed new one. If the margin between the top two candidates is under a set value, it proposes nothing and says `locator_ambiguous`.
4. **The patch goes through the existing loop:** publish gates against the test stand, then a person approves the diff ([section 217](../spec/2-kernel/217-traces-evidence-and-repair.md)). An agent may still write the patch from the bundle; the proposer is the deterministic floor for when it does not.
5. **Writes get a stricter bar than reads.** A candidate for a field or action on a write path needs a fingerprint pass and a person's approval with the candidate's scores shown. No score alone applies a repair.

Alternative weighed: heal at run time when the score is high, as Healenium does. Rejected. It fails rule 1 in spirit (the run improvises), rule 7 (unknown means stop), and the sources show the failure mode above. Alternative weighed: leave repair wholly to an authoring agent with the fixed bundle. Kept as the path, but a deterministic proposer is cheap to test and gives the agent and the person a baseline to compare against.

### Drift signals before a hard failure

The step trace already records which locator in the ordered list matched and at what position, and the fingerprint score. Three early signals follow from fields that exist:

- **A locator slid down its list.** The first locator no longer matches and the second does. The tool still works; the canary should report it as `drifting` with the position. This is the cheapest signal and needs no new capture.
- **A fingerprint score fell.** The element is found but the near text or section matches less.
- **A set of screens changed together.** Many screens of one surface fail in one release: a framework change, not a reword.

## What could not be verified

- The numbers in Part A. Each is from a search summary, and the papers' hosts did not load. Re-read them when the hosts are allowed.
- Healenium's algorithm and threshold meaning. Its README gives the settings only; "weighted longest common subsequence" is from third-party blogs.
- Any breakage rate by locator type. ReproBreak may answer it (its `locator_analysis.csv` and `categorize_locators.py` were not opened).
- Testing Library's query order (the page returned 404) and any browser vendor's selector-priority list.
- How often a modern application changes accessible names or test ids per release. No source found.
- Commercial self-healing products and their model steps. Only marketing and blog text was seen.
- Maintenance state of any tool beyond the repository counts as rendered on 2026-11-23.

## What this changes in the design

All are **proposals**. The lead applies them.

1. **Section 217, "Drift and the repair loop": add after step 2's bundle list.** "The bundle also holds a ranked list of candidate elements from the failure snapshot, each with its score by property, made by a deterministic proposer under fixed weights from the platform profile. The proposer proposes nothing, and the failure stays `locator_ambiguous`, when the top two candidates are closer than a set margin. No score applies a repair; a person approves the diff."
2. **Section 217, same section: add.** "A repair never applies at run time, whatever the score. A false repair, a locator that matches the wrong element and passes, is the failure the loop exists to prevent. The fingerprint, the exact-one-match rule, and the approval of the diff are what hold it."
3. **Section 217, canary text and the metrics list: add a `drifting` state.** "A canary reports `drifting`, not `drift`, when a field or action still passes but its matched locator is not the first in its list, or its fingerprint score is below the score it had at the last verified run. A `drifting` tool stays available and is shown in the drift metrics; it does not stop a run."
4. **Section 301, "Locating by role and name": add.** "A list of locators should include one that does not depend on the element's words, a `test_id` where the application has stable ones, because the content locators (role and name, label, text) fail together on a reword. The linter warns when every locator in a list depends on the same visible string."
5. **File formats, the screen model's field and action: add an optional `reference`** (proposed name) "that records, from the last verified run, the matched element's role, name, label, test id, section, near text, and order among its siblings. It is data for the repair proposer; the driver never reads it to find an element." A decision record is needed, because it changes a file format.
6. **Hypotheses register: add H-02a** (proposed). "In a set of releases of the test application changed on purpose in the ways named in Part B (reword, restyle, reorder, markup wrapper, framework upgrade), a list that holds a role-and-name locator and a test-id locator survives more releases than either alone; and the proposer's top candidate is the right element in at least 9 of 10 cases and says nothing in the rest." Measure with the test stand and roadmap package 9.1's changed-screen proof; record per locator type which one broke. This also gives the number the sources do not.
7. **Roadmap package 9.1: add to the proof.** "A screen changed in each way of H-02a is caught by a canary as `drifting` or `drift`, repaired by a reviewed patch, and no write reaches a wrong element in any case, including a case with two equally good candidates."
8. **Section 301 platform profile, `drift_risk`: add.** "`drift_risk` is a judgment until measured. When package 9.1 has the numbers from H-02a, the profile's values and the canary interval are set from them, with a decision record."
