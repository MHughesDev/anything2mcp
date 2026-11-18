# 0260. The machine's network facts are served as reads of the local user, and one of them is listed

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-18

**Changes:** the crate `a2m-actions` ([its design](../architecture/crates/a2m-actions.md), build-order step 21), the [action catalog](../reference/action-catalog.md), the saved OpenAPI copy and both generated clients under `bindings/`, and the product test `crates/anything2mcp/tests/net_tools.rs`. Carries out the registry half of [decision 0243](0243-the-machine-reads-its-own-network-state-and-sends-nothing.md), after the cleaner of [decision 0261](0261-a-proxy-values-credentials-are-removed-before-any-other-cut.md).

## Context

The owner decided that the product knows the networks its machine is on, and that an agent gets tools to inspect the network it is on ([decision 0222](0222-the-product-is-aware-of-the-network-its-machine-is-on.md)). The crate `a2m-netstate` reads those facts from the operating system and sends nothing (decisions 0243 and 0261). No action served it, so no person and no agent could reach it.

## Decision

1. **Seven actions, one per reader.** `net.interfaces`, `net.routes`, `net.resolvers`, `net.neighbours`, `net.listeners`, `net.proxy`, and `net.standing`. Each takes no input, is a read, and answers with the reader's own serialized form. That form already shows a hardware address as its length only, names a program by its file name, and removes a proxy value's credentials (decision 0243 point 4, decision 0261). No action adds an input that would show more.
2. **Who may call: the local user.** The policy engine is asked for the role `Admin`. On one machine every local caller is the installation's person acting for itself, and that person holds every role, so an agent acting for the person may call all seven. On a shared server they would be for administrators only, which fits a server's map of its own networks.
3. **`net.standing` is listed in the `use` toolset; the other six are reached through `action.run`.** The `use` toolset held eleven actions against the cap of twelve that `BuiltInToolset::cap` sets. (Point 3 of [decision 0094](0094-the-action-layers-interface-as-building-it-settled-it.md) states a cap of 19 for `use`; the code has said 12 since it was first written. This record does not settle which is meant, and works to the code.) `net.standing` is the summary an agent needs first: what kind of place each address is, and which interface carries each family's preferred default route. It takes the last place, and its result names `net.interfaces` through `action.run` as what to call next. The `use` toolset is now full: an action marked for it later is left past the cap, which `doctor.run` reports, unless it takes this place or the cap is changed by a record of its own.
4. **The default health need stands.** Each action needs the store to record its decision. A stopped kernel serves only the engine's own status, so it refuses these as it refuses every read (`busy` / `engine_degraded`). A degraded one still answers. The alternative, `HealthNeed::Nothing`, would have served the machine's network facts with no decision recorded.
5. **Each reader runs on a blocking thread.** The readers block on system calls and file reads. A thread that ends without an answer is `busy` / `engine_degraded`; a reader's own error keeps its class and code (`net_fact_refused`, `net_reader_not_built`, `net_answer_unreadable`, `net_read_failed`).
6. **`net.proxy` says whose setting it reads.** On Linux it reads the engine's own environment, not the caller's shell: a command and a bridge are thin clients of the engine, which may have been started with another environment. On Windows it reads the user's system setting. macOS has no reader yet and answers `net_reader_not_built`, as does `net.listeners` there.
7. **Every word list in the output schemas is taken from `a2m-netstate`'s model.** The address scope uses the names of `AddressScope` in `a2m-core`. The other lists are written in the catalog; a unit test serializes every value of every list, each enumerated by a `match` with no wildcard, and checks the result against the schema, so a value added to the model and not to the catalog fails to compile or fails the test.

## Tests

| Test | Proves |
| --- | --- |
| `a2m-actions`, `tests::net::each_network_action_answers_an_agent_with_a_decision_and_fits_its_schema` | Through the dispatcher, with an agent at the MCP door as the caller: each answers, a decision is recorded, the result fits its schema, the standing holds an address of this machine, and its next action is `action.run` |
| `tests::net::only_net_standing_is_listed_and_every_one_is_for_the_local_user` | The listing, the role, the effect, and that no action is past the `use` cap |
| `tests::net::a_stopped_kernel_serves_no_network_fact` | Point 4 |
| `tests::net::every_word_a_reader_writes_is_one_its_schema_takes` | Point 7 |
| `anything2mcp`, `net_tools.rs`, `each_network_tool_answers_through_the_command` | The command of each of the seven; each decision is explained by `policy.explain` as an allow; a port the test bound on `127.0.0.1` is listed with the scope `this_machine` |
| `net_tools.rs`, `an_agent_sees_net_standing_listed_and_reaches_the_rest_through_action_run_at_the_mcp_door` | Through the real `bridge` and `serve`: `a2m_net_standing` is listed, `a2m_net_interfaces` is not, and `a2m_action_run` reaches `net.interfaces` |
| `net_tools.rs`, `a_proxy_password_in_the_engines_environment_reaches_no_caller` (Linux) | An engine started with `http_proxy=http://someone:pa/ss?wo#rd@proxy.example:3128` answers a command whose own environment names another proxy with `http://proxy.example:3128` and `credentials_removed: true`; no part of the user name or password is in the command's output, the engine's log, what the engine wrote, or any file under the machine's home or the project |

The helpers that run `serve` and `bridge` as child processes moved from `local_engine.rs` to `tests/common/served.rs`, so that both test files use one copy.

An MCP result carries no decision's identifier, so the product test cannot explain a decision made at the MCP door. That each call there is decided is proven at the kernel, with the same caller, and through the command.

## What only CI can prove

Both crates were compiled and linted for `x86_64-pc-windows-msvc` and `aarch64-apple-darwin`; the tests ran on Linux only. On Windows the listeners test expects the test's own port in the reading, and `net.proxy` reads the system setting, which the tests do not set. On macOS the tests expect `net_reader_not_built` from `net.listeners` and `net.proxy`.

## Not done

- An input that narrows a reading, such as one interface. None is needed yet.
- The wireless network's name, network recognition, and macOS listeners and proxy: they belong to `a2m-netstate` (decision 0243).

## Alternatives weighed

- **List none of the seven, and reach all through `action.run`.** Refused: decision 0222 asks for tools an agent sees, and an agent that does not know the actions exist does not ask for them by name.
- **Raise the `use` cap to list all seven.** Refused: the cap needs a record of its own (see point 3), and six more tools in every client's list cost more than one call through `action.run`.
- **`HealthNeed::Nothing`, as the reader's proposal had it.** Refused by point 4.
- **One action that returns every reading at once.** Refused: the listeners walk and the neighbour table are the slow readings, and an agent that asks where it stands should not wait for them.
