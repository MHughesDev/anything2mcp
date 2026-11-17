# 0131. The front doors joined: `serve`, the thin client, and the bridge

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Settles:** how the product crate joins `a2m-local`, `a2m-api`, and `a2m-mcp` into the local engine; what a command and the bridge do when no engine runs; and the two small additions to `a2m-api` that the join needed. It changes the interface sketch of the design of `anything2mcp`.

## Context

The three front doors were built and tested apart ([decision 0117](0117-the-local-channel-and-the-person-surface-on-one-machine-as-built.md), [decision 0116](0116-the-rest-router-takes-its-caller-from-the-mount-and-chooses-its-status-in-one-table.md), [decision 0115](0115-the-mcp-endpoint-as-built-checks-in-front-of-the-sdk-and-a-redactor-on-the-way-out.md), [decision 0130](0130-the-mcp-protocol-layer-is-our-own-because-the-sdk-brings-an-unmaintained-crate.md)). The product ran every command with the engine inside the command ([decision 0119](0119-the-product-crate-as-building-it-settled-it.md)). Starting an engine on demand is designed and not built in `a2m-local`.

## Decisions

1. **`serve` binds the channel first, then opens the kernel.** `Channel::bind` takes the engine lock, so a second engine on the same data directory is refused, by name (`busy`, `engine_running`), before it opens the store. A client that connects while the kernel opens waits for the answer to its hello, which comes when the engine serves. *Instead of:* opening the kernel first, as the lifecycle in spec section 222 said. `Channel::bind` owns the lock, so a second `serve` would have opened a second kernel on the same store before it learned that it had lost.
2. **One layer of the product crate stands in front of both routers.** It turns the `Peer` the channel saw connect into a caller through `Kernel::verify_caller`, and refuses a request with none. A stopped engine names nobody and answers its own status, as in one process. Role crates cannot name each other's types, so this layer is here.
3. **A caller on the channel is an agent, unless the command line says a person is at its terminal.** The MCP face and an SDK are always agents. A command says so with the request header `A2M-Person-At-Terminal`, as it decides it in one process: its input and error output are a terminal. This stops a mistake and is not a security boundary; a person's act is proven by the presence prompt. *Instead of:* taking every channel caller for an agent, which would make the same command end differently with and without an engine.
4. **A command is a thin client when an engine answers, and runs the engine inside itself when none runs.** *Instead of:* answering that an engine is needed, with exit code 4. A first user, a CI job, and every end-to-end proof so far run one command with nothing else on the machine; until an engine is started on demand, refusing would make each of them start `serve` by hand. An engine that holds the engine lock and does not answer is starting or closing: the command waits for it up to twenty seconds and then ends with `busy`, `engine_unreachable`, exit code 4. It never opens a second kernel beside it. `--in-process` is the explicit form of the second way and is refused while an engine runs (`busy`, `engine_running`). The setting `engine.in_process` is not read by a command yet.
5. **The thin client sends an action to `POST /api/v1/actions/<name>` and prints the answer with the one renderer.** The renderer works from the JSON form of a response and an exit code, so a command prints the same whichever way it ran. The client looks at nothing in a response but its exit class. Any failure of the way to the engine ends with exit code 4.
6. **The bridge starts no engine and does not run one.** While none runs it stays up and answers each request with error -32000, carrying the failure and its next step (`anything2mcp serve`); the next request is carried as soon as an engine answers. It says the same once on standard error. Standard output carries protocol messages only, so even a malformed `bridge` command is told on standard error alone. *Instead of:* running the engine inside the bridge, which decision 4 would suggest. A bridge lives as long as its client, so that would be a second long-lived engine on one store, or an engine that dies under every other client when the first one closes.
7. **The bridge opens one connection for each message.** A call that takes long then holds up no other message, and an engine that was restarted is simply found. The connection is kept until the answer's body has been read; dropping it is how the engine learns that a caller has gone. *Instead of:* one connection for the bridge's life, on which HTTP/1.1 would make every request wait for the one before it.
8. **`engine.stop` is carried out by the engine's process, after the dispatcher has decided.** The layer of decision 2 sends the action to the dispatcher, so that the policy engine decides and the decision is recorded; the dispatcher answers that the action is the client end's (`client_action`), and the process that serves the channel then stops serving. A command sends `engine.stop` like any other action. `serve` also stops on the terminal's interrupt, on the termination signal where the system has one, and after `engine.idle_stop_minutes` with no connection and no page waiting. *Instead of:* a route of the product's own that stops the engine, which would have gone past the one decider.
9. **The verified write through the served engine is proven in the test's own process.** The program ships no presence prompt on any operating system, so no approval can be recorded through the binary. The test starts the same `LocalEngine` that `serve` starts, with a prompt the test answers, and approves on the real page over loopback. Nothing was added to the program for the test.

## Changes below the product, each forced by a test of the join

| Crate | Change | The test that needed it |
| --- | --- | --- |
| `a2m-api` | Every rendered response carries its exit class in the header `A2M-Exit-Class` (`EXIT_CLASS_HEADER`). The body holds no exit class and the status is coarser than one: a failed input that is not malformed has status 400 and exit code 1 | `two_commands_share_one_engine_a_second_engine_is_refused_and_a_stop_leaves_nothing`: four commands end with the same exit code and the same object through the engine and in one process |
| `a2m-api` | `ProjectFolder`, a request extension beside the caller, becomes the request's project folder. The router had no way to carry one, so every action on a project's files failed through the engine | The same test: `draft lint` through the engine reads the target files of the project folder the command was run in |

No public signature changed. `a2m-local` and `a2m-mcp` were joined as they were.

## What was measured

- **Two unrelated client libraries against the real program** (H-40). The reference client library for Python (`mcp` 2.3.0) and the one for TypeScript in both release lines (`@modelcontextprotocol/sdk` 1.32.2 and `@modelcontextprotocol/client` 2.4.0) each started `anything2mcp bridge` as a stdio server. Seven runs; all seven connected, listed the eleven tools of the `use` toolset, called the published read by name, and read the structured result. Four asked for 2026-07-28, three of them with `server/discover` first; three asked for 2025-11-25 with `initialize` and were answered with it. [Research note 18](../research/18-mcp-server-base.md) has the table.
- The Python library wrote one line to its standard error on each run: the tool it called is not in the listing, so it could not check the structured content against an output schema. The default listing holds the built-in tools; a published operation is found with the search tool and called by name.

## What follows

- The design of `anything2mcp` lists the interface, the roles as built, and the proofs.
- Owed by `a2m-local`: starting an engine on demand, after which decision 4 is revisited; and a way to keep the engine lock until the kernel has closed. Today the lock is released when the channel stops serving, a moment before the kernel closes.
- Owed by `a2m-actions` and `a2m-mcp`: the project folder for a call that arrives over MCP. The build toolset's actions on a project's files answer `path_not_allowed` through the bridge.
- Owed by `a2m-actions`: a count of the runs in hand, so that a run with no connection waiting holds an idle engine open.
- CI must show the three tests of `local_engine.rs` passing on Windows, where the named pipe has never run, and on macOS.

## What would reopen it

Start on demand in `a2m-local`; a presence prompt on any operating system, after which the write is proven through the binary; a client that cannot wait for a bridge that answers with an error; or measurements that show a connection for each message costs more than it saves.
