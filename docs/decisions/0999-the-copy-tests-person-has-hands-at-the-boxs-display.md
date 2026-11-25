# 0999. The copy test's person has hands at the box's display

**Status:** Accepted. Design decision, made by the worker on FT6 under the lead's brief. The owner had not read this record when it was written. No public interface of the product changes: the change is to the copy test runner, which is test equipment.

**Date:** 2026-11-25

**Changes:** `tools/copy_test.py` and its tests; the [copy test harness's design](../architecture/crates/a2m-copy-test-harness.md); the [copy test results](../reviews/field-tests.md), field test 11.

## Context

FT6 copies a person's use of a web application: the engine opens a watching window of its own browser and records what the person does there ([decision 0442](0442-a-persons-use-is-watched-by-web-use-and-what-it-saw-twice-is-drafted-held.md)), and a person signs in in a window of the same browser ([decision 0254](0254-a-person-signs-in-in-a-window-of-the-engines-own-browser-and-the-web-pack-stops-on-every-declared-sign-of-expiry.md)). The product's own tests stand for the person with a script the test application puts in its own pages; a real application's pages carry no such script, and nothing of the engine types, presses, or moves in those windows. The runner's `person` step presses buttons on the engine's own page only. The box had a display since [decision 0459](0459-the-field-test-box-takes-a-desktop-session-and-one-application-the-recipe-names.md), and no step that acts on it.

## Decision

1. **A step `"hands": [argv]` runs a program in the box as the box's user, with the desktop session's `DISPLAY` and nothing else of the runner's environment.** It needs `box_desktop`. The program is an argument list whose first part is an absolute path: one of the box's image, or a file of the recipe folder given by `box_mounts`, so the sandbox flag check of decision 0447 reads it. It gets no engine's home, no passphrase, and no session bus, so it reaches the product only through what the display shows and takes, as a person's hands do.
2. **It is waited for, within `seconds` (300 unless the step says), or started and not waited for with `"detach": true`.** A person who waits at the screen for a window a press opens, such as the sign-in window, is a detached step before the `person` step, because the press's page answers only once the sign-in has ended.
3. **What a recipe's hands do is the recipe's.** For FT6 it is a shell script of a program that types and presses keys on the display: it types each address into the address bar of the engine's window, waits until the window's title names the page, and types an invented account into the application's own sign-in form. It reads nothing of the page but the window's title.

## Alternatives

| Alternative | Why not |
| --- | --- |
| A proxy that puts the test application's walking script into the real application's pages | It changes what the application sends, which is what the capture records, and a page's own security policy can refuse it |
| Drive the engine's browser through its debugging protocol from the runner | The engine owns its browser's only pipe (rule 5); a second driver would be the engine acting, not the person |
| Run the hands as the desktop session's application | The session waits for its application on the accessibility bus, which a script of keystrokes never joins |
| A `python` step of the runner's machine that finds the box by name | It reaches into the run's containers past the runner, and its environment holds the runner's |

## Proof

`test_the_person_s_hands_need_the_session_and_reach_the_box_by_its_display_alone`, in `tools/test_copy_test.py`: a `hands` step without a desktop session is refused; its command runs in the box with `DISPLAY` alone; `detach` starts it detached; an argument list that is empty, relative, a string, or holds an empty part or a NUL is refused. Field test 11 ran with it, three times on each of three recipes.

## Revisit if

A copy test must act in a window of a program other than the engine's browser, or on a machine with no container, where the person's hands are a person's.
