# 205. Target files

Everything the system knows about a target is a folder of YAML files, each checked against a published JSON Schema before it loads. The files are compiled into a plan before anything runs ([section 206](206-the-compiler-and-the-plan.md)). Every field of every file is listed in the [file format reference](../../reference/file-formats.md). Names and fields below are placeholders for an imaginary system called `example`, here a web application with an HTTP interface beside it; they describe no real product.

```text
targets/example/
  target.yaml        # surfaces, platform profile, environment, defaults
  auth.yaml          # sign-in method per surface, session rules
  screens/           # screen models (UI and desktop)
  endpoints/         # endpoint catalog (HTTP, page_http, API)
  types.yaml         # field-type overrides: units, rounding, code lists
  operations/        # one file per operation, with its bindings
  pipelines/         # one file per pipeline
  sources/           # readers, template fingerprints, mappings
  fixtures/          # recorded pages, responses, and traffic for offline tests
  tests/             # expected results per operation and pipeline
```

Some of these file kinds are the kernel's and the rest arrive with packs ([section 202](202-kernel-and-packs.md)). Each kind has one owner, and registration is keyed by the file kind and the surface kind; a pack that needs another pack's kind declares that it reads it, and receives the entries as data.

| File kind | Owner | Read by |
| --- | --- | --- |
| `target.yaml`, `auth.yaml`, `types.yaml`, `operations/`, `pipelines/`, `sources/`, `tests/`, `fixtures/` | Kernel | Every pack, through the compiler |
| `screens/`, for web surfaces | The web pack | None |
| `screens/`, for desktop surfaces | The desktop pack | None |
| `endpoints/` | The HTTP pack | The web pack, for its `page_http` mode |
| New kinds, such as `commands/` for the command pack | The pack that brings them | Whichever packs declare they read them |

A surface's `kind` is a surface kind its pack registers; a binding's `mode` is a mode its pack registers. A name nobody registered does not compile.

The folder is read as it stands and nothing outside it is read. A link that leaves the folder is refused, and so is a link to a folder, which would have the same files read again under another name. A target file holds at most 1 MiB and a fixture's recorded file at most 16 MiB. The drafter writes only regular files inside the folder, never through a link and never over a file a person wrote or changed ([`a2m-files`](../../architecture/crates/a2m-files.md)).

**Target.** Declares the surfaces and how they relate, so a large system with several services is one target.

```yaml
schema_version: 1
target: example
surfaces:
  web: {kind: web, origin: "https://app.example.com", profile: auto, viewport: [1440, 900]}
  api: {kind: http, base: "https://api.example.com", same_session_as: web}
environment: {locale: en-US, timezone: UTC}
```

**Operation with two bindings.** Inputs, outputs, effect, and key are defined once; each binding says how one mode does it. A line item is unique by its order and its code together, so both inputs form the key ([section 209](209-record-identity-and-dependent-records.md)). A placeholder in a path or a `reach` address takes the input of the same name.

```yaml
operation: example_line_item_upsert
effect: write
key: [order_id, item_code]
inputs:
  order_id:  {type: string, description: "Order number shown in the application"}
  item_code: {type: string, description: "Item code, such as L-101"}
  length:    {type: length_ft_in}
outputs:
  length: {type: length_ft_in}
bindings:
  - mode: http
    status: shadow
    endpoint: line_item_put
    body: {length: "to_inches(input.length)"}
    read_back: {endpoint: line_item_get}
  - mode: ui
    status: verified
    screen: order.line_item_form
    verb: upsert
    fields: {length: input.length}
    action: save
    read_back: {screen_field: length, prefer: response}
description: >
  Creates or updates one line item on an order. Prefer example_order_sheet_enter for whole sheets.
```

**Screen model** (the web pack's `ui` driver and the desktop pack's driver) holds ordered locators and a fingerprint per field. **Endpoint** (the HTTP pack's drivers, and the web pack's `page_http`) holds the method, path template, headers the session keeper supplies, and request and response schemas.

```yaml
screen: order.line_item_form
reach: {url: "/orders/{order_id}/items/{item_code}"}
ready: {visible: {role: heading, name: "Line Item Details"}}
fields:
  length:
    type: length_ft_in
    control: text
    locate: [{label: "Length"}, {role: textbox, name: "Length"}]
    fingerprint: {near_text: "Length", section: "Dimensions"}
actions:
  save: {locate: [{role: button, name: "Save"}], confirm: {response: line_item_put}, commits: true}
---
endpoint: line_item_put
surface: api
method: PUT
path: "/api/orders/{order_id}/items/{item_code}"
auth: session            # cookie or bearer, supplied by the session keeper
request_schema: schemas/line_item_put.request.json
response_schema: schemas/line_item_put.response.json
```

**Pipeline** names operations only, never screens, endpoints, or modes. The source file names the reader, lists the sheet's columns, and gives each row field a field type and the column or expression it comes from; each step passes values to its operation's inputs with `with`:

```yaml
pipeline: example_order_sheet_enter
description: Enters every row of an order sheet as a line item of one draft order.
effect: write
inputs:
  order_id: {type: string, description: "Order the sheet belongs to"}
source: {template: order_sheet_v2}
limits: {max_records: 50}
steps:
  - {id: check, operation: example_order_get, with: {order_id: input.order_id},
     expect: "result.status == 'draft'"}
  - {id: enter, for_each: source.rows, operation: example_line_item_upsert,
     with: {order_id: input.order_id, item_code: row.code, length: row.length},
     checkpoint: per_record}
  - {id: totals, operation: example_order_totals_get, with: {order_id: input.order_id}}
success: [all_records_verified]
```

The sheet itself arrives as the pipeline's reserved `source` input, a `file` ([file format reference](../../reference/file-formats.md)).

Expressions use CEL, which always terminates and never converts types silently. Conversions such as `to_inches` are a small library of tested host functions. There is no general scripting in YAML; anything that truly needs code goes in a reviewed plug-in.
