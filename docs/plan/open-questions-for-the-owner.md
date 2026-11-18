# Questions that are waiting for the owner

One page of every question that only the owner can answer, as it stood on 2026-11-18. Each has a Proposed decision record with the reasoning; nothing in those records is in force. Until a question is answered the product stays as it is today, which is the "today" column. An answer can be one word.

A question is here because a rule or an owner decision stands in the way of something, never because an agent wanted permission for ordinary work ([AGENTS.md](../../AGENTS.md): tell, do not ask).

## What the product may reach

| # | Question | Today | Recommended | Record |
| --- | --- | --- | --- | --- |
| 1 | May the engine act in a browser session the person already has, through an extension they pair? May it start a browser-based desktop application with that application's own data? Does a permission the person grants the engine in system settings (Accessibility, Automation) fit rule 5? | No to all three. The macOS desktop back end cannot work without the third | No, no, **yes** | [0211](../decisions/0211-whether-the-engine-may-ever-act-in-a-session-the-person-already-has.md) |
| 2 | Are applications that offer only pixels out for good, or only for now? May the scout keep a screenshot as build-time evidence? | Out; no screenshots | Out at run time; a screenshot as build-time evidence only | [0212](../decisions/0212-whether-an-application-that-offers-only-pixels-is-ever-reached.md) |
| 3 | May the product load a target someone else wrote? May the project point to places where others publish targets? | Neither | **Yes** to loading, under the existing safety rules; no to pointing | [0213](../decisions/0213-whether-a-target-someone-else-wrote-may-be-loaded-and-found.md) |
| 4 | May a credential the machine already holds for the person be used through whatever holds it, without being read: the system's integrated sign-in; a smart card or passkey with the person present; an SSH or GPG agent; another program's store? | None of the four | Yes, yes, not in the first release, no | [0232](../decisions/0232-whether-a-credential-the-machine-holds-for-the-person-may-be-used-through-its-holder.md) |
| 5 | May a person replace, and not only add to, the name the product gives itself to a system? | Neither is built | Not in the first release; adding is built first | [0239](../decisions/0239-whether-a-person-may-replace-how-the-product-names-itself-to-a-system.md) |

## The network

| # | Question | Today | Recommended | Record |
| --- | --- | --- | --- | --- |
| 6 | Does asking many hosts, or many ports, exist at all? | Nothing looks at a network yet | No, in the first design | [0224](../decisions/0224-whether-many-host-or-many-port-looking-exists-in-the-first-design.md) |
| 7 | Is anything ever sent on a network the person has not named as theirs to look at? | Nothing is sent to an address a person did not give | No: on an unnamed network the product reads only what the machine already knows | [0225](../decisions/0225-whether-anything-is-ever-sent-on-a-network-the-person-has-not-named.md) |
| 8 | Are findings about a network kept, and for how long? A list of devices is personal data | Nothing is kept | For the run only, unless the person saved that network | [0226](../decisions/0226-whether-and-how-long-findings-about-a-network-are-kept.md) |
| 9 | Does "nothing listens for another machine" also rule out receiving a signed webhook, and reaching the engine on the person's own second machine? | Both are ruled out | Yes to both, for the first release | [0233](../decisions/0233-whether-being-told-by-another-machine-is-serving-it.md) |
| 10 | The second design round for network awareness was stopped by a safety check while it described how the product listens for announcements. Stop at the two slices that send nothing and only restrict destinations, or try the rest again in narrower pieces? | The design stands at its first round | Stop at the two slices until questions 6 to 8 are answered | [0222](../decisions/0222-the-product-is-aware-of-the-network-its-machine-is-on.md), [the plan](network-awareness.md) |

## Writes and approval

| # | Question | Today | Recommended | Record |
| --- | --- | --- | --- | --- |
| 11 | May a person give standing approval to an action that leaves nothing to read back, where its declared hazard is none? | Each such call needs a person | Yes, narrowly, after measuring how many actions this really covers | [0215](../decisions/0215-whether-a-person-may-pre-approve-an-action-that-leaves-nothing-to-read-back.md) |
| 12 | May a person's own code take part in proving a write, as a field type's way of comparing two values? | No | No, for the first release | [0238](../decisions/0238-whether-a-persons-own-code-may-take-part-in-proving-a-write.md) |
| 13 | Rule 3 was reworded on 2026-11-18 from "every write is verified" to "every write is recorded first and proven after", with the case of a write that cannot be read back stated in the rule. Was the old wording meant as a ban on such writes? | The reworded rule is in AGENTS.md | Keep the rewording | [0231](../decisions/0231-the-owners-direction-is-written-in-exact-words-of-the-agents-own.md) |

## Building and releasing

| # | Question | Today | Recommended |
| --- | --- | --- | --- |
| 14 | May the macOS runner in CI be given the accessibility permission by writing to its permission database as an administrator? Without it the macOS desktop back end can be written and never proven | Not allowed until the owner says | Yes, for the CI runner only: it is a throwaway machine, and the write is to the runner's own settings |
| 15 | The version number of the first publish | The packages say 0.0.1 | 0.1.0, when the first hour works from a published package |
| 16 | Publish to crates.io as well as pip and npm? | Undecided; it would need every `a2m-*` crate published | No |
| 17 | The publishing setup: the PyPI trusted publisher and environment, the npm organization and its nine first publishes | Not done; only the owner can | [The steps](publishing-setup.md) |
| 18 | The polkit action's identifier names the GitHub account. Keep it or change it before the first release? | `io.github.mhughesdev.anything2mcp.presence` | Keep |

## Keeping this page true

When the owner answers a question, its record is marked Accepted or Rejected with the answer, the row leaves this page, and [SCOPE.md](../../SCOPE.md) is updated in the same change. A new question is added here when its record is written.
