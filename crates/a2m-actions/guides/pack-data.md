name: pack-data

# The data pack, mode `sql`

**Latches onto.** A relational store. As built: an embedded store that is a file in the project folder. No server store, no network. Surface kind: `database`, with `store`, `file`, and `stands`.

**File kind.** `statements/`: record kinds on a surface, and every statement a binding may run.

**Verbs.** `read`, `list`, `search`, `create`, `update`, `delete`.

**Steps.** `begin`, `execute`, `fetch`, `expect_rows`, `commit`, `catalog`. Only `commit` saves, and it always does.

## A statements file, from the `example` target

```yaml
surface: orders_db
records:
  line_item:
    table: order_lines
    key: [order_id, item_code]
    version: row_version
    fields:
      order_id: {column: order_id, type: identifier}
      item_code: {column: item_code, type: identifier}
      length: {column: length_in, type: length_inches}
statements:
  line_item_by_key:
    effect: read
    form: table
    record: line_item
    time_limit: 2s
  line_item_update:
    effect: write
    form: table
    record: line_item
    verb: update
    set: [length]
    guard: version
    hazard: data_elsewhere
    time_limit: 5s
```

The surface says `stands: alone`, or `beneath: <surface>` when an application owns the store. With neither, a write binding does not compile. A binding of mode `sql` names a `statement`, and a write names a read statement as `read_back`.

## Rules

- A `form: table` statement is written by the pack, from the record. A `text` statement is reviewed, hashed with the plan, and uses `?` parameters declared in `params`.
- A read-back is one read by the record's key. A key that identifies more than one row is `key_not_unique`.
- `guard: version` sends the row version as a precondition. A change by someone else is `stale`, `version_conflict`.
- A write is one transaction.
- A write to a store beneath an application needs the owner's word in the authorization record, and is `irreversible`.
- A write statement declares its hazard, and the operation declares the same one (lint `data.hazard_not_declared`).

## Refuses

- A statement built from inputs. A table, column, or sort order chosen by an input.
- A statement that changes the schema, grants rights, or manages the store, or one that opens another file or begins or ends a transaction.
- A read statement that writes (`read_statement_wrote`).
- A shared administrative identity.
- Sampling values in discovery unless a person asks.

## Codes

`transient`: `upstream_unavailable`. `busy`: `store_locked`. `rejected`: `constraint_violated`. `policy`: `denied_by_store`, `read_statement_wrote`, `statement_not_allowed`. `drift`: `schema_changed`, `read_not_by_key`, `key_not_unique`. `stale`: `changed_since_preview`, `version_conflict`. `unknown_variant`: `unexpected_status`, `value_kind_unsupported`, `result_too_large`, `catalog_too_large`. `input`: `value_not_bindable`, `payload_unreadable`.
