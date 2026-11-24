# 0999. A declared download gives its name and size as outputs, which the read-back does not read

**Status:** Accepted. Design decision, made by the worker on package 9.3s under the lead's brief. The owner had not read this record when it was written. It amends point 5 of [decision 0362](0362-a-step-that-declares-a-download-lifts-the-refusal-of-downloads-for-itself-alone-into-a-folder-of-the-profile.md) and changes no public interface: the provider contract, the engine, and the compiled target are as they were.

**Date:** 2026-11-24

**Changes:** the `download` of a screen's action (`size_into`); the `ui` lowering's `read` step; the interface driver's `invoke`; [file formats](../reference/file-formats.md); the design of `a2m-pack-web-ui`; the exports project of `crates/anything2mcp/tests/`.

## Context

Decision 0362 gave a declared download's name to the output `into` names. Through the product, the call never returned it. A `ui` write needs a read-back, and the lowering's `read` step reads every output of the operation from the screen's field of the same name, refusing an output with no field. So the target's author gave the screen a field `file`, the page's status line, to satisfy the compiler. The engine then merged the read-back over what the act's steps observed, as it must for a record's values, and the call returned the status line ("Export ready.") where the file's name belonged. The size the step observed went only into the step's capture, which is in the trace and in no field the caller sees.

The name and size are values the engine made: the browser named the file, and the engine counted its bytes. No page holds them, so no read of the page can give them back, and a field that pretends to is a read of something else under their name.

## Decision

1. **A download may give its size too.** `download: {most_bytes: N, into: name, size_into: size}`: with `size_into`, the step gives the file's size in bytes, as a whole number, to that output. `into` and `size_into` naming the same output is refused as `download_invalid`.
2. **The outputs a download fills are not read back.** The `ui` lowering's `read` step of a write's read-back, and so the preview and the freshness check that run it, skips each output the binding's action's download names. The screen needs no field for it. Every other output is read as before, and the read-back still visits the page in an observing tab and waits for it, so it still shows that the page is in the state its screen declares.
3. **Each output a download names is one the operation declares.** One it does not is refused at lowering as `unknown_field`, since the value would reach no one.
4. **The bytes stay out.** What reaches the result is the file's name in the profile's `downloads` folder and its size: never its content, its path, or the name the page suggested. Spec section 502's resource link is still not built; the name is its fallback.

## Alternatives

| Alternative | Why not |
| --- | --- |
| The engine lets the act's values win over the read-back's | The engine knows nothing of downloads, and for a record's values the read-back is the truth: a system that normalized what was written must be believed over what was sent |
| The engine gathers a `downloads` list from the step's captures into every result | It puts knowledge of one kind of system into the kernel (rule 8), and a capture is evidence, not an output |
| One output holding a record of the name and the size | A new value shape for one case, and an author who wants only the name still has to unpack it; two plain outputs keep the result flat |
| Only a change to the target files: rename the screen's field | The `read` step refuses an output with no field of its name, so the author must still invent one; the fault is in the lowering |
| Drop the read-back from the binding | A `ui` write without a read-back stops as `no_read_back`; the pack's evidence is its read-back |

## Proof

`crates/anything2mcp/tests/export_download.rs`, `a_declared_download_comes_back_as_its_name_and_size`, no longer ignored: through the product, against the test application's export, `tool call` and `run resume` return `result.file` equal to the name of the one file in the profile's `downloads` folder and `result.size` equal to its size on disk. Its sibling still holds that the marker in the file's bytes is in no field of the result, the run's row, its trace, or any other file of the engine's home or the project. The project's offline test expects the name and size from the press step alone. In `crates/a2m-pack-web-ui/tests/compile.rs`, `what_a_screen_may_not_say_is_refused_with_a_file_a_line_and_a_fix` holds the two refusals: one output for both name and size (`web.download_invalid`), and a download output the operation does not declare (`web.unknown_field`).

## What would reopen it

A client that takes resource links, which would make the name and size parts of one link; or a page that does show the file it offered, which an author would then read back as a field under another name.
