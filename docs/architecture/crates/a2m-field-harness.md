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

## The box it runs in

The runner starts the harness in the box, a container on the run's closed network. By default the box is the runner's own Debian image, run as the runner's user. A recipe may change three things about it, and nothing else ([decision 0447](../../decisions/0447-the-field-test-box-takes-a-pinned-image-a-user-that-is-not-root-and-a-named-filter-for-the-browsers-sandbox.md)), which is what a browser in the box needs:

| Option | What it allows | What the runner refuses |
| --- | --- | --- |
| `box_image` | Another image, such as one with a browser at one of the web pack's known places, named `name@sha256:<64 hex>`. Its entry point is never run | Any image not named by its digest |
| `box_user` | A user, `uid:gid` in numbers, given the engine's home and the project | Root; a name; another user than its own when the runner is not root |
| `box_seccomp` | `browser_sandbox`: the container engine's default filter, kept in `tools/field-box/` and checked against its hash, plus `clone` of a user, a process, and a network namespace and `unshare` of a user namespace | `unconfined`, a path, a filter of the recipe's own, any other name, and the filter for a box whose user is root |

Whatever the options, a recipe that holds a flag that switches a browser's sandbox off, in its text or in a file it gives the box, is refused before anything starts, and the step `sandboxed` proves from the box's processes that each renderer runs in a user namespace of its own. The report states the box's image, user, and filter.

A fourth option gives the box a desktop session, which a desktop application needs ([decision 0999](../../decisions/0999-the-field-test-box-takes-a-desktop-session-and-one-application-the-recipe-names.md)):

| Option | What it allows | What the runner refuses |
| --- | --- | --- |
| `box_desktop` | `{"application": [argv]}`: before the harness starts, the script `tools/field-box/desktop-session.sh`, mounted read-only, starts in the box as its user a virtual display that listens on no TCP port, a session bus, and the accessibility bus, each in a folder of the box's own, then the application the recipe names; the runner waits until the accessibility registry lists it, or stops as `desktop_not_ready`. The harness and every command get the session's `DISPLAY` and `DBUS_SESSION_BUS_ADDRESS`. A system of `kind: desktop` is that application alone, with no system's container | Without a `box_image` or a `box_user`, as root, with any key but `application`, or an application whose program is not an absolute path; nothing of this machine's display or buses is ever given to the box |

The step `desktop` lists the applications the box's registry holds, asked of the bus by the runner and never through the product. The report states the application and the script's hash.

## Where it runs

On Linux alone. The closed network of decision 0310 is a container network made with `--internal`, and the runner (`tools/field_test.py`) reaches the system by the container's address on it, which only Linux with docker allows; on Docker Desktop the containers are in a virtual machine whose addresses the host cannot reach. The harness itself is built from the product and would compile anywhere, but the binary in the box is a Linux one.

So no field test has run, or can yet run, on Windows or macOS, and none can approve a write there: the field prompt is safe only inside the closed network, and outside it a write's approval is a person's at the system's own prompt. What only those two systems have (the desktop pack's back ends, the presence prompts, a file kept by a sync program, the named pipe) is proven by CI against the test stands and by the owner's checks, never by a field test. Roadmap package OS5 is the first field test on each of the two: reads only, through the shipped program, with no container ([review of lens 13](../../reviews/other-operating-systems-review-2026-11-24.md), finding 6).
