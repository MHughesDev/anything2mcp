# 0419. A declared download is written beside the downloads folder, and moved in once whole

**Status:** Accepted. Design decision, made by the worker who found the fault, under the lead's brief. The owner had not read this record when it was written. It amends point 4 of [decision 0362](0362-a-step-that-declares-a-download-lifts-the-refusal-of-downloads-for-itself-alone-into-a-folder-of-the-profile.md) and changes no public interface.

**Date:** 2026-11-24

**Changes:** `ActTab::allow_download`, `ActTab::downloaded`, and `Page::forget_download` in `a2m-pack-web-browser` (`tab/downloads.rs`); the assertion of case 24 in `crates/a2m-pack-web-ui/tests/later.rs` on the run that hears an undeclared download.

## Context

Decision 0362 deleted the file of a download that was cancelled for its size once the browser said the download had ended (`Page.downloadProgress` with `state: canceled`), "so that it is not written again after it is deleted". The bet was that nothing is written after that event. It is wrong. Under load, the cancel can reach the browser while it is still choosing where the file goes: the browser reports `canceled` at once, then makes its half-written file (`{guid}.crdownload`) in the downloads folder, and clears it some 20 to 50 milliseconds later. The browser reports nothing in between. The engine found no half-written file, deleted nothing, and the step returned; a reader of the folder in that window found two files where one was kept. This happened in 2 of 50 loaded runs of the case-download test. In 5 of 25 loaded runs the cancel came too late, and the whole 2 MiB file was briefly kept in the folder under its final name before the engine deleted it.

Case 24 failed in 2 of 30 loaded runs for a different reason, and the fault was in the test. The undeclared download's report can arrive after the click's step has ended. The run then ends without a stop, and the next run hears the report at the start of its `wait` step, which sent nothing and says `not_sent`. The test held that every stop was `sent`, and that cannot be true for this branch.

## Decision

1. **The browser writes into a writing folder, never into the folder the step names.** `allow_download(folder, ...)` makes `{folder}-writing` beside `folder` (for the profile, `downloads-writing` beside `downloads`). It empties that folder of files an earlier step left, and tells the browser to save there.
2. **A file enters the folder only when it is whole.** When the browser says the download is `completed`, `downloaded` moves the file by a rename from the writing folder into `folder`, under the same name the browser made. A file that cannot be moved is deleted, and the step stops as the browser answering in a shape no rule covers.
3. **What a cancel leaves is never in the folder.** A cancelled, too-large, or late download is deleted from the writing folder once the browser says it ended. Any half-written file the browser makes after that is in the writing folder alone; the browser clears it, and the next lift clears what is left. The timed wait of up to two seconds for the browser to clear its half-written file is removed: nothing waits on it any more.
4. **A stop heard at the start of the next run tells that run's truth.** In case 24, a stop in the click's own run is `sent`, and a stop heard at the start of the next run's step is `not_sent`. The test asserts each of those exactly, in place of `sent` for both.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Wait for the half-written file to appear and then vanish | When the cancel is late, the file never appears, so the wait could end only on time. No event of the browser marks the end |
| Delete again when downloads are forbidden, or at the next lift | The file can still appear after either moment. Anyone who reads the folder in between sees it |
| A writing folder inside the downloads folder | The folder a step names would then hold something that is not a whole file |
| Make the click's step wait until the browser has answered for any download it might start | A click that starts nothing gives no condition to wait on. Waiting for that would mean waiting on time |

## Proof

The two tests in `crates/a2m-pack-web-ui/tests/later.rs` were looped alone. Load came from four busy loops on four cores and two loops of the file's other tests, each running its own browsers. Before: the case-download test failed 2 of 50, both times with `left.len() == 2` and a `{guid}.crdownload` beside the kept file; case 24 failed 2 of 30 at `assert_ne!(stopped.sent, Sent::NotSent)` with `unexpected_page ... at the start of a wait step`. After, under the same load: the case-download test passed 50 of 50, and case 24 passed 50 of 50.

## What would reopen it

A browser that reports the end of its own file handling for a cancelled download, which would let the engine wait on that instead. Another case is a person who reads the writing folder and expects it to be empty.
