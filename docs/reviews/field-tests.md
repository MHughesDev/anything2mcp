# Field test results

What the product showed when it was run against existing software that the project runs itself ([decision 0293](../decisions/0293-software-we-run-ourselves-is-tested-against-during-the-build-and-may-be-written-to.md)). Each system is described by its kind and never named (rule 9). How a field test is run is in [decision 0295](../decisions/0295-a-field-test-is-run-from-a-recipe-kept-outside-the-repository.md). A finding stays here until a stand holds it as a case and CI passes with it.

## Field test 1: an HTTP service with a published description, reads only (2026-11-23)

**The system.** A self-hosted application for hosting source code, started from its maker's container image, pinned by digest. It keeps its state in one embedded database file. It publishes a description of several hundred operations in the Swagger 2.0 form. It was given one invented administrator, one repository, and three issues. Its database file was read directly to confirm the three issues before the product was started.

**What was run.** `init`, `target add` with the kind `http`, `scout run` with the description as a file, `lint`, `draft delete`, and `try` on three drafted reads. Nothing was written through the product, and nothing was published. Linux only. The recipe ran three times with the same result.

**What worked.** The target was registered. A description in the OpenAPI 3.0 form was read into 20 findings and 39 drafts. One drafted read, of the system's version, ran and returned the right value. Every failure had a class, a code, and a next step.

**Findings.**

| # | What happened | Why it matters | Where it belongs | State |
| --- | --- | --- | --- | --- |
| 1 | The system's own description was refused as `description_unreadable`, because it is Swagger 2.0 and the reader takes OpenAPI 3.0 and 3.1. Nothing was drafted | Many services still publish the older form. For them the scout drafts nothing | `a2m-pack-http`, the description reader | Open |
| 2 | The scout's summary said only that the description's discoverer was not looked at. It did not give the code or the fix, which the failure does carry | A person reading the summary cannot tell what to do | `a2m-actions`, the scout report | Closed on 2026-11-23: the summary gives the code and the fix |
| 3 | With no description, the scout asked the surface's own address twice, got "not found" twice, and stopped with two findings | A service with no description gets no drafts. That is the case the capture of a person's use is for, which is not built | Roadmap, the capture packages | Open |
| 4 | Every endpoint drafted from a description handed in as a file left its `surface` as an open question, though the target declares one surface of that kind. No draft compiled. The summary said there were no open questions | A person must edit every drafted endpoint by hand before anything can be tried | `a2m-pack-http`, the drafter; `a2m-actions`, the count | Open |
| 5 | Two drafted operations were given the same tool name. One draft that does not compile stops every other draft of the target from being tried | A large description is likely to hold one bad draft, and it blocks all the good ones | `a2m-pack-http`, naming; `a2m-files`, whether a trial needs the whole target to compile | Open |
| 6 | Lint suspected a credential in a field whose name contains the letters of a secret's name: the ordering number of pinned items | False alarms teach a person to ignore the rule | `a2m-files`, the credential lint | Open |
| 7 | The two reads of an issue stopped as `drift`, `response_schema_mismatch`: the system answers `null` for an optional object, and its description does not say the object may be absent | Descriptions in the wild seldom mark this. Two of the three reads tried stopped on it. Unknown means stop (rule 7) is right; what is wrong is treating a described object's absence as unknown without asking. This needs a design decision | `a2m-pack-http`; a decision record | Closed on 2026-11-23 by [decision 0297](../decisions/0297-a-property-a-description-does-not-require-is-drafted-as-possibly-null.md); test application case 35 holds it. Both reads return the values the software's database holds |
| - | The engine refused to start under a deep folder with `socket_path_too_long` | A named stop, with a fix. Recorded because a project in a deep folder is ordinary | None; the runner takes a short home | Not a fault |

**After the first fixes, the same day.** The recipe was run again with findings 2 and 7 closed: all three reads return, and the two that had stopped return the titles the software's own database holds. The recipe has 21 steps, of which four are still marked `gap` (findings 1, 4, 5, 6).

**What this field test does not show.** A write, a sign-in, a publish, a second operating system, or any kind of system but a request service. Findings 1 and 4 were got past by hand, with a conversion of the description and an edit of each draft, so that the later steps could run; the recipe keeps both as steps.
