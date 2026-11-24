# 0399. Every refusal of the approval page is named, and a script is proven unable to approve

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-24

**Changes:** `a2m-local`: the `A2M-Failure-Code` header (`FAILURE_CODE_HEADER`) on every answer of the loopback listener that records nothing, and three codes in its table, `request_not_from_page`, `page_not_found`, and `change_not_shown_whole`. A change already approved no longer blocks a new page when the kernel asks for one. `a2m-actions`: an approval that is stale when the run's writes would begin clears nothing, and the run waits for a person again. The product crate: `tests/approval_surface.rs`. Roadmap package 3.6, whose remaining step is a person at a macOS and a Windows machine.

## Context

Package 3.6's proof is "a person approves; a script cannot". The desk and the page were built and tested piece by piece. Nothing yet showed both halves against the real local engine, with the test application asked what it stored. Building that test showed three things.

1. A refusal on the approval page was a page of words and a status. A program that tried a press could not be told by name why nothing happened, and a test could not hold each refusal to its reason.
2. An approval whose window had ended when the run was resumed cleared nothing, as it should. But nothing said so, and the run never asked again. The policy engine refused with `approval_stale`. The engine leaves a run at `needs_approval` resting, so the failure was dropped. The kernel's memory that a person had approved went on telling the agent "a person approved this write, resume it". Every resume took a new lease and did nothing.
3. The local surface remembered the change as approved, so even an ask for a new page got none.

## Decision

1. **Each refusal of the loopback listener names its code.** The `A2M-Failure-Code` header carries it, and the page's words for a person stay as they were:
   - `request_not_from_page`: wrong host or origin, or a missing, wrong or spent one-time value.
   - `page_not_found`: no page at that address.
   - `prompt_busy`: a second press, or another prompt is up.
   - `presence_failed`: the prompt was dismissed, denied, or none is available.
   - `surface_unavailable`: no approval desk.
   - `change_not_shown_whole`: the change was shown in part.
   - `approval_stale`: the change no longer waits.
   - A refusal by the desk keeps the desk's own code.
2. **An approval that is stale when the writes would begin leaves the run waiting for a person.** In the transaction that would have used it, the run's events record a `need` naming `approval_stale` and the policy engine's reason. Stale means expired, used, revoked, or given for another diff. The kernel forgets that the run was approved, so the run's next answer gives a new page, and the local surface opens one. Nothing is written until a person approves again. This is what the engine already said a run at `needs_approval` does: "it goes on waiting".
3. **A program on the machine is modelled as able to raise the presence prompt and unable to answer it.** The test's prompt stands for the operating system's prompt with a person in front of it. It passes only right after that person pressed Approve on a page showing that change, which is what the page tells a person to do. A replay of the page's exact request therefore raises a prompt the person cancels. That a program cannot answer the operating system's own prompt is hypothesis H-12, which no test here can show.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Name the code in the page's text only | A program reading the answer would have to parse words written for a person. The header is one fixed name |
| End a run whose approval expired as `expired`, as a decline does | An expired approval is not a person's refusal. The diff may still be wanted, and the person can approve it again without the agent calling the tool again. Section 212 keeps `expired` for a decline and for a page's window that ends in silence |
| Keep the surface's memory that a change was approved, and have the kernel say when to ask again | It would change `ApprovalAsk`, a public contract. Inside one process the kernel asks a surface about an approved change only when that approval cleared nothing |

## Proof

`crates/anything2mcp/tests/approval_surface.rs` runs against the real local engine, with the loopback listener, the channel, and both toolsets. Both tests passed five runs in a row on Linux.

`a_person_approves_on_the_page_and_no_program_on_the_machine_can`:

- These are all refused, and the test application stores nothing:
  - three approve-like commands;
  - five approve-shaped routes on the local channel (`route_not_found`);
  - guessed approve tools in both toolsets, of which neither lists one;
  - other paths on the listener (`page_not_found`);
  - presses with no origin, another origin, or another host, and presses with no, made-up, older, or spent one-time values (`request_not_from_page`);
  - the exact replay of the page's own request (`presence_failed`).
- A person in a real browser then approves, and the run is written once with the right values.
- A replay afterwards reaches no listener, and resuming again writes nothing.

`an_approval_is_for_one_run_and_one_diff_used_once_and_survives_a_restart`:

- The first run's page value pressed on another run's page is refused, and that run writes nothing.
- A record changed at the stand after the approval stops the run with `changed_since_preview`.
- An approval recorded before the engine stopped is used by the next engine, and the write is made once.
- An approval past its window, one minute on that machine, clears nothing: the events name `approval_stale` and no intent is recorded. A new page is given, and a new approval writes once.

## Not done

- The presence prompts on macOS and Windows, run by a person at each machine. This is the owner's step of package 3.6, and the package stays held until it is done.
- The window is shown ending by one minute of real time, the setting's least, and not by a clock moved forward: the engine keeps one clock, and a test that moves only an injected one tests a machine that does not exist.
