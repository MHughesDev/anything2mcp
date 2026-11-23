# 0317. The desktop pack reads an application bundle's property list and the scripting dictionary it names

**Status:** Accepted. A design decision by an agent, made on 2026-11-23; any agent may supersede it with evidence. The owner has not read this record.

**Date:** 2026-11-23

**Refines** [decision 0276](0276-what-the-scout-may-learn-from-a-program-itself-and-by-which-means.md), point 1 (the macOS reader). **Changes** [spec section 305](../spec/3-packs/305-the-desktop-pack.md), the [design of `a2m-pack-desktop`](../architecture/crates/a2m-pack-desktop.md), and the roadmap (package X2 done, X3 ready).

## Context

A macOS application says a good deal of itself in two files of its own bundle: `Contents/Info.plist`, which names the URL schemes it takes, the kinds of document it opens, the services it offers, and, under `OSAScriptingDefinition`, its scripting dictionary; and that dictionary, an `.sdef` file, which lists its commands, events, classes, and, for each class property, whether it can be read (`access="r"`), set (`w`), or both (`rw`). Both are files. Reading them needs no system call, no Apple event, and no running application. Decision 0276 chose to read them, as the means of the macOS half of "what a program already publishes".

The package, X2, left open how the files reach the scout, whether a dictionary's includes are followed, and what the reader does with the two forms of a property list.

## Decision

1. **A third discoverer in the desktop pack, `desktop_bundle`.** It applies to the surface kind `desktop`, makes findings of the kind `application_bundle` at the level `inferred` with the source `artifact`, and sends no step to the driver: it reads files only. It runs on every system.
2. **The bundle is handed to the scout as a folder,** under the name `bundle`, from the project's `sources` folder (decision 0263), as a person copies an application's bundle there. The discoverer reads exactly two paths below it: `Contents/Info.plist`, and the dictionary that property list names, `Contents/Resources/<name>`, where the name is a plain file name (letters, digits, space, `_`, `-`, `.`; no slash, no leading dot) and gets `.sdef` when it has no extension. A bundle with no property list, a property list of neither form, a dictionary it names that cannot be read, and a name that is not a plain file name are each a named stop before anything is stored.
3. **The property list is read in both forms by readers of the pack's own** (`plist.rs`, with `xml.rs` for the XML form, on `quick-xml`): the XML form, and the binary form (`bplist00`). Both give one value, which a test holds against one fixture written in both forms. They are bounded in size, depth, objects, and walking steps; a binary file whose objects lead back to themselves, one cut short, one that names an object beyond its table, and one of another version are named stops; a test changes the binary fixture at every byte and cuts it at every length and holds that the reader answers each. From it the finding carries the identity (`CFBundleIdentifier`, `CFBundleName`, `CFBundleExecutable`), whether it is scriptable, the URL schemes, the document types, and the services, as the application's own words, cut.
4. **The dictionary is read by a reader of the pack's own** (`sdef.rs`): suites; commands and events with their direct parameter, parameters, and result; classes with their properties and the classes of their elements; and enumerations. For each class property the finding carries the access the dictionary states and whether it states one: a property that states none is read-write by the format's own default, and the finding says it was not stated, so that it is not taken for evidence that it only reads.
5. **A dictionary that includes a file outside the bundle is refused whole, by name,** before any of it, or the property list, is stored: the failure `dictionary_include_refused`, class `policy`, says which include. An include is outside the bundle when it is another scheme, an absolute path, a path that climbs above the bundle, or a name that hides a path (`%`, `\`). An include that stays inside the bundle is listed in the finding and not followed, and the finding says so. The pack reads no file outside the bundle a person handed it.
6. **No reads are drafted from these findings.** A read of a property of a running application is an Apple event, which this version of the pack cannot send; so the finding carries the access, and nothing is drafted until a driver can ask. Package X3's windows half, and a later macOS driver of properties, use it.

### Not built, and why

- **Following an include.** A dictionary most often includes a part of the system's standard suite, a file of the system and not of the bundle; a part chosen by an `xpointer` needs a reader of that expression, and a wrong guess would give a suite it did not include. A choice, not a rule; see "Revisit if".
- **A surface that is not a running application.** As for the Linux reader (decision 0316), the `desktop` driver prepares a surface by finding the application; the discoverer asks it nothing, but a discovery is prepared first. A bundle on a machine where the application is not running cannot be read yet. A choice, noted as the same open point.

## Consequences

- The desktop pack has a discoverer, a finding kind, a failure code, and two readers more; its manifest and its section of the technical reference say so.
- The Windows readers of package X3 reuse the XML reader and the finding shape.
- Package X3 becomes ready.

## Alternatives weighed

- **Hand the two files singly** (`info_plist` and `scripting_dictionary`). It would not need a folder, but then an include could not be judged against the bundle, and the dictionary "named by the application's own property list" would be whichever file the person handed. Not taken.
- **Read the dictionary the property list names through the system** (`NSAppleScript`, or `sdef` the program). That runs the system's code on a file that may be hostile, on macOS only, and for the system's own includes would read outside the bundle. Not taken.
- **Refuse only the include, and store the rest.** It would give more, and it would also give a dictionary with a hole where the standard suite is. The package's proof says refused; a partial reading can be made when someone needs it, with a decision.
- **Read the property list with a crate.** The workspace has one reader of property lists, in the file pack, that a pack may not use; a crate of the system's would add a dependency for a format of a few hundred lines. Not taken.

## Revisit if

A dictionary worth reading is refused only for including the system's standard suite, which is common: then the system's standard-suite file may be named, by exact path, as the one include allowed; or a person needs the properties as reads, which needs a driver that sends Apple events.
