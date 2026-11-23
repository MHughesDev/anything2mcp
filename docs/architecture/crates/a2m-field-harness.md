# `a2m-field-harness`

The field harness: the product's own `serve`, with one difference. Its presence prompt is the field prompt of `a2m-local`, which writes its question to standard output and takes its answer from one line of standard input. A field test's runner starts the harness, keeps the harness's standard input, and writes the short hash of the change it means to approve. The shipped program never has this prompt ([decision 0310](../../decisions/0310-the-field-harness-runs-in-a-closed-network-and-says-in-the-ledger-what-it-is.md)).

It is test equipment, like the test stands and the benchmark harness. It lives in `tools/field-harness/`, is a member of the workspace, and is never packaged: no wheel and no npm package holds it.

## Facts

| | |
| --- | --- |
| Side | Test equipment |
| Layer | 12, Field harness |
| May depend on | Everything |
| Depends on | `anything2mcp` (the command line's library, which it starts with a `Host` that carries the prompt), `a2m-local` (the field prompt), and Tokio |
| Roadmap packages | FT4 (decision 0310) |
| Carries out | The field harness of [decision 0310](../../decisions/0310-the-field-harness-runs-in-a-closed-network-and-says-in-the-ledger-what-it-is.md), points 1, 2, and 5 |

## Owns

- One program, `a2m-field-harness`, which takes the arguments of `serve` and no other command.
- The closed-network check: before it does anything it tries one public address, `1.1.1.1:443`, and refuses to serve when the address answers. Only a refusal to route (network or host unreachable) or no answer in time counts as closed; a connection made, a connection refused, and any other outcome count as reached.

## Interface

None. It is a program, not a library. It builds a `Host` from the process, sets `Host.prompt` to the field prompt over its own standard input and output, and hands the arguments to the product's command line.

## It must never

Decide anything the policy engine decides; hold engine logic; answer an approval of an `irreversible` operation (the field prompt refuses it, always); be reached by a package, or named by any crate.
