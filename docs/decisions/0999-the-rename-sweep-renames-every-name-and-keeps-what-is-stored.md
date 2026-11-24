# 0999. The rename sweep renames every name, and keeps what is stored

**Status:** Accepted. Design decision, made under [decision 0294](0294-the-owner-hands-the-open-questions-to-the-agents.md), in package N0. The owner had not read this record when it was merged. The number 0999 is a placeholder the lead renumbers on merge.

**Date:** 2026-11-24

**Changes:** the name of the compiled target in the code and the documents in force ([decision 0329](0329-the-compiled-file-the-engine-runs-is-the-compiled-target.md)); the failure codes and the action whose names carried the old word; the platform profile `aspnet_webforms`, now `server_postback_forms`; nothing that is stored.

## Context

Decision 0329 renamed the plan, the compiled and hashed form of a target version the engine runs, to the compiled target, and gave package N0 the sweep. Some of the old word is not only a name in the code: it is in bytes the engine has already written, and those bytes must still open. Decision 0390 (point 15) put in the same sweep the one platform profile whose name was a real framework's, against rule 9.

## Decision

1. **Every name of the part is renamed.** Types (`Plan` is `CompiledTarget`, `EncodedPlan` is `EncodedCompiledTarget`, and the like), the module `plan` of `a2m-core` (now `compiled_target`), fields and functions, the format number (`COMPILED_TARGET_FORMAT`), spec section 206 (now "The compiler and the compiled target"), the file formats, the crate designs, the references, the generated clients, and the messages a person reads.
2. **The failure codes that carried the word are renamed:** `plan_format_unsupported`, `plan_hash_mismatch`, `plan_unreadable`, `plan_unverified`, and `plan_expression_unloadable` are `compiled_target_format_unsupported`, `compiled_target_hash_mismatch`, `compiled_target_unreadable`, `compiled_target_unverified`, and `compiled_expression_unloadable`, each with its meaning and next step unchanged. A code is a name a caller reads, not a value a store holds, so no older store is affected.
3. **The interfaces that carried the word are renamed:** the action `plan.show` is `target.show` (it prints a target's compiled target), and the field `plan_hash` of the actions' inputs and results is `compiled_target_hash`. The command, MCP tool, REST route, and SDK method follow from the registry.
4. **What is stored keeps the old word, so that what an older build wrote still opens and every hash stays the same.** Each is named where it is defined, with a pointer here:
   - the envelope every encoded compiled target starts with, `anything2mcp-plan 1`, so the bytes and every hash are those an older build made;
   - the ledger's columns `target_versions.plan_hash` and `target_versions.plan` (no migration);
   - the field `plan` of a stored diff, and of a trace's wire form, and the trace attribute `a2m.plan`;
   - the key `plan` of a published version's gate evidence;
   - the field name `plan` in the hashed input of a scout's trial, so a trial's hash is unchanged.
   The format number did not change, because no encoded byte did.
5. **Words that name something else stay:** the ledger's planned records (`plan_records`, `PlannedRecord`, the event `planned`), the scheduler's `plan` of a watch's due time, the program's `start::plan` of a launch, prose about planning work, `docs/plan/`, the Build Order, a step's plan in an approval, and the text of test data such as `Digest::of(b"plan")`. Old decision records, journal entries, research notes, and review results keep the word of their day.
6. **The profile `aspnet_webforms` is `server_postback_forms`,** its file `profiles/server_postback_forms.yaml`. Its comments and detection hints name no framework: it is detected by a hidden field that carries the page's state between postbacks, or by the page's own postback function. It states no `ready` rule, because its platform's own test is a call into the framework's script object and would name it; the rule was read and checked and never run, and `blur_then_wait_ready` waits for the form to be drawn again, not on that rule ([decision 0385](0385-the-set-step-commits-a-text-control-as-the-surfaces-platform-profile-says.md)). Its identifier patterns are unchanged. Decisions 0370 and 0385 keep the old name as history.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Rename the stored names too, with a migration and a new format number | Every hash of every published version would change, and a store or trace an older build wrote would need rewriting. The stored name is read by no person |
| Name the action `compiled_target.show` | Actions are grouped by the object a person names; a person shows a target, and what is printed is its compiled target |
| Keep the profile's framework-specific readiness script | It names a real framework, against rule 9, and nothing runs it |
