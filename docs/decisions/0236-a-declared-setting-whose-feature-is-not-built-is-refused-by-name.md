# 0236. A declared setting whose feature is not built is refused by name

**Status:** Accepted. Design decision. Written by the configurability review of 2026-11-18; the owner has not read it. Ready to build.

**Date:** 2026-11-18

**Changes:** the settings schema and loader in `a2m-actions` ([crate design](../architecture/crates/a2m-actions.md)); `config.show`; the built-in `ci` preset; [section 226](../spec/2-kernel/226-configuration.md), principles 2 and 3 and the protected-settings table; the test of [section 107](../spec/1-product/107-the-bar.md), configurability. Takes the rule [decision 0196](0196-the-engine-and-the-registry-after-the-acknowledgement-and-the-first-hour.md) made for `network.proxy` and makes it the rule for every key.

## Context

Section 226 promises that no setting is ever silently without effect: an unknown key is refused, with the nearest known keys. The [configurability review](../reviews/configurability-review-2026-11-18.md) read every key of the schema and every reader of one. Of the 95 kernel keys, 55 are read by nothing. Of those, 35 govern a part that is not built (a server's front doors, telemetry export, capture recording, updates) and 20 govern a part that runs today (retention of traces, the form and place of logs, disabled modes, promoted writes, the store's place, concurrency ceilings, the scout's budget). A value written for any of them is accepted, shown by `config.show` as in force, and does nothing. Three of the eight values the built-in `ci` preset sets are among them. One key, `network.proxy`, is already refused by name when set, because a person who set a proxy would otherwise believe their traffic went through it.

The catalog of actions already has the rule this needs: an action that is in the catalog and not built answers `action_not_built`, by name, saying what it arrives with.

## Decision

1. **Every key in the schema says whether it is built.** `SettingDef` gains a field: built, or not built with the words of what happens instead (such as "traces and captures are kept; nothing is deleted yet") and what it arrives with (a roadmap package or a decision). A key is built when code reads it and honours every value its type takes.
2. **A value written for a key that is not built is refused by name, from every layer.** The load stops, as for an unknown key, with the code `settings_invalid`, the file and line, the key, what happens instead, and the fix: remove the key. This holds for a value equal to the default too: a written value states an intent, and no intent about a part that is not built can be honoured. The installation file, a site's preset, a variable, and a flag are refused alike.
3. **The key stays in the schema.** So a person who writes it is told the truth ("not built yet, and this happens instead") rather than "not a setting" with the nearest keys, and so the schema, `config.show`, and the generated reference keep one list of what is designed.
4. **`config.show` shows it.** Each entry gains `built`, and for a key that is not built, `instead` and `arrives_with`. A key read only in part (such as `engine.person`, honoured for the wait for a sign-in alone, or `runners.lease_seconds`, at least 600 in one process) is built, and its documentation in the schema says exactly which part is in force.
5. **A built-in preset never sets a key that is not built.** A test holds every built-in preset to it. The `ci` preset loses `logging.format`, `logging.to`, and `authoring.capture.enabled` until they are built; section 226's row for it says so.
6. **Two tests hold the schema to the code.** The present test, that every key read is declared, scans every crate that reads settings, not `a2m-actions` alone. A new test holds the converse: every key marked built has a reader. A reader is found by the accessor names (`int`, `text`, `flag`, `list`, `get`, `entry`) and by the declared prefixes read through `strip_prefix` (`providers.mode.`, `packs.`, `scheduler.`, `ledger.intent_window.`). A key marked built that no code reads fails the build.
7. **The 20 inert keys are marked not built in the same change.** Each is then built in its own slice, and loses the mark when its reader and a test that changes it exist. The review ranks which first: `evidence.trace_retention_days`, `modes.disabled`, `engine.promoted_writes`, `evidence.redaction.extra`, `logging.format`, `logging.to`.

## Also decided, from the same reading

**`policy.two_person_irreversible` is protected,** with true the safer value. Section 226 says layers 5 to 8 may only tighten; the key was unprotected on the thought that raising it tightens, and so lowering it from a user's file, a variable, or a flag went unchecked after an installation had turned it on.

## Consequences

- `network.proxy` loses its special case in `Loader::finish`: it is a key not built, and the general rule refuses it, as today, with the same message.
- A person whose file sets one of the 55 keys finds the engine does not start until the key is removed. The product has no release yet, so no installed user meets this.
- The loader's fix for a refused loosening names `anything2mcp config set`, which is not built. Until it is, the fix names the installation file, which is the one channel there is.
- Section 226's principle 2 gains one sentence, and its protected table gains `policy.two_person_irreversible` and `packs.command.admin_groups`. Section 107's test gains "every declared key is read, or marked not built and refused by name".
- A pack's manifest can mark a key not built the same way, with the same effect, once a pack declares one ahead of its reader.

## Alternatives weighed

- **Take each unread key out of the schema until its reader exists.** Simple, and the unknown-key rule then refuses it. But the refusal says "not a setting" and offers the nearest keys, which misleads a person who read the key in section 226; the design's list of what is coming is lost from `config.show` and the reference; and each key comes back later as a change to the schema. Turned down.
- **Keep the key, accept its value, and show it as "not built" in `config.show`.** Nothing stops at load. But a person who never runs `config.show` believes their retention, their disabled mode, or their logging form is in force. That is the silent lack of effect the promise forbids. Turned down.
- **Refuse only a value that differs from the default.** This is what `network.proxy` does. For most keys it is enough, but where the default itself is not what happens (a 30-day retention that keeps everything for ever), a written default is the same false belief. Turned down for the general rule.
- **Build every inert key now.** Most are a slice each; together they are a phase. Marking them first makes the product honest in one change, and the building follows in order.
