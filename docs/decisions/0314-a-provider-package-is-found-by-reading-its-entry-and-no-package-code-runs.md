# 0314. A provider package is found by reading its entry, and no package code runs

**Status:** Accepted. Design decision, made under [decision 0294](0294-the-owner-hands-the-open-questions-to-the-agents.md). The owner had not read this record when it was merged. Settles the open point of [spec section 605](../spec/6-running-it/605-packaging-and-release.md): the exact entry-point group.

**Date:** 2026-11-23

**Changes:** a new function in `a2m-provider`, `find_python_providers`, and a small module in the Python package; the wording of sections 203 and 605 and of the designs of `a2m-provider` and `anything2mcp`. No change to the provider manifest or the provider contract.

## Context

Package F3: a provider package installed with pip is to be found through a Python entry point and listed as present and not enabled. The usual way to use an entry point is to load it, which imports the package and runs its code. Rule 5 says to operate openly, and the product's promise is that nothing runs that the person did not install and enable. A search for provider packages that ran their code would run it before anyone enabled anything.

## Decision

1. **The group is `anything2mcp.providers`.** An entry in it has the provider's name as its name and the dotted name of a package of its own as its value, such as `example_sheet = example_sheet_provider`. The package holds the file `anything2mcp-provider.toml` and, for a plug-in that starts a program, the program the manifest's `run` section names.
2. **An entry is read, never loaded.** The search reads the `entry_points.txt` of each installed distribution in the folders it is given, takes the group's entries as text, reads the value as a folder name inside the site folder, and reads the manifest there. A value that is not a dotted name of identifiers, such as `module:function`, is refused as a manifest problem and is never imported. No Python starts, in the search or at any later step before a person enables the provider.
3. **The folders are stated.** `find_python_providers` takes the site folders from its caller and returns them with what it found, and `EntryPointSearch::statement` says in words where it looked, that it ran no package code, and how many it found. The Python package gives the interpreter's own `purelib` and `platlib` folders as `anything2mcp.provider_site_folders()`: not the working folder, not `PYTHONPATH`, and not another installation.
4. **A found package is listed as present and not enabled,** by the existing `RegistryBuilder::add_plugin`. It registers nothing. The entry's name must be the provider's name in its manifest, or the package is a problem and is not listed.
5. **It starts only under the supervisor.** `PythonProvider::launch_spec` resolves the manifest's `run.program` inside the package folder with the checks every path passes, and gives the supervisor the manifest's digest, its fixed arguments, and an environment of only the names it lists. The supervisor's own checks of a program apply when it starts.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Python's `importlib.metadata` run by the engine, or the entry loaded | It starts Python and can run the package's code before it is enabled; rule 6 keeps Python off the path of a run |
| The entry's value as `module:attr`, as Python tools read it | It invites loading; a value that names code is refused so that no one writes one |
| A search of every `PATH` or every Python on the machine | The person did not choose those; the search covers one interpreter's folders and says which |
| A manifest path written in the entry | One more path from outside the package to check; the package name gives a folder inside the site folder |

## Not built

The product crate does not call the search yet, so `provider.list` on a running engine does not show a package installed with pip; the call, with `provider_site_folders()` of the wheel's own interpreter or a setting that names the folders, is a later slice of the product crate. The plug-in host and enabling a plug-in are not built ([section 203](../spec/2-kernel/203-the-provider-contract.md)). The signature check is not built. An extra such as `anything2mcp[name]` is not built.

## Proof

`a_provider_package_installed_with_pip_is_found_listed_not_enabled_and_started_under_the_supervisor` in `crates/a2m-provider/tests/entry_point.rs` builds three wheels by hand, installs them into a new virtual environment with pip and no network, and shows that the search finds the one good package, refuses the entry that names code and the entry whose name differs from its manifest, lists the package as not enabled, writes no marker that an import would have written, and starts its program under the supervisor with the declared digest verified. `bindings/python/tests/test_providers.py` shows the interpreter's folders hold the installed package and that its entry is visible without being imported.
