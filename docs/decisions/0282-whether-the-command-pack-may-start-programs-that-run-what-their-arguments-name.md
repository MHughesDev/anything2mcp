# 0282. Whether the command pack may start programs that run what their arguments name

**Status:** Proposed. A design decision proposed by an agent on 2026-11-18. It asks whether to widen what [decision 0271](0271-the-check-of-programs-the-product-starts-after-a-second-reader.md) refuses, and it is put to the owner because it weighs a safety line against the North Star ([decision 0277](0277-the-north-star-an-agent-uses-the-software-a-person-already-has.md)), which is the owner's. Decision 0271 is in force until this record is accepted. The owner has not read this record.

**Date:** 2026-11-18

## The question

Decision 0271, point D2, refuses by name, as the program of either pack that starts programs, language launchers, package runners, version managers, container and sandbox runners, build tools, and programs that start the program their arguments name. The reason it gives is that what runs is the project's or the image's code, which nothing pins (a Choice). Decision 0277 measures work by how much software it brings within an agent's reach, how little a person must do, and how far the tools can be trusted. Should the command pack reach such programs in a form that keeps what can be pinned pinned, and says plainly what cannot?

## What a person loses today

The refusal is by the program's name, so it takes every command of such a program, including the ones that run no code but the program's own. Today a person cannot give an agent a tool over:

- **A container or sandbox runner,** not even to list what is running or to read a container's state, though those commands run none of an image's code.
- **A build tool or a task runner,** to run the tasks a project already declares for itself: its tests, its lint, its build.
- **A language launcher,** to start a program written for a runtime and shipped as a file the runtime loads, such as an application's main library.
- **A version manager or a package runner,** to list what is installed, or to run a tool the person already uses by its package's name.
- **A program that opens what its argument names,** such as the system's own opener for a file or an address.

Each is software already on the person's machine, which the person already uses, and which offers agents no tools. The MCP pack's servers that are started through a launcher are refused for the same reason (decision 0250, point 3).

## The form that could bring them back

The command pack already gives every program a fixed argument list declared in the target's files, shown at review, with each typed value given as one element of data ([section 306](../spec/3-packs/306-the-command-pack.md)). For these programs four things would be added, all of them required:

1. **The fixed arguments are the operation.** Each operation names its subcommand and every fixed argument in the target, so that what the program is asked to do is in the reviewed files; the typed values are data only, and no value may stand where the program reads the name of a thing to run.
2. **The program's own files are pinned by digest where they are files.** The task file a build tool reads, the file a container runner composes services from, a language launcher's application file, the configuration a runner reads: each is named by its path inside the target's declared folders and checked by digest at every start, as the script of a fixed pair is checked now. A launcher in its one form, with the file it loads pinned, is a new row of the closed table of interpreter forms, proven by a test as every row is.
3. **Every such operation declares its effect and its hazard.** An operation that runs code the target does not pin has the hazard `sends_outside` unless the developer shows otherwise, and the publish page says for each one which code it runs that nothing pins.
4. **A person approves anything that changes the system,** on the engine's own page, as for any write; a read that runs no other code needs no more than any read.

## What would still be impossible to pin

- **An image's code.** An image can be named by its digest, which pins its bytes; what those bytes do when they run is the image's, and nothing here reads or proves it.
- **A project's code.** A pinned task file still runs the compilers, scripts, and tests the project holds, and those change whenever the project does. Pinning every source file would pin a snapshot no one wants to run.
- **What a package runner fetches.** It may fetch code at the moment it runs, from a registry, unless the version and its digest are given and the runner honours them.
- **What any of them starts in turn,** by name, through the search path, beyond what the supervisor's check of the search path's folders already holds (decision 0271, D1).

So such a tool can be trusted to start what was reviewed, with the files that were reviewed, under a person's approval for a change. It cannot be trusted to do only what its review shows.

## Recommendation

Widen, in two steps, each its own package with its own record.

1. **First, the commands of these programs that run no code but their own:** a container runner's listings and inspections, a version manager's listings, a launcher's version. They are reads of the program itself, pinned by the program's digest like any program, and they bring the most within reach for the least risk. A closed list per kind of program says which subcommands run no other code; a subcommand not on it stays refused (rule 7).
2. **Then, the form above for the commands that run other code,** with launchers first, since a launcher with its file pinned is the fixed pair the product already builds, and build tools and container runners after, with their files pinned and every run that changes the system approved by a person.

Package runners that fetch at run time stay refused until a run can be held to a version and a digest the runner is proven to honour.

## Alternatives weighed

- **Keep the refusal.** Safe, and it leaves whole kinds of software a person uses daily out of an agent's reach, which decision 0277 counts against it.
- **Refuse by what a command does, not by the program's name.** That is the form recommended above for its first step; it needs a list per kind of program, which can be wrong, so a subcommand not on it is refused.
- **Allow them with a warning only.** A warning at review is not a guarantee, and a tool whose code nothing pins would be offered as one the product verified.

## Revisit if

The owner answers; or a test stand shows that a kind of program's listing commands run other code after all.
