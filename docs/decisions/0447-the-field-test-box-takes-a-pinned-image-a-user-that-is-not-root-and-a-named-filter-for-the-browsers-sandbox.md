# 0447. The field test's box takes a pinned image, a user that is not root, and a named filter for the browser's sandbox

**Status:** Accepted. Design decision, made by the worker on FT5's page in a browser under the lead's brief. The owner had not read this record when it was written. No public interface of the product changes: the change is to the field-test runner, which is test equipment.

**Date:** 2026-11-25

**Changes:** `tools/field_test.py` and its tests; a new folder `tools/field-box/`, holding the container engine's default system-call filter unchanged and a notice of where it came from; the [field harness's design](../architecture/crates/a2m-copy-test-harness.md); roadmap FT5 and its row in the [Build Order's line](../plan/build-order.md#the-line-every-package-left-and-whether-a-worker-can-take-it); the [field test results](../reviews/field-tests.md).

## Context

The field harness's box, the container in which the runner runs the product's commands and the field harness ([decision 0310](0310-the-field-harness-runs-in-a-closed-network-and-says-in-the-ledger-what-it-is.md)), could not start a browser, so no field test of a page in a browser had been run (FT5's entry "The kinds not run in FT5" in the field test results). Three things stood in the way, each measured on this machine:

1. The box ran the product as the runner's user, here root, and the browser refuses to start as root unless its sandbox is switched off with `--no-sandbox`. The web pack never passes that flag, and a test holds it (`the_launch_arguments_never_switch_the_sandbox_off_or_hide_automation`). Rule 5 keeps it so.
2. The box's image held no browser, and the closed network gives it no package source.
3. In a container of a published image that holds a browser, run as a user that is not root, the browser started with its sandbox only when the container's system-call filter was off.

Measured again for this record, on 2026-11-25, with the container engine's default filter: the browser's zygote stops with "Failed to move to new namespace ... Operation not permitted" (exit 82). The browser's sandbox on Linux makes a user namespace, and in it a process and a network namespace, for each renderer. The default filter lets `clone` make no namespace and `unshare` none, unless the container holds `CAP_SYS_ADMIN`, which it does not and must not.

## Decision

1. **A recipe may name the box's image: `"box_image": "name@sha256:<64 hex>"`.** The image is named by its digest and in no other way: a tag, a name without a digest, or anything else is refused before anything is started. It takes the place of the runner's own Debian image, so it must hold what the box needs: a glibc at least as new as the product's binary was built against, a browser at one of the web pack's known places (the pack searches nowhere else, and a recipe does not name another program), and `sh`, `bash`, `timeout`, and `getent`, without which the closed-network check gives no verdict and stops the run, as it already does. The image's own entry point is never run (`--entrypoint ""`), whatever the image: an image with a browser may start programs of its own from one, and the box runs only what the runner starts in it. The report states the image.
2. **A recipe may name the box's user: `"box_user": "uid:gid"`, in numbers, never root.** A name is refused, so the runner need not ask the image who it is. A uid of `0` is refused. The runner gives the engine's home and the project to that user, before the first step and again after each step that writes in them from this machine (`copy`, `replace`, `python`), as a person owns their own; only a runner that is root can do that, so a runner that is not root may name only its own user. Without the option the box runs as the runner's user, as before. The report states the user.
3. **A recipe may name the box's system-call filter, by a name from the runner's own list: `"box_seccomp": "browser_sandbox"`.** The list has one name. `browser_sandbox` is the container engine's default filter, kept unchanged in `tools/field-box/seccomp-default.json` (copied from the engine's own repository at a named commit; `tools/field-box/NOTICE` says which, and its licence, Apache-2.0), plus exactly two rules: `clone` may make a user, a process, and a network namespace and no other kind, and `unshare` may make a user namespace and no other kind. Each rule masks the call's flags by every other namespace flag and lets it through only when the result is zero. The runner checks the default file against its hash before it uses it, and refuses a file that has changed, so that the base cannot be widened by an edit nobody reviews. It writes the composed filter into the run folder and states its hash in the report.
4. **What the runner refuses.**
   - A filter given as anything but a name of its list: `unconfined`, a path, a filter written in the recipe, or a name it does not know. A recipe cannot give a filter of its own, and no name on the list turns the filter off.
   - `browser_sandbox` for a box whose user is root: the filter is for a sandbox that only a browser started as another user makes.
   - Any recipe that holds a flag that switches a browser's sandbox off, in whole or in part (`--no-sandbox`, `--disable-setuid-sandbox`, `--disable-namespace-sandbox`, `--disable-seccomp-filter-sandbox`, `--disable-gpu-sandbox`, `--single-process`, `--no-zygote`), in its own text or in any file it gives the box. A recipe cannot name a wrapper program that passes one, or put one in a step.
   - The runner itself passes no option that widens the box: no `--privileged`, no added capability, no other security option, no `--userns`. A test holds the box's argument list to that.
5. **A step proves the sandbox: `"sandboxed": {}`.** Run in the box while a browser runs there, it passes when at least one renderer runs, every renderer is in a user namespace that is not the box's own, and no process of the box carries a flag of point 4. A browser that started is not by itself the proof, because the proof must not rest on the pack's own word.
6. **Unconfined was not needed, and is not offered.** On this machine the browser's sandbox started under the default filter plus the two rules of point 3, and its renderers ran in user and network namespaces of their own. Under the default plus `unshare` alone, `clone` alone, or `clone` kept from making a process or a network namespace, it did not start. Should an engine, a kernel, or a browser version be found where these two rules are not enough, the change is a new name on the runner's list with the call it adds, measured and recorded, never `unconfined`.

Rule 5 holds throughout: the browser runs with a data folder the engine owns and with its sandbox on, and the runner adds nothing to how the pack starts it.

## Alternatives

| Alternative | Why not |
| --- | --- |
| `seccomp=unconfined` for the box (what made the browser start in FT5's trial) | It lifts every limit of the filter for every program in the box, when two calls with three kinds of namespace are what the sandbox needs |
| A filter file written in the recipe | The recipe lives outside the repository and nobody reviews it; a filter is what decides how far the box reaches into the machine, so it is named from a list the repository holds |
| Give the box `CAP_SYS_ADMIN`, which the default filter takes as leave to make any namespace | It lets the box mount, change its name, and much else besides, far wider than the two calls |
| Install a browser into the runner's Debian image at run time | The closed network has no package source, and an image built at run time is not pinned |
| A box user by name, such as the image's own browser user | The runner would have to ask the image for its numbers before it could give it the home and the project; numbers are what the container engine and the files take |

## Proof

`tools/test_field_test.py`, the class `BoxOptions`: without the options the box is as it was; an image without a digest, a root or named user, a user handed over by a runner that is not root, a filter that is not a name of the list, and the browser filter for a root box are refused; the default file has the hash the runner was written for, and a changed one is refused; the composed filter is the default and the two rules and nothing else; `clone` passes the sandbox's three namespaces and no other, and `unshare` a user namespace only; the box's argument list carries the image, the filter, and the user, never the image's entry point, and no wider option; a sandbox-off flag is found in a recipe's text and in a file it gives the box; the sandbox check passes only on renderers each in their own namespace.

The field test of a page in a browser, recorded in the [field test results](../reviews/field-tests.md), runs the box with all three options against the test application.
