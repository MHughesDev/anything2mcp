# 0289. A failure is made from its code's row, so the place that makes it cannot give it another class

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-18

**Changes:** the [design of `a2m-core`](../architecture/crates/a2m-core.md) (`failure_codes!`, `CodeEntry`, `FailsAs`), the failure helpers of `a2m-provider`, `a2m-ledger`, `a2m-engine`, `a2m-local`, `a2m-pack-data`, `a2m-pack-desktop`, `a2m-pack-device`, `a2m-pack-file`, `a2m-pack-http`, and `a2m-pack-web-ui`, and `tools/failure_codes.py`. Follows [decision 0258](0258-every-crate-declares-its-failure-codes-in-one-table.md).

## Context

After every crate moved to the table form of decision 0258, a failure was still made with `Failure::new(class, code, ...)`, or with a crate's helper that took the class apart from the code. So the class was written twice, once in the table and once where the failure was made, and nothing held the two together: the desktop pack's discoverer gave `step_not_usable` the class `unknown_variant` while its table said `input`. Two crates looked the class up in their own list instead, and fell back to a class of their choosing for a code the list did not hold (`unknown_variant` in the file pack's helper, `possible_commit` in its lowering); `a2m-local` fell back to a next step of its choosing the same way. And one crate, `a2m-pack-file`, was still outside the table form: the generator did not name it, because its manifest gave each of its codes a class.

## Decision

1. **A row per code, as a constant.** `failure_codes!` also writes a module `row`, with a `CodeEntry` constant per row under the code's constant name. The `FailureCode` constants and `TABLE` are made from those rows, so each row is written once.
2. **A failure is made from its row.** `CodeEntry::failure(message, next)` makes a failure in the row's class. A crate's helpers take a row (`&codes::row::NAME`), not a class and a code, so a call site cannot state a class, and a code with no row does not compile. A failure whose case adds nothing to the row's next step gives `row.next`. `FailsAs` is made from a row (`FailsAs::from(&row)`), and `FailsAs::failure` makes a step's failure in the class its plan states.
3. **Where it applies now.** Every crate whose helpers took a class and a code apart: `a2m-provider`, `a2m-ledger`, `a2m-engine`, `a2m-pack-data`, `a2m-pack-desktop`, `a2m-pack-device`, `a2m-pack-http`, `a2m-pack-web-ui`; and `a2m-local` and `a2m-pack-file`, whose lookups fell back. In those crates the class-named helpers (`input(code, ...)`, `busy(code, ...)`) and the direct `Failure::new` with a stated class went too. A test in each, `every_failure_takes_its_class_from_a_table`, finds no `Failure::new(` and no `class: FailureClass::` in its sources outside tests, except where a recorded failure is remade in the class it has (named in the test). No code's class changed.
4. **The file pack moved to the form.** Its nineteen codes are rows; `payload_unreadable` and `value_out_of_range` are the core's, in `codes::PASSED_ON`. Its `busy` codes do not pass by themselves: `file_locked` waits for a person to close the file, and `storage_full` for space.
5. **The generator reads one form.** `tools/failure_codes.py` reads `failure_codes!` tables and manifests only. `MIGRATED` is gone; a literal `FailureCode::fixed("code")` in a crate's source outside a table is a fault, so no crate slips back. `--list` prints the table and writes nothing.
6. **A shared code's row is worded for every crate that declares it.** The reference shows the row of the first crate in the generator's order. `a2m-telemetry`'s `storage_full` row now speaks of something that cannot be written, a log, a record of the ledger, or a file, and `a2m-local`'s `answer_lost` of a request such as a tool call. The log writer's own failure still says it is the log folder.

## Alternatives weighed

- **Look the class up in the crate's `TABLE` at run time, as the file and stream packs did.** No change to the macro. A code the table lacks still needs a class from somewhere, which is the fallback this record removes; only a test could catch it, and only for the codes the test thinks of.
- **A procedural macro, or one constant per row named by joining names.** Either needs a crate the workspace does not have. A module named `row` gives the same name twice in two namespaces with `macro_rules!` alone.
- **Convert every crate's `Failure::new` now.** Several hundred call sites across the remaining crates, each run with a stated class that matches its table today. Left for later, crate by crate; the generator could then refuse a stated class everywhere.
