name: pack-file

# The file pack, mode `file`

**Latches onto.** A file in a known format that is the system itself: a price list, a stock file, a settings file. Surface kind: `file`, with `folder`, and `stands`. The target names the file. No input ever does. This is not a source: a source is an input a pipeline reads.

**File kind.** `documents/`: the file's name, format, layout fingerprint, and the named places in it.

**Verbs.** `read`, `list`, `update`, `create`, `delete`.

**Steps.** `open`, `locate`, `set`, `read`, `save`.

**Formats.** Delimited text and JSON are read and written. A workbook is read only. A format the pack cannot write back without loss is read only.

## A document, from the `example` target

```yaml
surface: price_files
file: price-list.csv
format: delimited
may_be_open: false
records:
  price:
    rows: all
    key: [item_code]
    fields:
      item_code: {header: item_code, type: identifier}
      unit_price: {header: unit_price, type: rate, writable: true}
```

A binding of mode `file`:

```yaml
bindings:
  - mode: file
    verb: update
    document: price_list
    record: price
    key: {item_code: input.item_code}
    fields: {unit_price: input.unit_price}
    read_back: {record: price}
```

## Rules

- A place is a row by key, a path of keys in a structured file, or a named cell. Never a cell coordinate.
- `writable: true` marks a field a binding may set. Nothing else is ever set.
- A document that a binding writes states `may_be_open`, `true` or `false`. When `true`, it also states the sign that the file is open.
- A save writes a new file and replaces the old one in one step. The file is then opened again and read back. Bytes that were not touched stay as they were.
- A value in delimited text that starts with `=`, `+`, `-`, or `@` is refused (`value_begins_formula`), unless the column says `may_begin_formula: true`. The value is never changed to make it safe.
- A surface with a write declares `stands: alone`, or `beneath: <surface>`. A write beneath an application is irreversible and needs the owner's word.
- Limits on bytes, rows, cells, and bytes per cell are settings. A file larger than they allow is refused as `file_too_large`.

## Refuses

- Any input that chooses the file, or a path that leaves its folder.
- Editing in place. Macros, embedded code, external links, and formulas: their bytes are copied untouched and never run.
- An encrypted, protected, or signed file.
- Writing while an application has the file open, where that can be told.
- Writing a file the engine's user may not write.

## Codes

`drift`: `file_missing`, `file_unreadable`, `place_not_found`, `key_not_unique`, `fingerprint_mismatch`, `round_trip_failed`. `unknown_variant`: `formula_at_place`, `metadata_not_kept`, `value_unreadable`, `shape_unsupported`. `policy`: `file_protected`, `file_too_large`. `busy`: `file_locked`, `storage_full`. `stale`: `changed_since_preview`. `input`: `payload_unreadable`, `value_begins_formula`, `value_unparsable`, `value_out_of_range`.
