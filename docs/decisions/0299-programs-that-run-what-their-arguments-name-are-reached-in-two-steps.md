# 0299. Programs that run what their arguments name are reached in two steps, and a server started through a launcher starts with its file pinned

**Status:** Accepted. Design decision, made under [decision 0294](0294-the-owner-hands-the-open-questions-to-the-agents.md), which hands the lead the questions that were waiting for the owner. It decides the question [decision 0282](0282-whether-the-command-pack-may-start-programs-that-run-what-their-arguments-name.md) put. The owner had not read this record when it was merged, and may reverse it.

**Date:** 2026-11-23

**Changes:** what [decision 0271](0271-the-check-of-programs-the-product-starts-after-a-second-reader.md), point D2, and [decision 0250](0250-one-check-of-who-may-change-a-program-and-the-mcp-packs-local-server-on-windows.md), point 3, refuse; the [roadmap](../plan/roadmap.md) (packages L1 to L3 are new); [the build plan](../plan/build-order.md). No code changes with this record: 0271 stays in force for each kind of program until the package that widens it merges.

## Context

The product refuses, by the program's name, to start language launchers, package runners, version managers, container runners, build tools, and programs that start what their arguments name. Decision 0282 set out what that costs and a form that could bring them back, and waited for the owner.

One cost has grown since. Most existing MCP servers are started through a launcher: a runtime given a file, or a package runner given a package's name. The MCP pack therefore cannot start most of the servers a person already has, and wrapping an existing server is one of the shortest paths to a verified tool.

## Decision

1. **Decision 0282's recommendation is taken, in its two steps.**
2. **Step one: the commands of such a program that run no code but the program's own.** A closed list for each kind of program says which subcommands those are. A subcommand not on the list stays refused. They are reads of the program itself, and the program is pinned by its digest like any other.
3. **Step two: a launcher with the file it loads pinned.** A launcher in its one declared form, given a file inside the target's declared folders, checked by digest at every start, is the fixed pair the command pack already runs for a script. It becomes a row of the closed table of interpreter forms for each launcher, proven by a test as every row is.
4. **An MCP server started that way is started.** The MCP pack starts a local server whose target declares a launcher from the table and the server's entry file, pinned by digest. The server's declared effects, a person's approval of a write, and read-back are added by the pack exactly as for any wrapped server.
5. **A package runner that fetches when it runs stays refused,** and so does a server started through one. The refusal says what to do instead: install the package, and declare the launcher and the installed entry file. Nothing the product starts fetches code at the moment it starts.
6. **Every operation that runs code the target does not pin says so.** It carries the hazard `sends_outside` unless the developer shows otherwise, and the publish review names, for each such tool, the code it runs that nothing pins: the packages the pinned file loads, and whatever those start.
7. **Build tools and container runners that run other code wait.** They come after launchers, each with its own record, when a field test asks for one.

## What is still not pinned, said plainly

A pinned entry file loads other files: its packages, and theirs. The digest holds the file that was reviewed and nothing it loads. So such a tool can be trusted to start what was reviewed. It cannot be trusted to do only what its review shows, and point 6 makes the review say that.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Keep refusing | The MCP pack then reaches few of the servers people have, and whole kinds of software a person uses daily stay out of reach ([decision 0277](0277-the-north-star-an-agent-uses-the-software-a-person-already-has.md)) |
| Allow package runners with a warning | A warning is not a guarantee, and code fetched at start is code nobody reviewed |
| Pin every file the entry file loads | A package tree is thousands of files that change with every install; the pin would break on each update and teach people to re-pin without looking |
| Run such a server in a sandbox the product builds | A better answer in the end, and a large piece of work on three operating systems. Revisit after the first release |

## Revisit if

The owner reverses it; a field test shows a launcher's form that the table cannot state; or a sandbox for started programs is built, which would let point 5 be reopened.
