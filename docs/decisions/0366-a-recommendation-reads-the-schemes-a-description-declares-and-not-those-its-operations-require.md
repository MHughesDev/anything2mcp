# 0366. A recommendation reads the schemes a description declares, and not those its operations require

**Status:** Accepted. Design decision, made under [decision 0294](0294-the-owner-hands-the-open-questions-to-the-agents.md), in package 2.7, from a finding of the [sessions-and-secrets review](../reviews/sessions-and-secrets-review-2026-11-24.md) (finding 5). The owner had not read this record when it was merged. The number 0399 is a placeholder the lead renumbers on merge.

**Date:** 2026-11-24

**Changes:** nothing is rewritten. Adds a limit to [decision 0367 on the rules](0367-auth-detect-recommends-a-mode-and-a-provider-by-fixed-rules-and-marks-it-inferred.md), point 5.

## Context

An OpenAPI description declares its security schemes in one place (`components.securitySchemes`, or `securityDefinitions` in Swagger 2) and says which operations take which in another (`security`, on the description or an operation). The HTTP pack already reads the second for a different purpose, whether an operation may be called with no credential. The first reading of the schemes took the first place only, so a description that declares a scheme and requires it of no operation, or of an operation the target will never use, still recommends its provider.

## Decision

1. **A recommendation lists every scheme a description declares, and does not say which an operation requires.** The sign of each is a choice for a person to confirm, in the order of the table, with the first named in `mode` and `provider` for a person who wants one answer. The finding says it was read from a description, at the level `inferred`, and carries the limitation that nothing was sent with it.
2. **This is a weakness, taken on knowingly, and why.** A wrong recommendation costs a person one look: the choices are shown with their reasons and nothing is drafted or stored from them. Reading `security` as well would tie the recommendation to which operations the target keeps, which is known only after a scout drafts them, and `auth.detect` runs before that. The drafter, which does know the operations, is the place that can choose among the choices by what each operation requires; it does not yet.
3. **Nothing of a scheme's own text is kept.** A scheme's name, the name of its header or query field, its realm, and its URLs are not in the finding or in the recommendation. A sign is one of a closed list of names, so a description cannot put its own words into an answer an agent reads (rule 4 and spec section 502). The product's test asks the answer for the header name the stand's description declares and finds none.

## Alternatives weighed

| Alternative | Why not |
| --- | --- |
| Keep only the schemes some operation requires | Needs the operations; see point 2 |
| Keep the scheme's own name, so the person sees which is which | The name is text the system supplied; it would reach an agent in a result, and it is not needed to choose a provider |

## What would reopen it

A target whose description declares many schemes and requires one, where the choices are noise a person cannot sort. Then the drafter should read `security` and name the scheme each operation takes.
