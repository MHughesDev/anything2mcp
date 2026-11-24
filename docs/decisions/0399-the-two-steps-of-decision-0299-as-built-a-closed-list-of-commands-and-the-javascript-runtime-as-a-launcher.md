# 0399. The two steps of decision 0299 as built: a closed list of commands, and the JavaScript runtime as a launcher

**Status:** Accepted. Design decision, made under [decision 0294](0294-the-owner-hands-the-open-questions-to-the-agents.md). It builds what [decision 0299](0299-programs-that-run-what-their-arguments-name-are-reached-in-two-steps.md) decided, in packages L1 and L2. The owner had not read this record when it was merged.

**Date:** 2026-11-24

**Changes:** [spec section 306](../spec/3-packs/306-the-command-pack.md); the designs of [`a2m-provider`](../architecture/crates/a2m-provider.md) and [`a2m-pack-command`](../architecture/crates/a2m-pack-command.md); the [technical reference](../reference/technical-reference.md); the [roadmap](../plan/roadmap.md) and [the build plan](../plan/build-order.md) mark L1 and L2 done. It narrows what [decision 0271](0271-the-check-of-programs-the-product-starts-after-a-second-reader.md), point D2, refuses, as 0299 said it would, and changes nothing else of it.

## Context

Decision 0299 decided two steps. Step one: a program the name list refuses, such as a launcher, a version manager, or a container runner, may be started in the commands of it that run no code but its own, by a closed list for each kind. Step two: a launcher given a file it loads, pinned by digest at every start, is the fixed pair the command pack already runs for a script, and becomes a row of the closed table of interpreter forms. These programs take, in their arguments, the name of other code to run, so the whole of the work is that no input chooses what runs ([spec section 502](../spec/5-security/502-defending-against-a-manipulated-agent.md)).

## Decision

1. **The list is the provider's, beside the name list.** `runs_its_own_code(path)` gives the row of a closed list `OWN_CODE` in `a2m-provider`: a kind (`Launcher`, `VersionManager`, `ContainerRunner`), the names the program is known by, and its forms. A form is the exact elements, from the first, and whether the end-of-options marker and one or more values may follow them (a container's name to inspect). It lives with the name list it qualifies, because only together do they say what a program may be started as; the command pack is its only user now. A name matches with a version of digits and dots after it (`python3.12`), never with any other suffix: a batch file, a script by its suffix, and `python3-dbg` have no row.
2. **A role of the check, `ProgramRole::OwnCode`.** In it the name list does not refuse a program whose declared name, and the name of the file its path leads to, both have a row; every other check holds, the first bytes and who may change the file among them. A name with no row is refused, a shell always.
3. **What the command pack holds a file to.** A program with a row whose files declare no interpreter is started only if every argument list its files could give it is one of the forms of its row, by its declared name and by its real name: each command's, the version check's, and each help form's. Each command is `read` and names no script, and the file gives the program no variable, set or passed, because each of these programs reads variables that name what it runs or where it reaches. A file that breaks one does not compile (`command.shell_program`, as for any program refused by its name), and a plan changed after compiling is refused before anything starts (`policy`, `program_path_unsafe`). A value can stand only after `--`, so it is never an option, and never the name of something to run: no listed form takes a name of code.
4. **What is on the list, and why some reads are not.** Each command was traced on a machine that had the program, started as the supervisor starts it, and is listed only where it started no other program. The tracing struck commands that look like reads: the Rust toolchain manager's own `--version` starts the compiler; the Python package manager's listing of interpreters starts each interpreter it finds; the container runner's help starts each of its plug-ins. The container runner's listed commands name the context `default` first, since the context a user's files name may reach the engine over a remote shell, and ask for JSON by the word `json`, never by a template. The rows: the launchers Python (`--version`, `-V`), the JavaScript runtime (`--version`, `-v`), and one other JavaScript runtime (`--version`); the version managers of Python packages (`--version`) and of Rust toolchains (`toolchain list`, `show home`); and one container runner (`--version`, and with the default context its version, its containers, its images, and the inspection of named containers and images). A second container runner is not listed: rootless, it may start the system's helpers that map user ids, and no machine here had it to trace.
5. **Step two: the JavaScript runtime is a row of the table of interpreter forms,** with no fixed argument: it takes the first element that is not an option as its file and hands every element after it, `--` among them, to the script. Its options that run or load code (`-e`, `-p`, `-r`, `--import`) are not its form, and its variables that do (`NODE_OPTIONS`, `NODE_PATH`) were already refused. Python's row (`-I`) is the other launcher; its test now runs on Windows too, where Python is installed in a folder the check accepts. The file is the target's, kept in its folder and pinned by its digest, copied to the run's private folder and checked again at every start, as for any pair: a changed file is refused at start (`drift`, `program_digest_changed`), and nothing starts.

## What is still not pinned, said plainly

- A listed command runs only the program's own code as the program ships. Its configuration, in its user's folders, can make it do more: a container runner's plug-in hooks, named in its user's settings, run a plug-in after a command. The review shows the command and the program's digest; it does not read that configuration.
- A launcher's pinned file is run as a copy, alone in the run's folder. It loads nothing beside it, and so a file that loads files of its own beside it does not run this way. What it loads from the runtime's own folders, its installed packages, is not pinned (decision 0299).
- Decision 0299, point 6 (the hazard `sends_outside` on an operation that runs code nothing pins, and the publish review naming that code) is not built here. It is package L3's, with the MCP pack and the publish page.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Keep the list in the command pack | The name list it qualifies is the provider's; a list kept apart from it can drift from it, and a role of the check that skips the name list must know which names it may skip |
| Match only the first element, the subcommand, and allow any flags after it | A container runner's global flags may follow its subcommand and name a remote shell (`-H`); only a whole form is known to start nothing |
| Allow values in any position of a listed form | A value before a subcommand can be read as an option; after `--` it is data |
| Run the launcher's file where it lies, so that files beside it load | The file's folder would then have to be closed to other users as the run's folder is, and what loads beside it is unpinned; L3 decides this for a server's installed entry file |

## Revisit if

A traced command is shown to start another program after all (strike it); a program's configuration is shown to make a listed command start one in its default setting; a field test asks for a command of another program; or the owner reverses decision 0299.
