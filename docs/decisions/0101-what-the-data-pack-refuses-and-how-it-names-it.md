# 0101. What the data pack refuses, and how it names it

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Settles:** choices [spec section 307](../spec/3-packs/307-the-data-pack.md) left to the building package, and the question the builder of the stand `targets/example-db` asked about its own triggers.

## Decision

1. **A hazard is declared on the statement and checked against the catalog.** A `form: table` write states `hazard: data_elsewhere` when its table has a trigger or a key that cascades from it. A lint rule requires the operation to declare the same hazard, which makes it `irreversible`. Before a write with no declared hazard is sent, once per table in a session, the driver reads the catalog; a trigger on the table, or a key in any table that cascades from it, stops the write unsent as `drift`, `schema_changed`. *Instead of:* trusting the declaration alone, which would let a trigger added after publishing pass; *and instead of* deriving the hazard when files are compiled, which has no connection.
2. **The stand's own triggers are triggers.** The pack does not know the prefix `stand_`, and has no setting that hides a trigger. With the stand's write log on, every table has a trigger, so the fixtures' statements on logged tables declare the hazard, and exactly-once is proven with them. The rule for a table with no hazard is proven with the log off. *Instead of:* a list of names to ignore, which would be knowledge of a test stand inside the product and a way to hide a real trigger.
3. **A lock held by another connection is `busy`, `store_locked`.** The embedded engine locks the whole file. A wait that ends with the lock still held is a record that is locked, in the largest sense, and nothing was sent. `transient`, `timeout` is kept for a statement that ran past its time limit and was interrupted. Section 307 named both `timeout`.
4. **A writer takes its lock at `begin`.** A lock wait then ends before the statement is sent, and the failure says not sent.
5. **`commit` is never interrupted, and is refused unless the count of rows was checked.** A commit cut short is the doubt the step must not create. A store that answers "busy" to a commit did not commit, says so, and the driver rolls back; any other failure of a commit is unknown, the connection is closed, and the session sends nothing more.
6. **Stored values.** A field whose type is text, a choice, a decimal, a date, or a time must be stored as text; an integer as a whole number; a truth value and a version as either. Anything else is `drift`, `schema_changed`. A float and bytes are `unknown_variant`, `value_kind_unsupported`: no rule reads either exactly, and the open point of section 307 on floating columns stays open. *Instead of:* reading a whole number where decimal text was declared, which would compare `1250` with `12.50` and call it a mismatch when the column was in fact retyped.
7. **Bound values.** A decimal and a date are bound as their exact text; a truth value as 0 or 1. A decimal's text carries no trailing zero. A moment is refused until a target can state the form a store keeps it in.
8. **The store's error text is never kept.** It may quote a row. A refused constraint is reported by its kind, in the pack's own words.
9. **A write compiles only where the surface declares `stands: alone`.** `beneath` is refused with the same diagnostic until the authorization record can be read when files are compiled.

## Consequences

- The failure table of section 307 gains the row for a lock.
- The design of `a2m-pack-data` lists each failure with whether anything was sent.
