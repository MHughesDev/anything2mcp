name: operations

# Operations and pipelines

An operation is one business action, defined once. A pipeline is an ordered run of operations, usually over the rows of a source file. Business steps live here and nowhere else.

## An operation

Files: `operations/*.yaml`.

| Field | Meaning |
| --- | --- |
| `operation` | The tool name: `{target}_{module}_{object}_{verb}` |
| `effect` | `read`, `write`, or `irreversible` |
| `key` or `identity` | How the record is found again. Required for a write. See `identity` |
| `commit_points` | How many times the system saves. At least 1 for a write. A read states none |
| `inputs`, `outputs` | Maps of name to `type` and `description`. An output may say `list: true` |
| `bindings` | One or more. Each names a `mode`, and the fields that mode needs |
| `hazard` | What else it could damage. `none` unless given. Any other hazard needs `effect: irreversible` |
| `description` | What it does, when to prefer something else, and its limits |
| `examples` | Example inputs, each a map of input name to value |

A binding has a `mode`, a `status` (`draft`, `shadow`, `verified`, `retired`), a read-back for a write, and the fields of its mode. The mode comes from a pack. See its page.

Hazards: `none`, `data_elsewhere`, `sends_outside`, `device_function`, `physical`. Any hazard but `none` means a person approves every run.

A write that example runs:

```yaml
operation: example_line_item_upsert
title: Create or update a line item
effect: write
key: [order_id, item_code]
commit_points: 1
inputs:
  order_id: {type: identifier, description: "Order number shown in the application"}
  item_code: {type: identifier, description: "Item code, such as L-101"}
  length: {type: length_ft_in, description: "Length, in inches"}
outputs:
  length: {type: length_ft_in}
bindings:
  - mode: stand
    verb: upsert
    status: shadow
    call: line_item_put
    args: {length: input.length}
    read_back: {call: line_item_get}
description: >
  Creates or updates one line item on an order.
  Prefer example_order_sheet_enter for whole sheets.
```

Tips:

- Write `description` for the caller who has not read your files. Say when to use a neighbouring tool instead.
- Mention a side effect in `description`, and give it a hazard.
- Take values from inputs with `input.<name>`. A placeholder in a path takes the input of the same name.

## A pipeline

Files: `pipelines/*.yaml`.

| Field | Meaning |
| --- | --- |
| `pipeline` | The tool name |
| `inputs` | As for an operation |
| `source` | `template`: a file under `sources/`. It adds an input named `source` |
| `limits` | `max_records`. Required for a write |
| `steps` | An ordered list. Each has `id` and `operation`, and may have `with`, `for_each`, `checkpoint`, `when`, `expect` |
| `success` | Conditions for `completed`, such as `all_records_verified` |

```yaml
pipeline: example_order_sheet_enter
title: Enter an order sheet
description: >
  Enters every row of an order sheet as a line item of one draft order,
  then reads the order back. Stops at the first row that fails.
effect: write
inputs:
  order_id: {type: identifier, description: "Order the sheet belongs to"}
source: {template: order_sheet_v2}
limits: {max_records: 50}
steps:
  - {id: check, operation: example_order_get, with: {order_id: input.order_id},
     expect: "result.status == 'draft'"}
  - {id: enter, for_each: source.rows, operation: example_line_item_upsert,
     with: {order_id: input.order_id, item_code: row.code, length: row.length},
     checkpoint: per_record}
  - {id: totals, operation: example_order_get, with: {order_id: input.order_id}}
success: [all_records_verified]
```

- Inputs reach a step only through `with`. Nothing is passed by matching names.
- A step stops on failure. There is no continue-on-error.
- Expressions use the safe subset of CEL. They have no floats: write `decimal("12.50")`.
- A record found `unchanged` counts as verified, so running the same pipeline twice writes nothing the second time.

## A test

Files: `tests/*.yaml`. `kind: offline` runs against a fixture folder under `fixtures/`. `kind: live` runs against a test record.

```yaml
test: example_order_get
kind: offline
fixture: order_get
inputs: {order_id: "SO-1001"}
expect: {status: draft, total: "125.00", note: "Call before delivery"}
```

A fixture folder holds a `fixture.yaml` that states `scrubbed: true`, and one file per step it answers, named after what the step refers to (the call, the endpoint, the statement, the document, or the upstream tool; or the step's kind when it refers to none): `<name>.json`. One object of the values the step observed, or `null` for a record that is absent, answers every step of that name; a list answers them in order, each once, which a write needs: the record before the save, then after it. A save also has `<name>.request.json`: the values the engine must send it, an object, or a list in the order the saves are sent. A pipeline's source is a file of the same folder, named by the test's input `source`. Write an exact number as text. Never commit recorded business data.

```yaml
test: example_line_item_upsert
kind: offline
fixture: line_item_upsert
inputs: {order_id: "SO-1001", item_code: "L-101", length: "90.5"}
expect: {length: "90.5"}
```

`test.run` runs each offline test through the engine against these answers: no system is reached, no session is used, and nothing is written to the installation. A test of a write runs in a ledger of its own, thrown away when the test ends: its intent is recorded, its save is answered from the fixture, and its read-back is compared, as in a real run. The result is compared with `expect`, field by field; a field the result does not hold may name the run itself: `state`, `records_verified`, `failure`, or `sent`, the number of saves sent. A test of a write with no `<name>.request.json` is not run; one that expects the record to hold the intended values already says `sent: 0`, and passes when nothing is sent, but the gate counts a write as tested only by a test that sent its save. `fixture.record` records a read's fixture and test from one real read, every value scrubbed by rule. A field that differs fails the test and the result names it; a save sent with other values than the request recorded fails it and names the field; a step the fixture has no answer for fails it as `fixture_no_answer`. A passing test proves that the binding turns the recorded answers into the expected result and sends the recorded request; it does not prove the system still answers so. A live test is listed as `not_run`.

A version is published only when every offline test has run and passed: `version.publish` and `publish.request` run them again and refuse with `publish_gates_not_passed`, naming each test that did not pass. The person's page lists each gate that ran, and those that are not built.
