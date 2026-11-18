# 0271. The check of programs the product starts, after a second reader

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-18

**Changes:** the provider contract's check of a program and its supervisor (`crates/a2m-provider/src/supervisor/`), and the design of [`a2m-provider`](../architecture/crates/a2m-provider.md). **Builds on** [decision 0250](0250-one-check-of-who-may-change-a-program-and-the-mcp-packs-local-server-on-windows.md) and [decision 0216](0216-the-command-pack-on-three-systems-the-administrators-group-and-a-script-the-target-keeps.md). **Answers** section D of the sixth safety audit (findings D1 to D8). No public signature changes: every function, type, and variant keeps its name and shape; what is refused grows.

## Context

A second reader went through the check every pack calls before it starts a program (`check_program`), the two closed lists beside it (`is_shell_or_interpreter`, `redirects_loading`), and the supervisor's start. Each finding was checked against the code before anything changed; all eight hold. A CI run on Windows had meanwhile shown that the walk refuses a program on a drive other signed-in users may change, and that the user's temporary folder passes it.

## Decision

**D1. The list of loading variables grows by family, and a `PATH` is checked folder by folder.** The list stays closed and kept in one place, and each family carries its reason beside it in the code. Added, from the documentation of the interpreters of the closed table, the runtimes they run on, and the libraries programs link with:

- Windows PowerShell's runtime, the .NET Framework, and .NET: every variable beginning `COMPLUS_`, `DOTNET_`, `COREHOST_`, and `APPDOMAIN_MANAGER_` (the application domain manager the .NET Framework loads into every program from `APPDOMAIN_MANAGER_ASM` and `APPDOMAIN_MANAGER_TYPE`), and `DEVPATH`; `PSMODULE` as a prefix in place of the one name.
- AppleScript's `osascript`: `OSA_` (its script libraries); and on macOS `__XPC_` (loader variables handed on by the service launcher) and `__PYVENV`.
- The shells: `IFS`, beside the names already there.
- The C library of Linux: `GLIBC_TUNABLES`, `LOCPATH`, `NLSPATH`.
- Any program: `OPENSSL_` (a configuration names engines and providers loaded as shared objects); `COMSPEC` and `SHELL` (what a program runs as "the shell"); helper commands run by a variable (`PAGER`, `MANPAGER`, `LESSOPEN`, `LESSCLOSE`, `EDITOR`, `VISUAL`, `BROWSER`, the password prompts, and the version control tool's commands and `GIT_CONFIG` settings); module paths of the desktop and media libraries (`GTK_`, `GIO_`, `GST_PLUGIN`, `VK_`, `QT_PLUGIN_PATH`, `QT_QPA_PLATFORM_PLUGIN_PATH`, `SASL_PATH`, `SASL_CONF_PATH`, `LIBGL_DRIVERS_PATH`); `ELECTRON_RUN_AS_NODE`; `TCL_LIBRARY`, `TK_LIBRARY`; `R_HOME` and the prefixes `R_PROFILE`, `R_ENVIRON`, `R_LIBS`; `ERL_`, `ELIXIR_`, `JULIA_`; Windows's `__COMPAT_LAYER`.

The list is closed and it is not complete: any program can read any variable as the name of code to load. New names come from an interpreter's or a runtime's own documentation, or from a finding, and each comes with a test row.

`PATH` is not refused by name. On Windows it decides where the system loads the libraries a program asks for by name, and on every system it decides which program a program starts by name. Three handlings were weighed. **Never passing it** breaks every program that starts a helper by name. **Always setting it to the system's own folders** gives a program a search path its target's files do not show, and still breaks helpers elsewhere. **Checking it** was chosen: the supervisor holds each folder of a `PATH` the launch passes or sets to `check_program` as a `Folder`, the rights of a folder that holds a program, so that nobody but the engine's user and the system may add a file to it; a folder that is not there is held by its nearest folder that is, so that nobody else can make it; a name that is not absolute, and on Linux and macOS an empty one, which stands for the working folder, is refused. A refusal is `policy`/`variable_refused`, naming the folder, and nothing starts. The check uses no group beyond the system's own, since a launch carries no `ProgramTrust` (Not yet: a field would change every pack's launch). A launch that gives no `PATH` gives none, as before: on Windows the system then searches the program's folder, its own folders, and the working folder, which D5 makes private.

**D2. Launchers are refused by kind, and a version does not hide a name.** Added to the name list, each kind with its reason: language launchers, package runners, and version managers (`py`, `pyw`, `uv`, `uvx`, `pipx`, `npx`, `npm`, `pnpm`, `yarn`, `bunx`, `dotnet`, and others), container, sandbox, and compatibility runners (`docker`, `podman`, `flatpak`, `wine`, and others), build tools (`make`, `cargo`, `go`, `gradle`, `msbuild`, and others), more interpreters (`julia`, `escript`, `swift`, and others), programs that start the program their arguments name (`open`, `arch`, `screen`, `gdb`, `xdg-open`, and others), the Windows programs that load or run what their arguments name (`regsvr32`, `msiexec`, `installutil`, `cmstp`, `conhost`, and others), and the dynamic loaders of Linux by the beginning of their names (`ld-linux`, `ld-musl`). A name of two letters or more followed by a digit or a dot and anything after it is that name (`python3-dbg`); a name followed by a letter or a hyphen is not (`python-example-server`).

The list is a list of known names and can never be complete. For a program whose name is not on it, two things still hold: the digest pins the bytes of the program that starts, and its arguments are the fixed ones in the target's files, shown at review, with every typed value given as one element of data. What such a program loads or fetches because of those fixed arguments is not pinned. The line of decision 0250 point 3 for the MCP pack stands, and now covers the known launchers too: a server that a launcher starts is refused until the fixed pair, which pins the script, is built for it. Showing an unknown program at review as "runs code the digest does not pin" cannot be done by name, and was not built.

This narrows the command pack too: a target that wraps a build tool or a container runner as its program is refused, because what runs is the project's or the image's code, which nothing pins (Choice, this record).

**D3. Access lists beyond the mode.** On macOS the walk now reads each folder's, link's, and the file's extended access list through the system's list functions, with one `unsafe` block per call, each with its soundness comment, and a test on macOS that adds entries with `chmod +a`. Whom an entry names and which rights it grants are not read: the first needs the membership service, and the second rests on bit values that could not be checked against a running system. So an entry that denies is passed over, which refuses more, never less (the system puts such entries on a user's own folders); an entry that allows, whatever it grants, is `Unreadable`; so are an entry of another kind and a list that cannot be read. The system answers "no such file" both for a file with no list and for an empty one; the walk has just found the file, so that answer is a list with no entry. On Linux the group's bits of the mode bound what a named user or group of a POSIX list may hold, so a list adds nothing except where a group's write was accepted because `ProgramTrust` holds the group; there, a list present is `Unreadable`.

**D4. A fixed local disk, by a letter that stands for a whole volume.** Before the Windows walk reads anything, `GetDriveTypeW` must say the root is on a fixed disk, and `QueryDosDeviceW` must say the letter stands for a device by its name alone (`\Device\HarddiskVolume3`). A letter mapped to another machine's share, a removable or optical disk, and a letter made with `subst` or defined to stand for a path below a device, are `Elsewhere`. Walking the real path behind a substituted letter was weighed and not chosen: one rule, refuse, leaves no second way of resolving a path to get wrong.

**D5. A working folder of the program's own.** A launch with no working folder now runs in a folder made for that start: in the system's temporary folder, under 128 random bits, mode `0700` on Linux and macOS and the private access list on Windows from the moment it exists, and held to `check_program` as a `Folder` before anything starts in it. A one-shot run removes it after it has reaped the program; a link's entry holds it until the link is stopped or the supervisor shuts down; removal runs off the async threads. Where removal fails, as it can on Windows while a process the job did not end has it open, a private folder is left behind. The program's own folder was weighed and not chosen: a program writes into its working folder, and its own folder is not the engine's to write.

**D6. A program begins as this system's programs do.** `Program` and `Interpreter` now refuse a file whose first bytes are not those of this system's programs: an executable and linkable file on Linux, a Mach-O file (thin in either byte order, or universal) on macOS, the DOS header on Windows. A text file without `#!` is a script, which the C library hands to the shell when the system will not run it. The refusal is `Script`.

**D7. A walk before every start in the command pack: not done here.** The command pack walks a program's folders once, when a session is prepared. Walking again under the start lock before each start needs a change in that pack's driver beyond the lines this package may touch; it is owed to the command pack.

**D8. A foreign link's pipe ends on Windows: not done.** A foreign link marks its two child ends inheritable for its own start, under the supervisor's start lock; a process another part of the engine starts in that moment would inherit them. The fix the finding names, an explicit list of the handles a child inherits, needs `PROC_THREAD_ATTRIBUTE_HANDLE_LIST`, which the standard library offers only behind an unstable feature; one lock for every start in the engine crosses four crates. It stays open. Links over standard streams, which the MCP pack uses, are not touched by it: the standard library makes those copies inheritable itself.

## Tests

Each fails before its fix, where this machine can run it:

- D1: `the_variables_the_interpreters_and_their_runtimes_load_code_from_are_known` (unit), and `a_path_that_names_a_folder_others_may_change_is_refused_and_nothing_starts` in `tests/supervisor.rs`, which plants a `PATH` folder everyone may write, a missing folder under it, and a relative name, and asks the stand that nothing ran.
- D2: `launchers_package_runners_containers_and_build_tools_are_known_by_name`.
- D3: `a_named_users_write_behind_a_trusted_groups_bits_is_refused_on_linux` writes a POSIX list in the kernel's own form; `an_entry_that_allows_is_refused_and_one_that_denies_is_not` runs on macOS only.
- D4: `a_substituted_drive_letter_is_refused_and_a_fixed_local_disk_is_not` runs on Windows only.
- D5: `a_program_with_no_working_folder_works_in_a_private_one_that_is_removed_after` starts the shell (the command interpreter on Windows), which prints its working folder and, on Linux and macOS, its mode.
- D6: `a_file_that_does_not_begin_as_a_program_of_this_system_is_refused_as_a_script` and, on Windows, `a_file_without_the_header_of_a_windows_program_is_refused_as_a_script`.

The command pack's table test of the old and new checks writes this system's header on macOS, as the new check asks.

## What only a run on macOS or Windows can prove

1. `acl_get_link_np`, `acl_get_entry`, `acl_get_tag_type`, and `acl_free` are in the system's library under those names with those types; `ACL_TYPE_EXTENDED` is `0x100` and `ACL_EXTENDED_DENY` is 2; a file with no list gives a null list and "no such file".
2. `chmod +a "everyone allow write"` and `"everyone deny delete"` add entries the check reads as an allowing and a denying entry.
3. No folder on the way to the temporary folder, to the build folder, or to a runner's home on macOS holds an entry that allows; otherwise every program there is refused.
4. `GetDriveTypeW` answers 3 for the system drive of a runner, and `QueryDosDeviceW` gives `\Device\` and one name for it, and `\??\` and a path for a letter made with `subst`.
5. `subst` runs without elevation on a runner and frees its letter with `/D`.
6. The system's folders on Windows (`System32`) pass the walk as a `Folder`, and a folder given everyone's write by `icacls` does not.
7. A folder made in the temporary folder with the private access list passes the walk, and the command interpreter started there with `/d /c cd` prints it.
8. The macOS shell's `ls -ld .` prints `drwx------` for a folder of mode `0700`.

## Consequences

- More programs and more variables are refused, on every system, by both packs that start programs. A target that passes a `PATH` naming a folder another user may change no longer starts. On Windows that includes many folders made directly under a drive's root, which give every signed-in user the right to add a file; that is the case the check is for.
- Every start without a working folder makes and removes one folder.
- `SCOPE.md` gains a row for the programs refused by kind.
