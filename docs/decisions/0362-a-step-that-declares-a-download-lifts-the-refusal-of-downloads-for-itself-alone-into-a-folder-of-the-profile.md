# 0362. A step that declares a download lifts the refusal of downloads for itself alone, into a folder of the profile

**Status:** Accepted. Design decision. The owner had not read this record when it was written. Carries out the declared download that [the record of cases 22 to 27](0355-the-web-pack-handles-the-edge-a-fixed-language-and-the-pages-of-cases-22-to-27.md) left with `a2m-pack-web-browser`, and the rule of [spec section 301](../spec/3-packs/301-the-web-pack.md) that a download is declared on the step.

**Date:** 2026-11-24

**Changes:** the browser base (`ActTab::allow_download`, `downloaded`, `forbid_downloads`, `Download`, `DownloadError`, `Browsers::downloads_folder`); an action of a screen (`download`); three failure codes; the edge-case row of spec 301; [file formats](../reference/file-formats.md); the designs of the two web crates; the test application's page `/cases/export`.

## Context

When a browser starts, the engine denies every download in every tab: the browser writes no file for one, so nothing lands outside the roots the engine was given, and an undeclared download is a stop. Case 24 of the test application is an export that finishes later and offers a file. Reading that file is work an operation may need, and the rule has always said the file passes through a scoped folder, declared on the step, never through an agent's context.

## Decision

1. **An action may declare one download.** `download: {most_bytes: N, into: name}` on an action of a screen says that pressing it starts one file of at most `N` bytes (`N` from 1 to 64 MiB, refused by the compiler otherwise), and, with `into`, which output takes the name the engine gave the file. An action that does not say so keeps the refusal: a download it starts stops the run, as before.
2. **The refusal is lifted for that step only, and put back whatever the step came to.** Before the click, the acting tab lifts the refusal for the browser's context of its profile and names the folder; after the file is whole or the step has failed, the refusal is put back before anything else is decided. A run that cannot put it back is stopped with the fate of the step unknown. While the refusal is lifted, a download that another tab of the same profile started would be written too: the observing tab cannot write and the acting tab is the one in the step, and this is the one place the lift is wider than the tab. The first download that begins is the one; a second is refused as an undeclared download is.
3. **The page decides nothing about where the file goes.** The browser is told to save under a name of its own making (the number it gives the download) into `downloads/`, a folder of the profile root, as a checked path made when it is not there. The name the page suggests is cleaned to a plain name and kept only to show, in the step's record. A suggestion that climbs out of a folder makes no file anywhere.
4. **The size is bounded while it is written.** A download whose announced size, or whose size so far, passes `most_bytes` is cancelled, and the file is deleted after the browser says the download ended, so that it is not written again after it is deleted. The step stops as `download_too_large`. A download that does not begin before the step's time ends is `download_not_offered`; one that the browser ends early, or that is not whole when the time ends, is `download_incomplete`, cancelled and deleted. All three are class `unknown_variant`, and the page's answer to the click is known.
5. **The file's name is the step's output.** The step gives the name of the file in the folder as the text of the output `into` names. What reads the file is a later step or a person: the web pack reads no file's content into a run (rule 4 and spec section 502: a file's content is the agent's untrusted input at best), and the folder is the engine's own.

## Not built

A declared download that is a page the browser shows instead of saving (a PDF in its viewer), a download that begins from a frame of another origin, several files from one action, a removal of old files from the folder, and the reading of a file's content by an operation (the file pack's, once it is a document there).

## Alternatives

| Alternative | Why not |
| --- | --- |
| Allow downloads for the whole browser, always | The refusal is a safe default that every pack built on the base relies on; a declared step is the exception |
| Let the browser name the file after the page's suggestion | The suggestion is the page's text and may hold a path |
| Bound only by the time of the step | A page can offer more than a disk should hold in a minute |
| Read the download through the page's own request instead | The page's request is a read from inside the page; a download the page starts by script, or one that needs the page's state, is not that |

## Proof

In `crates/a2m-pack-web-ui/tests/later.rs`, against the page `/cases/export`: the declared action's file is written into the profile's `downloads` folder under a name that holds neither a separator nor the page's name, with the content the application sent, and the application was asked for it once; the next visit's undeclared download is a stop that writes no file, so the refusal is back; a file of two mebibytes against an action that allows a thousand bytes is cancelled, stops as `download_too_large`, and leaves no file in the folder, and the name its page suggested (`../../large.csv`) made no file anywhere in the stand's folders. The base's own test that a download writes no file still passes.

## What would reopen it

A browser that reports a download's size only at its end, which would make the bound a check after the fact; or a person who needs the engine's files somewhere other than the profile's folder.
