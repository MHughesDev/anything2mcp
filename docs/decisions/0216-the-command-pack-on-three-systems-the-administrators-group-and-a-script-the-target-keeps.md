# 0216. The command pack on three systems: the check on Windows, the administrators' group, and a script the target keeps

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-18

**Accepts:** parts 1 and 6 of [decision 0214](0214-ways-through-the-refusals-that-keep-a-person-from-their-own-machine.md), by building them with the tests that part names. Part 5, elevation, is not taken here. **Changes:** [section 306](../spec/3-packs/306-the-command-pack.md), [decision 0140](0140-the-command-pack-after-its-safety-audit.md) finding C5 (a folder a group can write), and the [crate design](../architecture/crates/a2m-pack-command.md), items 8 to 10 of its build order. **Is** the record rule six asks for a new use of another language: a script, in any language its interpreter reads, that a target keeps and the command pack starts.

## Context

The coverage review ([findings 2 and 4](../plan/coverage-review-2026-11-18.md)) found a safety gap and two limits. On Windows the check of who can replace a program resolved the path and refused nothing, so the refusal section 306 states did not hold there. On every Unix system a folder a group can write was refused whatever the group, which on macOS is believed to refuse every program inside the folder of installed applications. And the fixed pair of an interpreter and a script, the cheapest route to the operating systems' scripting interfaces and to PowerShell, was designed in one sentence and not built.

## Decision

**1. On Windows the check reads the owner and the access list of the file and of every folder on the way.** The owner must be the engine's user or one of the system's own identities, a closed list: the local system account (`S-1-5-18`), the built-in Administrators group (`S-1-5-32-544`), and the trusted installer service. Any other identity that holds a right that changes what the declared path names is another user, and the program is refused as `policy`/`program_path_unsafe`, as on Unix. Which rights those are depends on the place:

| Place | Rights refused to another identity |
| --- | --- |
| The file | Write, append, write extended attributes or attributes, delete, change the access list, take ownership, generic write, generic all |
| The folder that holds it | Add a file or a folder (the system loads a program's libraries, and reads a folder that redirects them, from beside it), delete what it holds, and the four rights over the folder itself: delete, change the access list, take ownership, generic all |
| A folder above | Delete what it holds, and the same four rights over the folder itself |

An entry that only denies is not read: it can only take a right away, so leaving it out refuses more, never less. An entry that applies only to what is made inside is not read. An allowing entry of any other kind, a list that cannot be read, and an object with no list (which allows everyone everything) are refusals. A link, a junction, or any other reparse point on the way is refused, not followed. A path on another machine's share, or a device, is refused. The code uses `windows-sys` and the calls `a2m-local` already uses for the same reading (decision 0192), and compares identities with the system's own comparison, never as text (decision 0204).

**2. A group's write counts as the system's own only for a closed list.** On macOS the group with id 80, to which the system gives its administrators, counts: its members become the superuser through the system's own prompt. On Linux no group counts by default: a group's id is not a fact about who is in it. Any other group counts only when the person names its numeric id in the new protected setting `packs.command.admin_groups` (a list; safer direction: fewer). A name is never looked up, and an entry that is not a number stops the pack's registration as `input`/`pack_settings_invalid`. The setting does not apply on Windows, where the closed list of point 1 is all. It is shown by `config.show`, as every pack setting is; the pack reports nothing to `doctor.run`, which has no place for a pack's own facts. The owner of a file is unchanged: it must still be the engine's user or the superuser.

**3. A script the target keeps runs as a fixed pair.** A `commands/` file may declare its program to be an interpreter: `program.interpreter.args` is the fixed list of arguments it is always given (for PowerShell `-NoProfile -NonInteractive -ExecutionPolicy Bypass -File`, for Python `-I`, for the shell none). Every command of such a file names `script: {path, digest}`: a path inside the target's folder, checked by the kernel's rule for a path in a target folder, and the SHA-256 of its bytes. A command without one, a script in a file with no interpreter, a fixed argument on the pack's list of flags that run their value, and help forms for an interpreter do not compile (`command.script_malformed`, `command.code_flag`). The lint `command.shell_program` no longer refuses a shell or interpreter surface on which every `commands/` file declares `interpreter`.

At each start the driver reads the script from `targets/<target>/` in the project folder of the request that started the run, through the provider's checked path; checks its digest; writes the bytes to a file in a fresh folder of the run's own under the pack's `command_runs` root; reads the copy back and checks it again; and starts the interpreter by its path, with the fixed arguments, then the copy's path, then the command's typed elements, each its own element. The interpreter passes every path check a program passes; only the name list does not refuse it. A run with no project folder stops as `input`/`project_folder_missing`; a script that is gone is `drift`/`program_missing`; one whose bytes differ is `drift`/`program_digest_changed`. Nothing starts in any of these cases.

The pack ships no list of interpreters known to pass arguments as data. The declaration states the fixed arguments, and the proof is a test per interpreter: on Linux and macOS the shell and Python, on Windows PowerShell with `-File`, on macOS `osascript`. Each test gives the script values holding the shell's and the interpreters' own syntax (`$(...)`, backticks, `;`, `&&`, `|`, both quotes, a backslash, a newline, `$HOME`, `%PATH%`, `${PATH}`, a glob, non-ASCII text, and after `--` a leading dash) and reads what the script wrote down, byte for byte.

## What the pair promises, and what it does not

The digest pins the script: the bytes reviewed at publish are the bytes started, every run. It does not make the script safe. A script is the developer's code running on the person's machine; what it does is the developer's declaration, as a program's is, and its effect class, read-back, and hazards follow the same rules as any command. The interpreter is never given text from an input to run, and no input can take the interpreter's or the script's place or become an option of the interpreter: what the script does with its arguments is the script's.

## Alternatives weighed

- **For point 1, the lead's wording taken literally: any write or append right held by anyone else, on any folder, refuses.** The root of an ordinary Windows drive lets every signed-in user add a folder, so every program on the machine would be refused. Adding an entry cannot change an entry that is already there, and every name on the way is; only the folder that holds the program is held to adding, because of the libraries a program loads from beside it.
- **For point 1, following junctions and symbolic links as the Unix walk does.** Refusing them is the stricter of the two choices decision 0214 allows, cannot be got wrong in a second resolver that has not run, and costs little: the folders programs live in hold none. A program behind an application alias is refused.
- **For point 2, naming `sudo`, `wheel`, or `staff` by name on Linux.** A name is a guess about a machine; the setting is the person's statement about theirs.
- **For point 3, carrying the script's bytes in the plan.** The plan would then pin the bytes with no file to find at run time, but the kernel's loader reads only the files of known kinds, so this needs a kernel change. Reading from the target's folder at each start, with the digest, gives the same guarantee and needs none. It knows that a target's folder is `targets/<target>` in the project, which the action registry also knows; the provider contract should hand a run its target's folder.
- **For point 3, handing the interpreter the target's own file.** It saves a copy, and leaves the script's folder to be checked for other users' writes, and the bytes to change between the check and the start. The copy is in a folder only the engine writes.

## Consequences

- The Windows code and the macOS group rule type-check and lint for both systems from Linux. None of it has run there; the next run of CI is its first test, and its tests are written for it.
- `packs.command.admin_groups` is a new protected setting. The pack's manifest digest changes.
- The compiled section gains two optional fields, `interpreter` on a program and `script` on a command, written only when present, so an existing plan's bytes do not change.
- The publish review summary has no place for what a tool starts. It should show, for each tool of the command pack, the program's path and digest, and for a fixed pair the interpreter's fixed arguments and the script's path and digest; see the report of this package.
