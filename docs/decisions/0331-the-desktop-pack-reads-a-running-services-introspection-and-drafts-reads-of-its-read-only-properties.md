# 0331. The desktop pack reads a running service's introspection, and drafts reads of the properties it states can only be read

**Status:** Accepted. A design decision by an agent, made on 2026-11-23; any agent may supersede it with evidence. The owner has not read this record.

**Date:** 2026-11-23

**Refines** [decision 0276](0276-what-the-scout-may-learn-from-a-program-itself-and-by-which-means.md), point 1 (the Linux reader), and [decision 0279](0279-what-the-three-surveys-left-open-against-decision-0276-is-settled.md), point 3 (a reader of our own on `quick-xml`, the call made with `zbus`). **Changes** [spec section 305](../spec/3-packs/305-the-desktop-pack.md), the [design of `a2m-pack-desktop`](../architecture/crates/a2m-pack-desktop.md), and the roadmap (package X1 done).

## Context

A desktop application on Linux often keeps a service on the session bus beside its window: a name on the bus, objects, interfaces, methods, signals, and properties. Its introspection is the one published interface of its kind that says, for each property, whether a caller may read it, set it, or both (`access`), which no help text does. Decision 0276 chose to read it, and decision 0279 chose how: a small reader of our own, the call made with the bus library the workspace already holds, a service that is not running never started.

The package, X1, left open where in the pack this lives, how a surface names the service, how a handed file reaches the reader, and what "drafted as a read" means for a property when the pack's one driver reads windows.

## Decision

1. **A second discoverer in the desktop pack, `desktop_service`.** It applies to the surface kind `desktop`. It makes findings of the kind `dbus_service`. It asks the running service through the driver, as the window's discoverer does, so that its only way to the bus is the driver's read-only back end.
2. **The surface names the service.** A `desktop` surface has an optional field `service`: the bus name the application's service takes on the session bus. Without it, the discoverer asks no service and reads only the files it is handed.
3. **The question is the driver's `introspect` step, sent by the discoverer only.** The back end walks the service's objects from `/`, asking each for `org.freedesktop.DBus.Introspectable.Introspect` and nothing else, within bounds (200 objects, 1 MiB, 12 levels, a bound named when hit). Every message carries the bus's flag that forbids it to start the service (`NO_AUTO_START`); the back end's one function that sends to a service is the only place that sends, and it sends no other member than the two of its list (`Introspect` and `Properties.Get`), which a test holds. A name no connection owns is not a failure of the run: the step answers `running: false`, and the finding says the service was not running and that nothing was started. A test proves it against a `.service` file of the stand's whose program leaves a marker: asked with the flag, no marker; asked without it, the marker, so that the check can fail.
4. **Two files a person hands the scout are read as text,** under the names `service_file` and `interface_file` (decision 0263): a `.service` file gives the bus name and the program it starts, with no argument kept; an interface description gives interfaces as a service would answer. Neither is run, nothing in it is fetched, and a document type with declarations of its own is refused so that no entity is expanded. Their findings are at the level `inferred`, with the source `artifact`; the live service's is `discovered`, with the source `live_system`. A file the reader does not know is a named stop before anything is asked of the bus.
5. **The reader is the pack's own, in `dbus.rs`, on `quick-xml`,** on every system, with no system call, bounded in size, depth, count, and name length. A property whose `access` is none of `read`, `write`, `readwrite` is a refusal of the document (rule 7), never a read. The bus's own interfaces (`org.freedesktop.DBus.*`) are counted in each object and not listed.
6. **Access is carried into the finding,** for every property, as the document states it.
7. **A drafting rule drafts a read for each property that states `access="read"`,** of a type the `property` step gives as text (a string, an object path, a flag, a whole number), on an interface that one object of the service answers, and whose name is not a secret's (decision 0302). It drafts nothing from a file's finding, from a service that was not running, or from a walk that was cut. A property that states `readwrite` or `write` is not drafted at all; no method, no signal, and no write is ever drafted. The draft binds a new key of the `desktop` binding, `property: {surface, path, interface, name}`, with the verb `read`, and its output is one `untrusted_text`.
8. **The `property` step reads one property.** It sends `Properties.Get` with the same flag and returns the value as text; any other form (a list, a map, a variant) is the named stop `property_not_text`; a service that is not running is `service_not_running`. The lowering refuses any other verb (`desktop.property_reads_only`), an operation that is not a read (`desktop.read_acts`), a stray window key on the binding, a path, interface, or name a bus does not accept (`desktop.property_invalid`), and more than one output (`desktop.property_output`).
9. **The stand.** `targets/example-desk` gains a service of its own on the session bus (`org.example.Desk`, two objects, one interface with a method that is counted if it is called, a read-only and a read-write property, a property named as a secret, and a property that holds a list), a second bus name that has a `.service` file and is not running, and the record of every member it is asked and of every program the bus starts.

### Not built, and why

- **A service of an application that publishes no accessibility tree.** The `desktop` driver finds the application through the accessibility bus first, and a surface that names a service is prepared like any other. A background service with no window needs a surface that does not ask for a window; that is a design change (a surface kind of its own, or a prepare that does not need an application), and it waits for a service that needs it. A choice, not a rule.
- **The system bus, a method call, a property set, signals as triggers.** Not built. Calling a method of a service cannot be known to read; setting a property is a write, which needs a read-back the interface does not give in general.
- **Windows and macOS.** The steps stop with `system_not_built`; the `.service` and interface readers compile and run on every system.

## Consequences

- The desktop pack has a discoverer, a finding kind, a drafting rule, two failure codes, two step kinds, and a binding key more; its manifest and its section of the technical reference say so.
- The stand's session bus has a service directory of its own, so that an idle service can be named in a file and be seen not to start.
- Package X2 becomes ready.

## Alternatives weighed

- **A surface kind of its own for a service.** It would reach a service that has no window and would not need the accessibility bus. It would also be a second mode, driver, and lowering for what is one more way of reading an application, and no service needs it yet. Not taken, and written down above as what is not built.
- **Ask `NameHasOwner` first, then introspect.** Two calls and a race between them; the flag on the one call is the whole guarantee and is what the bus defines. Not taken.
- **Use `zbus_xml`.** Decision 0279 weighed it. Not taken.
- **Draft a read of a `readwrite` property too.** Reading it is safe, but the document says a caller may set it, and a person decides which of those a tool is for. Left to a person.

## Revisit if

A service worth reading turns out to be one with no window; a service states a property `read` that a call then sets (the document would be wrong, and the drafted read would still be a read); or a form of `access` other than the three is found in use.
