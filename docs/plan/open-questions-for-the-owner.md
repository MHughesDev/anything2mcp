# Questions that are waiting for the owner

One page of every question that only the owner can answer. Each has a decision record with the reasoning. A question is here because a rule or an owner decision stands in the way of something, or because only the owner's own hands can do it, never because an agent wanted permission for ordinary work ([AGENTS.md](../../AGENTS.md): tell, do not ask).

On 2026-11-18 the owner took the recommended answer to all eighteen questions then on this page. The answers are in [decision 0242](../decisions/0242-the-owner-takes-the-recommended-answer-to-every-open-question.md), and the eleven question records are marked Accepted. What is left below is what the answers did not cover.

## Only the owner can do

| Item | State | Where |
| --- | --- | --- |
| The publishing setup: the PyPI trusted publisher and its environment, and the npm organization with its first publishes | Not done. No agent handles a registry token ([decision 0242](../decisions/0242-the-owner-takes-the-recommended-answer-to-every-open-question.md), point 17) | [The steps](publishing-setup.md) |

## Choices the agents have made and the owner may change

These are not blocked on an answer. They are listed because the owner may want a different one.

| Question | Today | Recommended |
| --- | --- | --- |
| Should the documentation audit also run on Windows and macOS in CI? | It runs on Linux only; the build, the lints, and the tests run on all three ([AGENTS.md](../../AGENTS.md)) | No. The audit reads documents and the crate manifests, which do not differ by system; three runs would cost time and prove nothing more |
| Should the 10% benchmark gate be wired into CI now, or when a budgeted path is measured? | The comparison is built (`a2m-bench compare`); CI does not run it, because no budgeted path is measured ([AGENTS.md](../../AGENTS.md)) | When the first budgeted path is measured: a gate with no budget to compare against would pass without proving anything |
| Should the command pack start programs that run what their arguments name: build tools, container runners, language launchers, version managers? ([decision 0282](../decisions/0282-whether-the-command-pack-may-start-programs-that-run-what-their-arguments-name.md), Proposed) | Refused by name, every command of them, because what runs is code nothing pins (Choice, [decision 0271](../decisions/0271-the-check-of-programs-the-product-starts-after-a-second-reader.md)); it narrows what an agent can reach, which the North Star counts against it ([decision 0277](../decisions/0277-the-north-star-an-agent-uses-the-software-a-person-already-has.md)) | Widen in two steps: first the commands of such programs that run no code but their own, then the rest with fixed arguments, the program's own files pinned by digest, a declared effect and hazard, and a person's approval for every change. Package runners that fetch at run time stay refused |

Record 0282 is the one Proposed record that asks the owner a question. The other Proposed records are design decisions, which any agent may accept or supersede.

## Keeping this page true

When the owner answers a question, its record is marked Accepted or Rejected with the answer, the row leaves this page, and [SCOPE.md](../../SCOPE.md) is updated in the same change. A new question is added here when its record is written.
