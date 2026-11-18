# 0273. The settings keeper, and the registry's owed lines, after a second reader

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-18

**Changes:** the check and the read of the installation's settings file, the rule tests over product sources, `doctor.run`'s `home` check, the policy engine's question about test records, and `version.publish`, all in [`a2m-actions`](../architecture/crates/a2m-actions.md); two sentences of [section 226](../spec/2-kernel/226-configuration.md). **Builds on** [decision 0241](0241-protected-settings-loosen-only-from-an-administrators-file-and-every-key-says-whether-it-is-built.md), and supersedes its exception for a group the system gives its administrators. **Answers** findings B1 to B4 of the sixth safety audit, and the registry's half of its finding C1, whose HTTP half [decision 0270](0270-the-http-pack-sends-an-asserted-read-only-from-a-published-version-and-bounds-what-it-leaves-running.md) carried out. No action, schema, or failure code changes.

## What was verified

Each finding was checked against the code before it was changed, and each holds. Each test below, but the one that runs on macOS alone, failed with the line of its fix put back as it stood; then the fix was restored (the list is under "Tests").

## Decision

**B1. No group's write counts for loosening a protected setting.** Decision 0241 counted write by the macOS administrators' group (80) as closed, on the model of the provider's check of a program. On a Mac the person's own account is usually a member of that group, and a member writes what the group may write with no prompt. So, for this question, a file or a folder on the way that any group may write is `Open`, the administrators' group included. Loosening a protected setting is a stricter question than starting a program: a program's digest and its fixed arguments still bind what a started program does, while a loosened setting removes a guard outright, and the person who could change the file is the one the guard is against. The provider's check of a program keeps counting group 80 (decisions 0216 and 0271), and nothing there changes.

**B1, one object.** The file is no longer checked and read as two lookups by path. The walk ends at the file's own entry, with no link left in its path, and keeps that entry's metadata. The file is then opened there without following a link at its end, without waiting on a pipe, and the opened handle's own owner and mode are checked and its device and number held to the entry the walk found. The text is read from that handle. A file swapped in between the walk and the open, with the same owner and mode, is `NotChecked` ("the file was replaced while it was checked"); a link put there is not followed. A file that fails the walk is read by its path as before, held to the person's rule. With no group's write excepted, no folder on the way can be changed by anyone but the superuser, so the walk itself cannot be raced by another account.

**B2. Access lists, from the provider's check.** After the handle passes, the keeper calls the provider contract's `check_program` (in `a2m-provider`) on the file, with the default `ProgramTrust` and the role `Script`, and takes from it what it adds: on macOS the extended access list of the file, of each folder, and of each link on the way, where any entry that allows is a refusal (decision 0271). The keeper's own rules of owner and mode are stricter than that check's and have already passed, so whatever else it refuses still refuses. No second walk of access lists was written. On Linux a named entry of a POSIX list raises the group's bits, and group write is refused here, so the list adds nothing.

**B3. The rule tests read each file to its end but its final unit-test module.** A file is cut only where a `#[cfg(test)]` (or `#[cfg(all(test, ...))]`) and any further attributes stand directly above a `mod name {` whose closing brace is the file's last line. A `#[cfg(test)]` on an item inside product code cuts nothing; a file that ends otherwise is read whole, which reads more, never less. The check that no product source sets the keeper now covers the field by name: only the loader and the product's test switch may name `installation_keeper`. The field stays public: the product crate assigns it (`anything2mcp/src/engine.rs`), and every test of this crate builds `Sources` with the struct's update syntax, which a private field forbids outside the crate. `Sources::with_installation_keeper_for_tests`, hidden from the documentation, is the route the product crate is to take instead; then the field may close. The rule test holds it meanwhile.

**B4, this crate's half.** `doctor.run`'s `home` check reads each of the engine's own folders as an entry, without following a link, and names one that is not a folder of this user's (another account owns it, or a link or a file stands in its place) as `config_drift`: the engine closes only a folder of its own user's, so it could not close that one when it started. A folder of this user's that others may enter is named as before. `keep_folder_private` in `a2m-local`, and the engine's start, which drops its error, are not this package's; the change they need is owed.

**C1, the registry's half.** Where the policy engine is told whether every record of a run is a declared test record, the environment's kind counts only from a published version's plan: `!drafted && kind == Test`, with `drafted` worked out as the lease's entry already is (the run's target and plan hash among the drafts the kernel keeps). A drafts' environment marked `test` gives no test records. The lookup is one function, `test_record_keys`, so the rule is tested by itself.

**Publish never takes the draft version.** `version.publish`, and the review `publish.request` raises, refuse the version `0.0.0` as `input`/`schema_invalid`, naming it as the drafts' own. Every plan compiled from the drafts carries `0.0.0`, and the HTTP pack tells a draft's plan from a published one by it (decision 0270); a published `0.0.0` would read as a draft there, and a draft as published wherever the two were mixed.

## Tests

- B1, group: `keeper::tests::a_file_the_administrators_group_may_write_is_not_the_administrators`: a file of group 80, and of the test's own group, mode `0664`, is `Open`, and so is a folder on the way of mode `0775`. Failed with the earlier keeper restored, its exception standing for the test's group as it stood for group 80 on macOS: `group 80: Administrator`. On Linux the access-list step alone also refuses it, because the provider's check counts no group there; on macOS that check counts group 80, so only the keeper's own rule refuses it.
- B1, one object: `keeper::tests::a_file_replaced_between_the_check_and_the_read_is_not_the_administrators`: a file of the same owner and mode put in place between the walk and the open, and a link put there, are never read as the administrator's. Failed with the read by path restored.
- B2: `keeper::tests::an_access_list_entry_that_allows_is_not_the_administrators`, on macOS alone.
- B3: `tests/settings.rs` `the_scan_of_product_sources_cuts_only_a_files_final_test_module`. Failed with the earlier cut restored: this crate's own `lib.rs` was cut at line 84 of its product code.
- B4: `paths::tests::a_folder_of_another_account_or_a_link_in_its_place_is_named`. Failed with the earlier mode-only check restored.
- C1: `control::told_tests::a_drafts_test_environment_gives_no_test_records`. Failed with `!drafted` taken out.
- Publish: `tests/build.rs` `drafts_are_linted_a_person_publishes_them_and_the_tools_are_found` asks to publish `0.0.0` and is refused by name, and nothing is stored under it. Failed with the refusal taken out.

The keeper's tests stand the test's own account in for the superuser (`read_with` takes the accounts that count as the administrator), so that the walk's rules run whoever runs the suite. The doctor's test names another account id to stand for the test's own, so a folder of the test's is another account's whoever runs it.

## What only a run on macOS can prove

1. A file or folder given `everyone allow write` or `everyone allow add_file` with `chmod +a` is `Open` through the provider's check, and the test's own temporary folder, with the entries the system puts there, passes.
2. The installation's folder an administrator makes, `/Library/Application Support/anything2mcp`, passes the walk with no group's write on the way. Where a folder on that way is group-writable on a real system, no installation file there loosens: the safe side, and the refusal names the folder.

## Alternatives weighed

- **Count group 80 unless the engine's user is a member** (the audit's first fix): reads the group list of the account, which tells nothing about another account the person runs a program as, and keeps an exception for a question where none is needed: an administrator who installs a file can make it the superuser's and close it. Not chosen.
- **Walk with `openat` folder by folder and read from the last handle**: proves the same thing in more calls into the system; with no group's write excepted, no folder on the way can be changed but by the superuser, so holding the handle's device and number to the walked entry closes the race the audit found. Not chosen.
- **Close the `Sources` field now and change every test**: the product crate, which is not this package's, assigns it. Left until it takes the setter.
- **A new failure code for the draft version**: the version refusals already answer `schema_invalid`; one more code would add a row to the technical reference for a refusal with the same next step. Not chosen.

## Consequences

- An installation file that a group may write, on macOS the administrators' group among them, no longer loosens a protected setting or sets a lock. It is still read, and held to the person's rule.
- A run of drafts is never told its records are test records.
- Owed: the product crate takes the setter in place of the field; `a2m-local`'s `keep_folder_private` acts on one opened handle; the engine's start reports a folder it could not close.
