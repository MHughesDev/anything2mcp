name: sources

# Sources

A source is an input file that a pipeline reads: a sheet, a delimited file, or JSON. It is validated in full before any system is touched. A source is read only. A file the system itself keeps is a target file in the file pack, not a source.

## The definition

Files: `sources/*.yaml`.

| Field | Meaning |
| --- | --- |
| `template` | The template's name |
| `reader` | `xlsx`, `csv`, `json`, a plug-in's reader, or `operation` for the rows another operation returned |
| `versions` | Known template versions, each with `version` and a `fingerprint` of `parts`. A file with no known version is refused, with the differences |
| `locate` | Where the data lies. `document` is the default |
| `columns` | The header text of every column read, each once |
| `fields` | Row field name to `type` and either `column` or `expression` |
| `key` | Row fields that together identify a row. Two rows with one key are a problem |
| `rows` | An expression that picks which rows are records |
| `missing` | Text values that count as empty. "NA" is text unless listed |
| `hidden` | `reject` (default) or `include` |
| `options` | The reader's dialect, such as `delimiter` |

Example, from the `example` target:

```yaml
template: order_sheet_v2
reader: csv
versions:
  - version: "2"
    fingerprint:
      digest: "7c00941f8d452ede3c7f4908015699942a87aecdd932649718d72bf6851999c7"
      parts:
        - {kind: sheet, place: "sheet 1", value: "|shown"}
        - {kind: header, place: "column 1", value: "Code"}
        - {kind: header, place: "column 2", value: "Length"}
locate: document
columns: ["Code", "Length"]
fields:
  code: {type: identifier, column: "Code"}
  length: {type: length_ft_in, column: "Length"}
  label: {type: identifier, expression: "upper(row.Code)"}
key: [code]
rows: 'row.Code != ""'
missing: ["n/a"]
hidden: reject
```

- Every row field has a field type, so every value is typed before anything runs.
- An expression reads `row` and `source`, and must end in a type of its own: `upper(row.Code)` is text.
- The compiler works out `digest` from `parts` and refuses one that differs.
- A formula cell with no stored value fails. Formulas are never calculated.
- Data is found by header text or name, never by cell coordinates.

## How a file reaches a run

- A pipeline with a source has an input named `source`.
- On one machine the file is read only from an allowed root: the project folder, or a folder the person added. Put it under `sources/` in the project and name it as `sources/items.csv`.
- Links are resolved before the check. A renamed file whose content is not the type it claims is refused.
- On a server a caller uploads content instead of passing a path.
- You cannot name any other file, and the system's text never picks one.
