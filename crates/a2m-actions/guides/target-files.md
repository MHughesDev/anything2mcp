name: target-files

# Target files

A target is a folder, `targets/<name>/`, in the project. Its files say everything the runtime knows about one system. Field names are lowercase with underscores. Write each fact once.

| File or folder | Holds | Brought by |
| --- | --- | --- |
| `target.yaml` | The target's name, its surfaces, its environments, the test records | The kernel |
| `auth.yaml` | How sign-in works for each group of surfaces. No credential, only references | The kernel |
| `types.yaml` | Field types: units, steps, formats, codes | The kernel |
| `operations/` | One business action each, with its bindings | The kernel |
| `pipelines/` | An ordered run of operations over a source | The kernel |
| `sources/` | How an input file is read | The kernel |
| `tests/`, `fixtures/` | Offline tests and scrubbed recordings | The kernel |
| `screens/`, `endpoints/`, `mappings/`, `commands/`, `statements/`, `documents/`, `channels/`, `schemas/`, `registers/` | What one kind of system needs | A pack. See its page |

A `---` line separates entries in one file.

## A small target: `example`

`target.yaml`:

```yaml
schema_version: 1
target: example
title: Example order entry
surfaces:
  desk:
    kind: stand
    address: "https://app.example.com"
    rate: {requests: 10, per_ms: 1000}
environments:
  test:
    kind: test
    surfaces: {desk: "https://test.example.com"}
    test_records: ["SO-1001"]
default_environment: test
environment: {locale: en-US, timezone: UTC}
drift_risk: low
```

`types.yaml`:

```yaml
types:
  order_status:
    base: choice
    codes: {draft: "Draft", released: "Released"}
  amount:
    base: money
    currency: USD
    rounding: half_even
```

`operations/order_get.yaml`:

```yaml
operation: example_order_get
title: Get an order
effect: read
key: order_id
inputs:
  order_id: {type: identifier, description: "Order number shown in the application"}
outputs:
  status: {type: order_status}
  total: {type: amount}
bindings:
  - mode: stand
    status: verified
    call: order_get
description: >
  Reads the status and the total of one order.
```

`kind: stand` is the kind of the built-in test application. A real surface names a kind a pack registers, such as `web` or `http`. The binding's own fields (`call` here) belong to its mode.

## Rules that bite

- Surface address, rate, and sign-in live in `target.yaml` and `auth.yaml`. An environment gives its kind (`test` or `production`), may override surface addresses, and names the test records.
- `test_records` are the only records a draft or a gate may write. Each is one value, or a list for a key of several parts, parent first.
- The tool name is `{target}_{module}_{object}_{verb}`. The module part is left out when the target declares no `modules`.
- `drift_risk` is `low`, `medium`, or `high`.
- Every operation needs `effect`, `inputs`, `outputs`, `bindings`, and `description`. A write also needs an identity (see `identity`) and a read-back.
- `status` of a binding is `draft`, `shadow`, `verified`, or `retired`. Only a person promotes a shadow binding to verified.
- A key that means nothing for a field type's base is refused, not ignored.
- `types.yaml` says units and formats. An operation does not.
- A secret is never a value in these files. `auth.yaml` holds a reference such as `vault://example/service/password`.
- Run `draft.lint` after every change. It reports every problem at once, with a file, a line, a rule, and a fix.

Built-in field types: `text`, `integer`, `decimal`, `money`, `date`, `datetime`, `time`, `boolean`, `choice`, `length_ft_in`, `length_metric`, `percent`, `email`, `phone`, `identifier`, `url`, `file`, `untrusted_text`. A field type of your own goes in `types.yaml`. Text that comes from the system, such as a note, should be `untrusted_text` (text marked untrusted) or a type of your own with `untrusted: true`.
