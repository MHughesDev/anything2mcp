# 0279. What the three surveys left open against decision 0276 is settled

**Status:** Accepted. A design decision by an agent, made on 2026-11-18; any agent may supersede it with evidence. The owner has not read this record.

**Date:** 2026-11-18

**Refines** [decision 0276](0276-what-the-scout-may-learn-from-a-program-itself-and-by-which-means.md), points 1, 2, 7, and 11, and **corrects** a count in its context. **Changes** the [roadmap](../plan/roadmap.md) (packages E6, X3, and X4), [SCOPE.md](../../SCOPE.md), and the places that named package E6. The evidence is [research note 30](../research/30-what-can-tell-what-a-compiled-program-does.md), section "Where the surveys differ".

## Context

Decision 0276 was written while the third of three surveys was still running. Research note 30 gathers all three, and its section "Where the surveys differ" names the points on which they do not agree. Five of those points bear on what decision 0276 builds or leaves out, and the decision did not settle them. One is a count that the decision states wrongly.

## Decision

1. **The count of interfaces that state reads.** Decision 0276 says that five published interfaces say which of a program's calls only read. The surveys name four that point 1 builds: the schemas of Windows management classes, COM type libraries, macOS scripting dictionaries, and Linux D-Bus introspection. Three of the four are read without asking the program described: a management-class schema is asked of the system's management service (that this asks nothing of the class's own provider is the survey's understanding, not verified), and a type library and a scripting dictionary are files. D-Bus introspection asks the service itself, and point 1 asks only a service already running. The other interfaces the surveys found that state reads ask a running program about itself: GraphQL introspection asks a running server, and UI Automation's read-only flag and macOS accessibility's settable attributes ask a running application, which the desktop pack's design already does. None of them is in the first set. The correct count is four, and the context of decision 0276 is read with this one.
2. **A COM type library is read from its file's bytes, never loaded by the system.** It is read by a reader of our own, or by a library taken whole and fuzzed here before it is used; `msft-typelib` is the candidate the third survey names (Apache-2.0, no dependencies, no fuzzing of its own). The system's loader (`LoadTypeLibEx`) is not used. A type library may sit inside a program file as a resource, and whether the system's loader maps such a resource as data only could not be verified; reading the bytes means that no code of the system or of the library runs on a file that may be hostile, and that the reader runs on all three systems. A type library in a form no reader here knows, such as the older SLTG form for which no reader was found, is a named stop. This settles the means that package X3 left to its plan.
3. **D-Bus introspection data is read by a small reader of our own on `quick-xml`,** the XML library already in the workspace (0.42.0). The call that asks for it is made with `zbus`, also already there, with the no-auto-start flag on every message, as point 1 says. `zbus_xml` is not added: it is not in the lock, and the reader it would replace is about three hundred lines by the survey's estimate.
4. **Of shell completion files, only fish's are read.** A fish completion file states each flag in a `complete` line of a fixed form, which can be read as text without running it. A bash or a zsh completion file is a program: its answers are worked out when it runs, often by calling the program's own help, so its text holds only what is written literally. Bash and zsh completion files are neither run nor read. Point 2's other sources (manual pages, PowerShell module manifests and help files) are unchanged. Package X4 reads fish completion lines and no zsh lines.
5. **Package E6 is withdrawn.** E6 was the optional provider for the outside project studied in [research note 16](../research/16-rea-methods.md), behind the MCP pack. Decision 0276, point 7, removed that provider from the design. The MCP pack's `discovery` mapping, by which any server a person maps gives findings at `inferred` and is never bound, is not withdrawn; it has no package of its own now. Hypothesis H-27 was to be tested by E6 and stays untested.
6. **Interception with an authority trusted by one process the person starts is left open,** under point 11 of decision 0276. Point 9 excludes a trust root for the whole system; the first survey proposed one trusted by a single process the person started, and decision 0276 neither allows nor excludes that. It is neither allowed nor excluded yet. The design of point 11, when it is written, decides it, with rule 5 and decision 0277 as its measure, and says which kind of line it is.

## Consequences

- The roadmap's row X3 names a reader of the file's bytes; row X4 names fish completion lines only; row E6 is marked withdrawn.
- SCOPE.md names fish's completion files where it named shell completion files, and says that bash and zsh completion files are not read.
- [Section 704](../spec/7-project/704-readiness.md), [section 304](../spec/3-packs/304-the-mcp-pack.md), the [design of `a2m-pack-mcp`](../architecture/crates/a2m-pack-mcp.md), and the hypotheses register stop naming package E6 as the test of H-27.
- Every library named here is still checked against `deny.toml` when it is added.

## Alternatives weighed

- **Ask the system to load a type library (`LoadTypeLibEx` through the `windows` crate),** as the first survey built on. It is the system's own reader, and it runs code of the system on a file that may be hostile, on Windows only. Not taken.
- **Write the type-library format from the documentation alone.** The only references to the format the surveys found are under the LGPL, which this project does not copy from, the estimate is 4 to 8 weeks, and the third survey judged it a mistake while a permissive reader exists. Taken only if `msft-typelib` fails its fuzzing.
- **Use `zbus_xml` for the introspection data.** It is maintained and MIT, and it adds a package for a small, stable format. Not taken.
- **Read zsh `_arguments` specs as text,** as the first survey proposed. Many zsh completions compute their specs when they run; a reader of the literal text would report some flags and miss others without saying which. Not taken.
- **Keep E6 as a placeholder.** A package the design forbids would read as work that is ready. Not taken.

## Revisit if

`msft-typelib` fails its fuzzing, or a type library worth reading turns out to need the system's loader; a zsh completion form is found that states its flags in data; or point 7 of decision 0276 is superseded.
