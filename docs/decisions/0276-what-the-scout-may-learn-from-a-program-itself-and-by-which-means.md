# 0276. What the scout may learn from a program itself, and by which means

**Status:** Accepted. A design decision by the lead agent, made on 2026-11-18 because the owner handed this choice to the agents; any agent may supersede it with evidence. The owner has not read this record.

**Date:** 2026-11-18

## Context

The scout learns what a system can do from what the system says about itself: a program's help, a page, a served description. For a compiled program that is often little. The owner asked what existing reverse-engineering software could be used, what could be taken in part and adapted, and what should be built here, and then left the choice to the agents.

Three surveys were made on 2026-11-18 from each project's own pages: the interfaces a program and its operating system already publish about it, and the tools that watch a program run; the analysis engines and decompilers, with the project studied earlier ([research note 16](../research/16-rea-methods.md)) compared as a dependency and as a source of code; and the Rust libraries that read compiled programs. Their findings are gathered in research note 30 when the third is complete. This record states the choices; the note holds the evidence.

What the surveys showed, in short. Five published interfaces say which of a program's calls only read, which a help text never does. The code that already parses such interfaces elsewhere is under licences that forbid copying it into this product, and the formats are documented, so a reader of our own costs weeks. The engines that decompile are large programs a person installs; the one studied project that wraps them changes its tools in a third of its releases, marks 9 of 255 tools as read-only, bundles two packages under licences this project does not take, and offers no interface a program could link.

## Decision

Everything below serves the scout at build time. Nothing here runs during a tool call (rule 1), and nothing here gets past a sign-in, a licence check, or a protection (rule 5).

### Built here, in Rust, first

1. **Readers of what a program already publishes, where the publication says which calls only read.** On Windows: the schemas of the system's management classes, and COM type libraries read as files. On macOS: the scripting dictionary an application names in its own property list, and that property list's other declarations. On Linux: D-Bus introspection, asked so that a service that is not running is never started. Each is a discoverer in the pack whose kind of system it describes, and each finding says where the claim that a call only reads came from.
2. **Readers of command metadata as text**: manual pages, shell completion files, and PowerShell module manifests and help files. They are read as data. A completion script is never run, and a module is never imported to ask it what it holds.
3. **The plain facts of a binary**: what kind of file it is and for which system, what it imports and exports, its version resource and manifest, its entitlements, and who signed it. Linked from permissively licensed Rust libraries chosen from the third survey, or written here where the format is small and stable. No disassembly and no decompilation.

### Used as a program the person installs, off by default

4. **A decompiling engine, as an optional discovery provider.** The engine first supported is the one whose licence is permissive and which runs without a window. The person installs it; the product never fetches it, identifies it by digest, starts it under the supervisor's rules, hands it a file and never a running process, and takes back facts that are never above `discovered`. The same form serves a decompiler for .NET and one for Java and Android. This is the place for decompiled text: evidence an agent or a person reads while drafting, never an input to a run.

### Tried before any choice

5. **A Rust port of that engine's decompiler exists as a library** under a permissive licence. Its own description says an agent wrote it, and its history is one day long. A spike compares its output with the engine's on the project's own test programs. Nothing depends on it before that.
6. **The rules that turn recorded requests into an interface description** are small in every project that has them, and none marks a read beyond the request's method. The drafter here already infers from recordings; one outside project is read as a reference when that drafter is next worked on. No code is copied.

### Not included

7. **The studied project as a dependency.** The product does not start it, bundle it, or recommend it. This replaces the line of [decision 0053](0053-rea-is-studied-for-method-and-used-only-as-an-optional-provider.md) that kept it as an optional provider; the rest of that decision stands, and its methods remain in the design. A person may still map any MCP server through the MCP pack, as with any other.
8. **Copying code from a project under a licence that asks the copier to take that licence on.** That rules out the best-known parsers of manual pages, of type libraries, and of system calls as sources. It is the licence list's rule, restated here because the surveys met it often.
9. **Anything that injects code into a program, attaches a debugger to a program the developer did not build, relaxes a system's protection in order to trace, or installs a trust root for the whole system.** Each changes the program or the machine it observes.
10. **A decompiler of our own.**

### Left for later, allowed

11. **Tracing a program the person starts, by means that do not inject into it** (the system's own event tracing, or its tracing of system calls), and reading a saved capture of traffic. Build time only, asked for by the person, never by default. Not designed yet.

## Consequences

- The scout's reach into compiled programs grows first where the evidence is strongest and the cost lowest: published interfaces that state reads.
- New work packages: the readers of points 1 to 3, one per operating system and pack; the optional engine provider of point 4; the spike of point 5. They join the roadmap after the work in hand, and [SCOPE.md](../../SCOPE.md) shows each as not built until it is.
- Every library named in the third survey is still checked against `deny.toml` when it is added. Two candidates the surveys named carry licences that are not on the list as written.

## Alternatives weighed

- **Depend on the studied project for web and script analysis, behind an allow-list of its read-only tools.** It would give that analysis today. It would also put 190 megabytes, a second runtime, two licences this project does not take, and a tool surface that breaks every third release in front of every user who switched it on, and each release would need its list reviewed again. The web pack's own probe already covers the part the product needs.
- **Port the studied project's own logic.** Its evidence rules were already taken as method. Its capture templating and its small checks are the only other parts that are its own; they are small enough to write when needed.
- **Start with the decompiling engine.** It answers more questions, and it needs a person to install a gigabyte and a runtime before the scout says anything. The published interfaces need nothing installed and say which calls read.
- **Do nothing beyond help text.** Then a program whose help is thin stays a program the scout cannot describe.

## Revisit if

The spike of point 5 shows the Rust port equal to the engine on real programs; or a published interface turns out to state reads less reliably than its documentation says.
