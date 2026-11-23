# 0333. The desktop pack reads a management class's definition and a COM type library, and drafts reads that it does not yet run

**Status:** Accepted. A design decision by an agent, made on 2026-11-23; any agent may supersede it with evidence. The owner has not read this record.

**Date:** 2026-11-23

**Refines** [decision 0276](0276-what-the-scout-may-learn-from-a-program-itself-and-by-which-means.md), point 1 (the Windows readers), and [decision 0279](0279-what-the-three-surveys-left-open-against-decision-0276-is-settled.md), point 2 (a type library read from its file's bytes, never loaded by the system, by a reader taken whole and fuzzed here). **Changes** [spec section 305](../spec/3-packs/305-the-desktop-pack.md), the [design of `a2m-pack-desktop`](../architecture/crates/a2m-pack-desktop.md), and the roadmap (package X3 done).

## Context

Windows programs publish two things about themselves that say which members only read. The system's management service gives the definition of a class (its properties, each marked writable or not, and its methods). A COM component's type library lists the members of its interfaces, each declared as a function, a property get, a property put, or a property put by reference. Decision 0276 chose to read both; decision 0279 chose how to read the second: from the bytes of its file, never through the system's loader, by a library taken whole and fuzzed here (`msft-typelib`, Apache-2.0, no dependency) or by a reader of our own.

The package, X3, left open how a type library reaches the scout, what a read of such a member is when this version of the pack has no driver that calls a COM component or reads a management instance, and what can be proven without a Windows machine.

## Decision

1. **Two discoverers in the desktop pack.** `desktop_com` reads a COM type library a person hands the scout, under the name `type_library` (decision 0263), as bytes, on every system. `desktop_management` asks the driver for the definition of the management class the surface names, with the optional surface fields `management_class` and `management_namespace` (default `ROOT\CIMV2`). Their findings are `com_type_library` (at `inferred`, source `artifact`) and `management_class` (at `discovered`, source `live_system`).
2. **The type library is read by `msft-typelib` 0.2.0, taken whole, held to that version, and fuzzed here.** The library has no dependency, denies unwrap, indexing, and unchecked arithmetic in its own code, and is on the closed list of licences. It borrows from the bytes; nothing is loaded, registered, or run, on any system. A test of the pack's own changes a type library built byte by byte at every byte and four bytes at a time to the largest values a field holds, cuts it at every length, and holds that the reader answers each with a value or a name. The reader of the pack's own around it bounds interfaces, members, parameters, and words.
3. **Only the MSFT form is read. Any other is the named stop `type_library_format_unknown`:** the older `SLTG` form, for which no reader is known; a program file (`MZ`), which may hold a type library as a resource, whose reading is not built; and anything else. An MSFT file that does not hold together is `finding_invalid`. A stop before anything is stored.
4. **The fixture is built, byte by byte, by the test's own builder** (`tests/common/typelib.rs`) from the format's description: one library of an imaginary application with one dispatch interface, two property gets, one property put, and one function. No real library is committed.
5. **The finding carries, for each member, how the library declares it to be invoked** (`function`, `property_get`, `property_put`, `property_put_ref`), its memid, its return type, and each parameter's type and direction; for each class property, its CIM type and `access`: `readwrite` when the definition marks the property writable, and `read` when it does not. A definition that does not mark a property writable says it can only be read; that the class's own provider agrees is not known from the definition.
6. **Reads are drafted for a property the definition does not mark writable, of a singleton class, of a simple type, and for a property get of a dispatch interface that takes nothing and gives a simple value.** The surface must state the class (`management_class`) for a management draft; a COM draft binds the target's one `desktop` surface of the program. A class that is not a singleton is not drafted (a schema does not say which instance); an array, a method, a function, a property put, and a name the compiler takes for a secret's (decision 0302) are never drafted. A drafted read has the verb `read`, the effect `read`, and one output, `untrusted_text`, and binds a new key of the `desktop` binding: `management: {surface, class, name}` or `com: {surface, interface, name}`.
7. **A drafted read compiles, and this version carries out no step for it.** The lowering turns the binding into one step, `management_get` or `com_get`, with no commit; the driver stops it by the named code `step_not_built`, with nothing sent. A COM property get is a call into a component, which needs a running object found by a means that starts nothing and is not read from the file; a management instance read needs an instance key. Each is a driver of its own, and a decision of its own. The drafts say what is a read; they do not yet run.
8. **The Windows reader of a management class is written and type-checked here, and unproven until CI's Windows runner runs it.** It asks for the class definition (`GetObject` of the class, the enumeration of its properties and methods, the `write` and `Singleton` qualifiers), never an instance, a method, or a put, on a thread that owns its apartment for the length of the call, and it compiles on all three systems (type-checked for Windows and macOS from this machine). On any other system the step stops with `system_not_built` and nothing is sent. The test of the service itself is written for Windows only.

### Not built, and why

- **Running a drafted read.** See point 7. A choice.
- **A type library inside a program file.** It needs the program file's resource table, which package X5's reader of programs gives. A choice.
- **The older `SLTG` form.** No reader is known. Not built; a named stop.
- **A surface for a program that is not running.** As for decisions 0316 and 0317, the driver prepares a surface by finding the application first. A choice, the same open point.

## Consequences

- The desktop pack has two discoverers, two finding kinds, two drafting rules, two codes (`type_library_format_unknown`, `step_not_built`), three step kinds (`schema`, `management_get`, `com_get`), two binding keys, one dependency (`msft-typelib`), and one `windows` feature (`Win32_System_Wmi`) more.
- A drafted read that no driver runs is a tool that cannot yet be published: a trial of it stops by name. That is the intended state until the drivers of points 7 exist.

## Alternatives weighed

- **Load the type library through the system** (`LoadTypeLibEx`). Decision 0279 weighed it: it runs code of the system on a file that may be hostile, on Windows only. Not taken.
- **Write the MSFT reader here.** Four to eight weeks by the survey's estimate, and a permissive reader exists. Taken only if the library fails its fuzzing, which it did not.
- **Draft nothing until a driver runs a read.** It would leave the evidence in a finding and no draft to compile. The package's proof names the drafts. Taken as written, with the limit of point 7 said plainly.
- **A property put by reference or a put drafted as a write.** Never: no write is built for these (rule 3 would need a read-back the definition does not give).

## Revisit if

`msft-typelib` is found to misread a library that a real component ships (the fixture is built here, and no real library is held); a type library worth reading is a resource of a program file; a management class that matters is not a singleton; or a driver of COM and management reads is built, which needs its own decision.
