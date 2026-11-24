# 0399. Platform profiles are data in `profiles/`, and the first part applied is the identifiers a platform makes

**Status:** Accepted. Design decision. The owner had not read this record when it was written. Carries out the first slice of package 8.4a, which [decision 0399 on the remaining widget drivers](0399-the-remaining-widget-drivers-built-from-what-a-page-says-of-its-controls-a-staged-file-a-grid-cell-by-its-rows-key.md) split from 8.4.

**Date:** 2026-11-24

**Changes:** the `profiles/` folder (four files); the `profiles` module of `a2m-pack-web-ui`; the surface field `profile` of a `web` surface; the lint rules `web.generated_id` and `web.profile_unknown`; the "Platform profiles" part of [spec section 301](../spec/3-packs/301-the-web-pack.md).

## Context

The design says a platform profile is one preset for each family of web interface, held as data, and that code never branches on a platform's name where a profile's data can say it. Nothing of it was built: a surface's `profile` was not a field of the surface kind, and the lint `web.generated_id` ("a locator matches a rejected identifier pattern of the surface's profile") waited on it. The first thing a profile can do with no page and no probe is to refuse a locator that could not last.

## Decision

1. **A profile is a file in `profiles/` at the root of the repository, in the form the spec shows.** Its keys are `profile` (the name, which is the file's name), `detect`, `ready`, `locators` (with a `priority` of kinds and `reject_id_patterns`), `input_commit`, `frames`, `drift_risk`, and `support_tier`. Four are shipped: `plain` (a server-drawn page, which rejects no identifier), `aspnet_webforms` and `controlled_inputs` (families whose identifiers are made, and whose inputs need a particular commit), and `virtualized_grid` (read only, so a write on it will not compile once that part is built). They name families of software, never an application.
2. **The files are read and held to a schema when the program is built and tested, and embedded in it.** the `profiles` module of `a2m-pack-web-ui` embeds the files and a test holds the list to the folder, so a file that is not embedded, or an embedded one that does not fit, fails the test. A file with an unknown key, a locator kind or a tier that is not one, a priority that repeats or is empty, a rule of `detect` that says two things, a name that differs from the file's, or an identifier pattern that the engine's regular expressions cannot run, is refused with a sentence that says what.
3. **The loader is in the web pack's crate, not in the browser base.** The spec says the data is compiled into the base because the probe needs the detection rules too. The base cannot read the YAML the files are written in without taking on the files crate, which is a layer it does not see, so the loader is here, in the crate that applies profiles. When the probe reads `detect`, it asks this crate (the two are one pack), or the loader moves down when the layers allow it.
4. **A surface of the kind `web` may name a `profile`.** It is one of the names of the files or `auto`, which the probe is to settle. Anything else is `web.profile_unknown`, an error that names the profiles there are.
5. **`web.generated_id` refuses a locator built on an identifier the profile says is made.** For a surface that names a profile, each `css` locator of a field or an action is searched for the identifiers it is built on (`#name` and `[id=name]`), and each `test_id` is taken as one; any that a `reject_id_patterns` entry matches is an error, with the file, the line, the profile's name, and a fix that says to locate by label, role and name, or text. A `within` is looked into. A surface with no profile, or `auto`, rejects nothing: no profile has said what its platform makes.

## Not built

- The probe's use of `detect` and `ready` to name a family and to know a page is ready (`unknown_platform`, `{profile_ready: true}`): these need a page, and are package E2's.
- The input commit (`blur`, `enter`, `native_set_then_blur`, `blur_then_wait_ready`) as an operation of the widget drivers: the files state it and the loader checks it, and the drivers still follow the one text rule of decision 0399 on the widget drivers.
- `web.profile_readonly`, the refusal of a write binding on a surface whose tier is `read_only` or `unsupported`; the frame mode `found`; the locator priority as the order the drivers try kinds of locator in; and `drift_risk` as how often drift is checked. Each reads the profile the same way these two lints do, and each is its own step.
- A profile written by a person outside the repository, and the settings that would name its folder.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Profiles as Rust tables in the pack | The design says adding a profile changes no Rust, and the probe and the lints would branch on names in code |
| Selectors on identifiers matched as plain prefixes | The spec's patterns are regular expressions, and an engine already in the workspace runs them |
| Reject identifiers whatever the profile | A site that chooses its identifiers well should be able to use them; only a platform that makes them is told it may not |

## Proof

In `crates/a2m-pack-web-ui/src/profiles.rs`: every file of `profiles/` is embedded and fits, a family rejects the identifiers it makes and only those, and each way a file may not fit is refused with its reason. In `crates/a2m-pack-web-ui/tests/profiles.rs`, compiling the `cases` fixture with the surface naming a profile: a locator on `#ctl00_Content_Reference` under `aspnet_webforms` does not compile and the message names the identifier and the profile; an identifier of that family that it does not make, the same one under `plain`, no profile, and `auto` all compile; a profile that is not one is refused with the names of those there are.

## What would reopen it

A family of interface whose made identifiers cannot be told by a pattern, or a platform whose profile needs to differ by a version the page says.
