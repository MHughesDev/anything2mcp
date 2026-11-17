# 0094. The action layer's interface, as building it settled it

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Changes:** the interface sketch in the design of [`a2m-actions`](../architecture/crates/a2m-actions.md), and open points of [spec section 220](../spec/2-kernel/220-actions-and-the-authoring-interface.md).

## Context

The design of `a2m-actions` sketched `Kernel::open(settings, providers, actions, person)`, a `Registry` that refuses to build when a built-in toolset lists more actions than its cap, an `ApprovalDesk` reached through `Kernel::approvals()`, and a JSON Schema validator "to be verified". Building the crate against the finished crates below it showed where each of these did not fit.

## Decision

1. **The kernel opens in two calls.** The provider registry needs the redactor and telemetry before it can be built, the redactor is the vault's, and the kernel needs the registry. `Kernel::prepare(KernelParts)` opens the store, the vault, and telemetry; the product crate builds the provider registry with `Prepared::host_services`; `Kernel::open(prepared, registry, problems, person)` loads policy and starts the keeper and the scheduler. Neither fails: an impaired part lowers the health state. The key store, the clock, and the randomness are inputs. The store is opened without telemetry, so commits have no span yet.
2. **The approval desk is moved to the person surface, and the kernel keeps no copy.** `PersonSurface::take_desk` receives the one `ApprovalDesk`, which holds the one `ApprovalSurface` that `Policy::open` returns. There is no `Kernel::approvals()`. A handler is given a `&Kernel` and nothing else, so no action can reach the desk: the rule holds by where the handle is, not by review.
3. **A toolset's cap limits what is listed, not what may be marked.** The catalog marks 25 actions for the `build` toolset against a cap of 15. The registry keeps the catalog's marking as data and never refuses to build over it. `ActionRegistry::toolset` lists the three `action.*` tools, then the marked actions in catalog order until the cap is reached; a marked action past the cap is reached through `action.run`, and `doctor.run` says how many there are. The caps are 15 for `build` and 19 for `use`.
4. **Schemas are built in code from a flat list of fields, and checked by the compiler's schema type.** An action's input is a list of typed fields, from which its `JsonSchema`, the schema type of `a2m-files`, is made; `check_json` validates every input and `to_json` publishes every schema ([record 0124](0124-the-plan-declares-a-surfaces-programs-and-a-schema-checks-run-time-values.md)). No outside validator is added. The field list also carries what a schema does not: the help text and which field a command takes without a flag.
5. **A verified caller is a type only the kernel makes.** `Kernel::verify_caller` returns a `Principal` whose fields are private. On one machine the only caller is the operating-system user the engine runs as, named by the local channel; bearer tokens and API keys are refused until a server's front doors are built. The local user's principal and the workspace are made at the first open and kept in the store.
6. **An action that is not built answers `input`/`action_not_built`**, after its input was validated and its decision recorded. An action the client end carries out answers `input`/`client_action` when it reaches the engine.
7. **A response may carry both a result and an error.** A run that failed is reported with the run's identifier in `result` and its failure in `error`. A run that rests waiting for a person has a result, no error, and the exit class `person_needed`.
8. **What a run was called with is kept by this crate**, in its own table, keyed by what makes a run the same run: the stored input, and the verified facts about the caller that the guard's question to policy is asked with. The ledger's run row exposes neither.
9. **`Kernel::publish_plan` exists only for tests**, behind the cargo feature `testing`, so that the tests of the role crates have a published tool without a pack that compiles.

## What follows

- The product crate calls `prepare`, builds the registry, then `open`, then `start_runner_loop`.
- `a2m-local` and `a2m-api` implement five methods of `PersonSurface` and keep the desk.
- The registry is named `ActionRegistry`, so that it is not confused with the provider registry.
- The open point of section 220 about the build toolset's cap is settled by item 3; the one about the validator by item 4.

## Rejected

- **A closure that builds the registry inside `Kernel::open`.** The product crate would hand its packs into the kernel's call, and the two phases would be hidden.
- **Raising the build toolset's cap to 25.** The cap exists because a long tool list costs every caller context.
- **A public `Kernel::approvals()`.** Every handler would be one call from the desk.
- **An outside JSON Schema validator.** A second meaning of "fits the schema" beside the compiler's.
