# 28. Observability, audit, and repair

Every run can be replayed afterward, every change can be traced to a person's approval, and a UI change is caught before a write meets it. [Section 47](47-traces-and-evidence.md) defines the trace format, capture levels, and redaction.

- **Trace per run.** Each step records timing, the locator that matched, the widget-driver rung used, the intended and read-back values, and the failure class if any. Step traces and screenshots are kept at checkpoints and failures, with secrets and marked fields masked.
- **Audit log.** Append-only and hash-chained, so a removed or edited entry is detectable. Each entry holds who called, which tool version, the input hash, who approved, the diff hash, and the outcome.
- **Metrics.** Success rate, mismatch rate, retries, and duration per tool version, exported through OpenTelemetry. A rising retry or drift rate is the early sign of a UI change.
- **Retention.** Traces kept 30 days by default, audit entries 1 year; both configurable to company policy.

**Repair loop.** When a canary or a run reports `drift`:

1. The affected tools are marked unavailable; reads through other tools continue.
2. The trace, a page snapshot, and the failing field go to an AI agent, which proposes a change to a screen model or endpoint: a new locator, a fingerprint update, a schema update, or a new platform-profile rule.
3. The change runs through the publish gates against the live app and the fixtures.
4. A person approves the diff, and it is published as a new version.

Because locators and endpoints each live in one place, one repair fixes every operation and pipeline that uses them. Web2MCP never heals a locator during a run, since a wrong match on a write path silently corrupts data.
