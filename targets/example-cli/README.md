# The command-line test stand

A program, `example-cli`, that behaves like a business command-line tool: an inventory for the imaginary order-entry system named `example`. The command pack is proven against this program and never against a real one. The pack's design, and the list of cases a stand for it must imitate, is in [spec section 306](../../docs/spec/3-packs/306-the-command-pack.md); the rules every stand follows are in the [test application plan](../../docs/plan/test-application.md).

It is test equipment. A product crate may depend on it in its tests only.

## Running it

```text
cargo run -p example-cli -- --help
cargo run -p example-cli -- --home ./somewhere item list
```

Rust tests do not use `cargo run`. They call `example_cli::harness::Stand::start()`, which gives the test a program of its own, a fresh home folder with the seed data, and the control interface, and removes all three when the test ends.

## What the program does

```text
example-cli [--home <dir>] <command> [arguments]
```

The data lives in one file, `items.json`, in the home folder. The home folder is named by a leading `--home <dir>`, or by the variable `EXAMPLE_CLI_HOME`. A home folder with no data file holds the seed data. Help and version need no home folder and no environment.

| Command | What | Effect |
| --- | --- | --- |
| `--version`, `version` | Prints `example-cli 2.4.1` | None |
| `--help`, `help`, `help <command>`, `<command> --help` | The list of commands, or one command's usage, arguments, and exit statuses | None |
| `item show <sku> [--format text\|json]` | One item, as lines of `field: value` or as one JSON document | Read |
| `item list [--location <code>] [--ref <reference>] [--format table\|json\|jsonl]` | Items as a table with tab-separated columns and a header, as one JSON document, or as one document a line | Read |
| `item add --name <name> --location <code> [--quantity <n>] [--ref <reference>] [--format text\|json]` | Creates an item. The program assigns the code. Run twice, it creates two | Write |
| `item set <sku> --quantity=<n> [--if-version <n>] [--yes]` | Sets a quantity. Prints `updated SK-1001 quantity=12` | Write |
| `item remove <sku> [--yes]` | Removes an item. Prints `removed SK-1001` | Write |
| `catalog push [--yes]` | Stands in for sending outside: records a push. Prints `pushed 3 items` | Cannot be taken back |
| `report stock [--rows <n>] [--pause-ms <n>]` | A report printed row by row on standard output, with a progress line for each row on standard error | Read |
| `login [--password-stdin]` | Reads a password from standard input and keeps a session in the home folder | Session |
| `echo [<anything>...]` | Prints the arguments and the whole environment it received, and its working folder, as JSON | None |

An option that takes a value accepts `--name value` and `--name=value`. Everything after `--` is a positional argument. An option the command does not know is bad input.

**Typed arguments.** An item code is two capital letters, a dash, and four digits. A location code is a capital letter and two digits; only `A01`, `A02`, and `B01` exist. A quantity is a whole number from 0 to 9999. A name is 1 to 60 characters with no control characters. A reference is 1 to 64 letters, digits, dots, underscores, colons, or dashes.

**Prompts.** `item set`, `item remove`, and `catalog push` print `Are you sure? [y/N] ` on standard output, with no line end, and read one line from standard input, unless given `--yes`. Anything but `y` or `yes` changes nothing. `login` prints `Password: ` on standard error unless given `--password-stdin`.

**Exit statuses.** Messages go to standard error as `example-cli: error: ...`.

| Status | Meaning |
| --- | --- |
| 0 | Done |
| 1 | The program itself failed |
| 2 | Bad input, or a question not answered with yes. Nothing changed |
| 3 | No such item. Nothing changed |
| 4 | Refused by a rule: a quantity over 9999, a location that does not exist, a wrong password. Nothing changed |
| 5 | The item does not have the version `--if-version` named. Nothing changed |
| 6 | Another start of the program is writing. Nothing changed |
| 75 | Try again later. Only a fault produces it |
| 77 | Not signed in. Only when the session fault is on |

**How a write happens.** The program takes an operating-system lock on the file `lock` in the home folder, reads the data file, writes the new contents to `items.json.tmp`, renames that over `items.json`, and releases the lock. The rename is the commit. The record of the write is in the same file, so a write and its record are committed together. A program that is killed loses its lock at once, because the system holds it, not a file's presence.

## The harness

```rust
use example_cli::harness::Stand;
use example_cli::faults::{Fault, FaultKind, StopHow, StopPoint};

let stand = Stand::start()?;
let finished = stand.run(&["item", "set", "SK-1001", "--quantity=12", "--yes"])?;
assert_eq!(finished.code, Some(0));
let stored = stand.control().stored_item("SK-1001")?;      // read from the data file
let count = stand.control().write_count("SK-1001")?;       // 1
let log = stand.control().invocations()?;                   // log[0].args, in order
```

| Call | What |
| --- | --- |
| `Stand::start()` | A new stand, with the binaries found or built. The call for a test in any crate |
| `Stand::start_with(Programs)`, `Stand::start_in(Programs, parent)` | The same with binaries the caller names, and in a folder the caller names |
| `Programs::built()` | Finds or builds both binaries; kept for the rest of the process |
| `Programs::at(first, rebuilt)` | Binaries the caller already has |
| `stand.program()` | The path of this stand's own program. Give it to the pack as the surface's path |
| `stand.home()`, `stand.root()` | The home folder, and the stand's own folder |
| `stand.command(args)` | A `std::process::Command`: the arguments, an environment holding only `EXAMPLE_CLI_HOME`, the stand's folder as working folder |
| `stand.run(args)`, `stand.run_with_input(args, input)` | Runs to the end and returns `Finished { code, stdout, stderr }` |
| `stand.replace_program(Build::Rebuilt)` | Puts the other build at the program's path: same behavior, same version, other bytes |
| `stand.control()` | The control interface, below |

**How a pack's tests get the binary.** A pack adds `example-cli` under `[dev-dependencies]` and calls `Stand::start()`. Cargo does not build a dependency's binaries for a test, so `Programs::built()` runs `cargo build --package example-cli --bins --message-format=json`, with the cargo that is running the tests, and takes the two paths from what cargo reports. Nothing assumes a file suffix or a folder layout. When nothing changed this takes a fraction of a second, and it happens once in a test process. A test run that must not start a build sets `EXAMPLE_CLI_BIN_DIR` to a folder that holds both binaries. This crate's own tests pass `env!("CARGO_BIN_EXE_example-cli")` and `env!("CARGO_BIN_EXE_example-cli-rebuilt")` to `Programs::at`.

**A stand's folder** is made in `example-cli-stands/` under the system's temporary folder, on every system. The engine refuses a program another user could replace, and the folder a build writes to need not be closed to other users: on a Windows machine whose checkout is on a drive other than the system's, as on a hosted CI runner, every signed-in user may change it. The user's temporary folder is closed to other users on Windows and macOS, and on Linux has the sticky mark, where only an entry's owner may replace it. On Linux and macOS the program is placed there as a link to the built binary, or a copy where a link cannot be made; on Windows always as a copy, because a link there shares the built file's access list.

```text
<root>/example-cli            the program (with the system's suffix)
<root>/example-cli-control/   the control folder
<root>/home/                  the home folder
```

## The control interface

`stand.control()` reads the program's files directly. It never runs the program.

| Call | What |
| --- | --- |
| `reset()` | Restores the seed items; forgets every write, start, and fault; ends the session and any child process |
| `stored_item(sku)`, `stored_items()` | Exactly what the data file holds |
| `writes()`, `write_count(sku)` | Every committed write since the reset: which start created, updated, or removed which item |
| `pushes()` | Every committed catalog push |
| `uncommitted_write()` | Whether a write got as far as its temporary file and has not committed |
| `invocations()` | Every start since the reset, oldest first: `start`, `build`, `command`, `args` in order, `env`, `cwd`, `home`, `exit` (none if it was killed or ended abnormally), `hung` |
| `set_fault(&Fault)`, `faults()`, `clear_faults()` | Switch a fault on, list those still on, switch all off |
| `wait_until_hung(count, within)` | Waits until that many starts have reached a hang, and returns their numbers |
| `living_children()`, `stop_children()`, `wait_until_no_children(within)` | Which starts' child processes are alive now, and ending them |
| `hold_write_lock()` | Takes the write lock, so that every write answers busy until the returned value is dropped |
| `signed_in()`, `expire_session()` | Whether a session is kept, and ending it |
| `for_home(path)`, `Control::at(control_folder, home)` | The same control folder over a home folder the stand did not make |

**How the program finds the control folder.** As the folder `example-cli-control` beside its own executable, or else by the variable `EXAMPLE_CLI_CONTROL`. The first way needs nothing in the environment, so a start with a bare environment is still recorded and still takes its faults. With neither, the program records nothing and has no faults. The program records each start before it does anything else.

**A home folder the stand did not make.** When the product names the home folder itself, the invocation log and the faults are still the stand's, because they belong to the program's path. Ground truth for that folder comes from `stand.control().for_home(path)`.

## Faults

A fault is a setting: `Fault::once(kind)` for the next start it applies to, `Fault::until_cleared(kind)` until cleared, and `.on("item set")` or `.on("item")` to limit it to a command or a group. A fault that names no command applies to every command except help and version, so a version check before a command does not use it up; `ReportVersion` with no command applies to the version command only.

| `FaultKind` | The program does this |
| --- | --- |
| `Delay { ms }` | Answers correctly after waiting |
| `Stop { at, how }` | Stops at `Start`, `BeforeCommit` (the temporary file is written, the lock is held, nothing is committed), or `AfterCommit` (committed, lock released, nothing printed). `how` is `Hang` (waits to be killed), `Exit { code }`, or `Abort` (ends by a signal where there are signals, with a crash status elsewhere) |
| `Drift` | Prints every output form differently: `qty` for `quantity`, another table column, other wording for results. The version is unchanged |
| `Prompt { text }` | Asks a question on standard output before anything else, on any command, whatever its flags. `FaultKind::prompt()` asks `Are you sure? [y/N] ` |
| `Flood { stream, bytes }` | Prints that many bytes of filler lines on one stream first |
| `Say { stream, text }` | Prints that text on one stream first, exactly as given. A test puts a made-up secret at a chosen place in the output with it |
| `RoundQuantity { multiple }` | Stores a quantity rounded down, and prints the quantity asked for |
| `UppercaseName` | Stores a new item's name in upper case, and prints it as given |
| `ReportVersion { version }` | Reports another version |
| `TabbedHelp` | Sets out each listing of commands in help with a tab between a name and its text. Name `help` with `.on("help")` |
| `UnnamedHelpLine` | Adds `  [options]` and a text to the overview's listing of commands, a line that holds no command name. Name `help` with `.on("help")` |
| `SilentMiss` | Answers `item show` of a code it does not hold with no output and status 0 |
| `InterfaceLine` | Prints a second line after the version: `example-cli interface 7` |
| `NestedJson` | Prints `item list --format=json` as one document, `{"header": {"count": n}, "items": [...]}` |
| `RequireSession` | Refuses every data command with status 77 unless signed in, by `login` or by the variable `EXAMPLE_CLI_TOKEN` |
| `LeakSecret` | Prints the password `login` read, and the token variable, on both streams |
| `SpawnChild { lifetime_ms }` | Starts a child process that keeps the program's output streams and outlives it |

The faults the build plan asks of every write, by name:

| Fault | Setting |
| --- | --- |
| A slow answer | `Delay` |
| A failure exit after the write was committed | `Stop { at: AfterCommit, how: Exit { code: 75 } }` |
| A write that commits and then hangs | `Stop { at: AfterCommit, how: Hang }`, then kill the process |
| A write killed before commit | `Stop { at: BeforeCommit, how: Hang }`, then kill the process; or `how: Abort` |
| Changed output format | `Drift` |
| An unexpected prompt | `Prompt` |
| A blocked surface | `hold_write_lock()`, or `Stop { at: Start, how: Exit { code: 6 } }` |
| An expired session | `RequireSession` with `expire_session()` |

## The cases

The numbers are those of the stand in spec section 306.

| # | Case | How the stand shows it |
| --- | --- | --- |
| C1 | Each output form | `item show --format=json`, `item list`, `item set` |
| C2 | Arguments and environment arrive exactly | `echo`, and `invocations()` for the same facts without the program's output |
| C3 | A write and its read-back | `item set` and `item show`; proven by `stored_item` and `write_count` |
| C4 | A stored value differs | `RoundQuantity`, `UppercaseName` |
| C5 | Commits, then hangs | `Stop { AfterCommit, Hang }` and `wait_until_hung` |
| C6 | An assigned code, with and without a reference | `item add` with and without `--ref`; `item list --ref` |
| C7 | A duplicate when run twice | `item add` twice |
| C8 | A refusal with a message | Status 4 and a message on standard error |
| C9 | A question unless `--yes` | The three writes that ask; `Prompt` |
| C10 | An undeclared status, an end by signal | `Stop { how: Exit { code } }`, `Stop { how: Abort }`, or kill a hung start |
| C11 | Slow output, and more than the ceiling | `report stock --rows --pause-ms`; `Flood` |
| C12 | Another version, another digest | `ReportVersion`; `replace_program(Build::Rebuilt)` |
| C13 | The injected secret printed, text aimed at an agent | `LeakSecret`; the note of seed item `SK-1003` |
| C14 | A child that outlives the program | `SpawnChild` and `living_children` |

## Seed data

| Code | Name | Quantity | Location | Note |
| --- | --- | --- | --- | --- |
| `SK-1001` | Hex bolt M8 | 120 | A01 | |
| `SK-1002` | Washer 8 mm | 4000 | A01 | |
| `SK-1003` | Étiquette « fragile » | 15 | B01 | Text aimed at an agent |

The next created item is `SK-1004`. The one password is the constant `example_cli::PASSWORD`; it is synthetic and guards nothing.

## Not built

- A secret passed through an inherited handle. The program takes a secret on standard input and in a variable.
- A command that writes a file for the pack to collect, or reads a file it is given by path.
- A program that ignores an option it does not know, and one that prints in an encoding other than UTF-8.
- A program on Windows that parses its command line by its own rule.
