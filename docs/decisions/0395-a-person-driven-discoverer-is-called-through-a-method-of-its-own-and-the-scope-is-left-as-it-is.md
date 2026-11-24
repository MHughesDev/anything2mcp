# 0395. A person-driven discoverer is called through a method of its own, and the scope is left as it is

**Status:** Accepted. Design decision, made by the design architect. The owner had not read this record when it was merged. Nothing in it is built. It changes a public contract, the `Discoverer` interface, by addition.

**Date:** 2026-11-24

**Changes:** how package E2a is built. Supersedes two clauses of point 2 of [decision 0392](0392-a-page-that-reads-by-a-method-that-may-write-is-learned-from-a-persons-watched-use-and-read-through-an-asserted-request.md): that `DiscoveryScope` gains the capture's end, and that a person-driven discoverer is handed a port that refuses every step. The rest of that point stands.

## Context

Decision 0392 put the signal that a person stopped a capture into `DiscoveryScope`. That struct is built field by field in thirty-three places across twelve crates, nearly all of them tests of packs. A new field breaks each, and a constructor moves each. Either way one package would hold twelve crates, and the rule that two workers never change one crate at a time would stop every pack's lane while it ran.

The signal also has no meaning for thirty-two of those places. A discoverer the engine drives never sees a person stop.

## Decision

1. **`Discoverer` gains `watch`.** It takes the scope, the capture's end, and the sink, and returns what `discover` returns. It takes no port: a discoverer a person drives reaches the system through nothing of the kernel's. Its default answer is a failure of the contract's own code for a step that is not usable, so no discoverer in the repository changes.
2. **`DiscovererHandle::watch` is the only caller,** and it checks each finding against the manifest as `discover` does. It refuses a discoverer whose manifest does not say `driven_by = "person"`, and `DiscovererHandle::discover` refuses one that does. So a discoverer is called by exactly one of the two, decided by its manifest.
3. **The capture's end is a type of the contract,** `CaptureEnd`: the kernel sets it once, and a discoverer waits on it or asks it. It is not constructible by a pack.
4. **`DiscoveryScope` is not changed.**
5. **A request's own body joins the capture's yield with the watching tab,** in package E2c, and not in E2a: the three fields of `CapturedRequest` are read first by the base, so the package that adds them holds the base too.

## Alternatives

| Alternative | Why not |
| --- | --- |
| The field, through a constructor, as decision 0392 said | Twelve crates in one package, for a value one discoverer reads |
| A second trait for a capture | Decision 0392 weighed it: the rules of a discoverer would be stated twice |
| A signal reached through the port | A port is how a discoverer acts on a system. A capture does not act, and a port that carries a signal and refuses every step is two things under one name |

## Proof

Package E2a's, in the roadmap.

## Revisit if

A second kind of person-driven discoverer needs something of the scope that `watch` does not hand it.
