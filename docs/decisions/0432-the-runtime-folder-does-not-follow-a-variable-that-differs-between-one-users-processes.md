# 0432. The runtime folder does not follow a variable that differs between one user's processes

**Status:** Accepted. Design decision, made in the design review round by lens 13, the other operating systems ([report](../reviews/other-operating-systems-review-2026-11-24.md), finding 1). The owner had not read this record when it was written. It changes where `Paths::resolve` of `a2m-actions` puts the runtime folder on macOS and, where the variable is missing, on Linux, and what a client says when it is refused `engine_running`. It changes no public interface. Built by roadmap package OS1; until then the code is as before.

**Date:** 2026-11-24

**Changes:** `a2m-actions` (`src/paths.rs`); `a2m-local` (the engine lock and the client's refusal); [section 222](../spec/2-kernel/222-front-doors-and-the-local-engine.md); the technical reference's on-disk layout; the designs of `a2m-local` and `a2m-actions`, once OS1 is built.

## Context

The local channel's socket, the engine lock's first file, and the start lock are in the runtime folder. `Paths::resolve` takes that folder from the environment of the process that asks: on Linux `XDG_RUNTIME_DIR`, else `run` in the data folder; on macOS `TMPDIR`, else `run` in the data folder; on Windows `LOCALAPPDATA`, which every process of a signed-in user has.

Two facts make this weaker on macOS and Linux than it looks.

1. **The variable is not the same in every process of one user.** A program started from a desktop session has it; one started by a scheduled job, a service manager, a remote shell, or a program that clears its children's environment may not. Decision 0117 saw this, and made the engine lock two locks so that two runtime folders never lead two engines to one store. That keeps the store safe. It does not let the second client reach the engine: it finds no socket in its own runtime folder, starts an engine, and that engine is refused at the data folder's lock with `busy`/`engine_running`. The fix the refusal gives, "use the running engine, or stop it with `anything2mcp engine stop`", cannot be followed from that process, since both reach only its own runtime folder. An agent whose host starts it with a trimmed environment has no engine at all while another is running.
2. **On macOS, `TMPDIR` is a folder the system cleans.** It is the user's temporary folder under `/var/folders`, and macOS's own cleaning of temporary folders removes entries left untouched for days. Connecting does not touch the socket's file. An engine left running with `engine.idle_stop_minutes` at 0, or held open by a session keeper, can lose its socket and its runtime lock file while it runs; a client then finds nothing, and is refused as above. This is the system's documented habit, not measured here.

Windows is not affected: the pipe's name is made from the user's identifier and the runtime folder's path, and `LOCALAPPDATA` is the same in every process of the user.

## Decision

1. **On macOS the runtime folder is `run` in the data folder,** `~/Library/Application Support/anything2mcp/run`, whatever `TMPDIR` says. It depends only on `HOME`, as the data folder does, and the system does not clean it. The socket's path stays within 103 bytes for any home folder whose path is at most 46 bytes (`/Users/` and a name of 39); a longer one is refused `input`/`socket_path_too_long` as today, and `A2M_HOME` is the way out.
2. **On Linux, when `XDG_RUNTIME_DIR` is not set, the runtime folder is `/run/user/<uid>/anything2mcp` when `/run/user/<uid>` is a folder this user owns that no other account may enter,** which is the folder the system's session manager makes and the variable would have named; only when there is no such folder is it `run` in the data folder. A process with the variable and one without then find the same socket on any system with a session manager.
3. **The engine writes the runtime folder it serves into the data folder,** in a file beside the data folder's lock, written whole and readable by its user alone. A client refused `engine_running` whose own runtime folder holds no engine reads it and names that folder, and the variable that leads to it, in the refusal's fix. It never connects there: the file names a place in a message, and no file or input decides where a request goes ([section 502](../spec/5-security/502-defending-against-a-manipulated-agent.md)).
4. **Nothing else changes.** The two locks stay, the socket's checks stay, and `A2M_HOME` still sets every folder.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Keep the variables and say in the documents that a process without them is not served | True and useless: the person does not choose the environment an agent's host gives its children |
| The runtime folder always in the data folder, on Linux too | A home folder on a network file system cannot hold a socket, which is why the runtime folder exists on Linux. On macOS the home folder is local in the first release's case, and the data folder already holds the store |
| On macOS, the system's own per-user temporary folder asked of the system (not of `TMPDIR`) | The same in every process, but still a folder the system cleans: it answers fact 1 and leaves fact 2 |
| The client connects to the folder the data folder names | A file in a folder decides where a request goes, against section 502; naming it in the fix gives the person the same help with no new route |
| The engine notices its socket gone and makes it again | Answers fact 2 alone, and adds a moment in which no engine answers and a client starts another. Moving the folder removes the cause |

## Proof

Roadmap package OS1:

- `Paths::resolve` with a macOS environment, with and without `TMPDIR`, gives the same runtime folder, `run` in the data folder; a test of the paths module, which reads an environment it is given and so runs on every system.
- With a Linux environment without `XDG_RUNTIME_DIR`, given a `/run/user/<uid>` stand-in folder that is the user's and closed, the runtime folder is inside it; given one open to others, or none, it is `run` in the data folder. The same test style.
- On Linux and macOS CI: an engine is started by a command with the variable set; a second command with the variable removed is answered by the same engine, or, where the folders still differ (no session folder), is refused `engine_running` with the first engine's runtime folder named in the fix.

## What would reopen it

A home folder on a network file system on macOS, which would need the runtime folder elsewhere again; or a session manager on Linux that puts the user's runtime folder somewhere other than `/run/user/<uid>`.
