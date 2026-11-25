# H-18: an agent with no prior knowledge reaches a first tool in ten minutes

**Status:** supported

**Tested:** 2026-11-24, package 5.6a, on the release build of the staging branch after decision 0425

## What was assumed

An agent with no prior knowledge reaches a first tool in ten minutes with the guidance file, `status`, and `docs`.

## What was done

Three agents, each started fresh with no knowledge of this repository, were given the same brief and nothing else:
- the path of the installed program;
- the test application's address, an order-entry stand of `example-app`, on `127.0.0.1:4010`;
- that `anything2mcp docs get` exists;
- an empty working folder;
- the goal: make a tool that reads an order, publish it, and call it.

The brief forbade reading any file other than the program's own output and documentation. It also forbade stopping the engine.

Each run had:
- its own engine home and its own project folder;
- the field harness as its local engine, with a vault passphrase made up for that run;
- a network namespace that holds only loopback, standing in for the closed network of [decision 0310](../decisions/0310-the-field-harness-runs-in-a-closed-network-and-says-in-the-ledger-what-it-is.md).

The person's press was a small script that opens the link the program gave, presses the page's button, and answers the harness's prompt with the short hash that the prompt showed. Each agent logged the time before each command. The three ran at the same time on one machine.

A first set of three runs was discarded. Those runs shared a shell working directory and a scratch folder, so one agent's commands reached another's engine. The program for each run was then fixed to that run's home and folder, and the runs were repeated. The times below are from the repeated runs only.

## What was found

| Run | First command | Published tool called | Time | Commands |
| --- | --- | --- | --- | --- |
| 4 | 23:06:41 | 23:08:30 | 1 min 49 s | 26 |
| 5 | 23:06:47 | 23:08:39 | 1 min 52 s | 26 |
| 6 | 23:06:43 | 23:08:46 | 2 min 03 s | 28 |

Every run ended with the published read called on an order. The answer matched what the test application holds, and each agent checked it against the application's own page.

Each run made the same core steps, in this order:
1. `docs get guidance`
2. `init`
3. `target add --kind web`
4. `scout run`
5. `draft delete`
6. `draft try`
7. `fixture record`
8. `test run`
9. `binding promote`
10. `publish request`
11. the person's press
12. `publish request` again
13. `tool search`
14. `tool call`

The rest were `--help` reads, `docs get` of a topic, and a look at the application with `curl`.

The bet holds with a wide margin, about a fifth of the ten minutes. The time went to these places where an agent went wrong or slowed:

- **A list on the command line (all three runs).** `draft delete --files <list>` does not say how a list is written. A JSON array and a comma-separated string were each read as one file name and refused as `not_found`. Runs 4 and 6 found the repeated flag after two refusals; run 5 went through `action run draft.delete` instead.
- **The scout's list read (all three runs).** The scout drafted the all-orders read with the screen address `/{key}` and a required `key` input. The application's `/` redirects to `/orders`, and the scout appears to have taken the redirect as a keyed page. Every agent deleted that read.
- **Nothing tried by the scout (all three runs).** The scout tried none of its three reads, because none stated an example of its key. Each agent read an order id off the application and ran `draft try` itself.
- **A `next` that skips a step (all three runs).** After `test run`, `next` names `publish.request`. The guidance requires `binding.promote` first. All three agents followed the guidance.
- **A promote refused (run 6).** The run id of the first `draft try` was refused with `promotion_unproven`; a second try, naming its binding, was accepted. Drafts deleted and a fixture written between the try and the promote may have made the first try stale. In the discarded runs, a promote failed the same way once a try's run had gone from the store.
- **The answer after publishing (all three runs).** Asked again after the person published, `publish request` answers ok with the version and warns "this call did nothing more". Two agents read it as something still pending.
- **Thin outputs.** Every drafted output is `untrusted_text`, numeric ones included. The order read returns customer and status but not the order's own id.
- **Small texts.** `init --help` says no action is beneath it. A scout entry that was not drafted gives "the rule gave no reason".

The discarded runs found one more fault, through their crossing. The engine chooses its store by the folder a command runs in, while `--project` chooses only the files. When the two differ, `tool search` lists a tool that `tool call` and `tool describe` answer `not_found`, with no error.

## What changes

H-18 is marked supported in the register. Package 5.6a is done. No section changes because of the time.

Each place listed above is a fault of the first hour, and each goes to the Build Order's line as work for an agent:
- the list syntax in help;
- the scout's redirect and its untried reads;
- the `next` after `test run`;
- promotion from a try whose run is gone;
- the warning after a publish;
- the store chosen apart from `--project`.

## What is still unknown

- **The agents were of one kind.** All three were one make of agent, and a model that answers fast. A slower agent, or a person typing, would take longer. The steps would be the same, so the margin is likely to hold.
- **The person read nothing.** The press was a script, taking about seven seconds a run. A real person reads the page before pressing.
- **The application was the easy case.** It was one web stand with no sign-in. A first tool behind a sign-in, or over another kind of system, is not measured.
- **The press was not checked in the ledger.** No run checked that the ledger names the presence that approved the publish as `field_harness`.
