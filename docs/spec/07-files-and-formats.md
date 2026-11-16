# 7. Files and formats

Everything the system knows about a target is a folder of YAML files, each checked against a published JSON Schema before it loads. The files are compiled into a plan before anything runs ([section 46](46-target-compiler-and-plan.md)). Every field of every file is listed in the [file format reference](../reference/file-formats.md). Names and fields below are placeholders for an imaginary application called `example`; they describe no real product.

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

**Target.** Declares the surfaces and how they relate, so a large app with several services is one target.

```yaml
target: example
surfaces:
  web: {kind: web, origin: "https://app.example.com", profile: auto}
  api: {kind: http, base: "https://api.example.com", same_session_as: web}
environment: {locale: en-US, timezone: UTC, viewport: [1440, 900]}
```

**Operation with two bindings.** Inputs, outputs, effect, and key are defined once; each binding says how one mode does it.

```yaml
operation: example_line_item_upsert
effect: write
key: item_code
inputs:
  order_id:  {type: string, description: "Order number shown in the application"}
  item_code: {type: string, description: "Item code, such as L-101"}
  length:    {type: length_ft_in}
outputs:
  line_item: {schema: "#/endpoints/line_item_get/response"}
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
    read_back: {screen_field: length, prefer: response}
description: >
  Creates or updates one line item on an order. Prefer example_order_sheet_enter for whole sheets.
```

**Screen model** (UI and desktop drivers) holds ordered locators and a fingerprint per field. **Endpoint** (HTTP drivers) holds the method, path template, headers the session keeper supplies, and request and response schemas.

```yaml
screen: order.line_item_form
reach: {url: "/orders/{order_id}/items"}
ready: {visible: {role: heading, name: "Line Item Details"}}
fields:
  length:
    type: length_ft_in
    control: text
    locate: [{label: "Length"}, {role: textbox, name: "Length"}]
    fingerprint: {near_text: "Length", section: "Dimensions"}
actions:
  save: {locate: [{role: button, name: "Save"}], confirm: {response: line_item_put}}
---
endpoint: line_item_put
surface: api
method: PUT
path: "/api/orders/{order_id}/items/{item_code}"
auth: session            # cookie or bearer, supplied by the session keeper
request_schema: schemas/line_item_put.request.json
response_schema: schemas/line_item_put.response.json
```

**Pipeline** names operations only, never screens, endpoints, or modes:

```yaml
pipeline: example_order_sheet_enter
effect: write
source: {reader: xlsx, template: order_sheet_v2, mapping: order_sheet_to_line_items}
limits: {max_records: 50}
steps:
  - {id: check, operation: example_order_get, expect: "result.status == 'draft'"}
  - {id: enter, for_each: source.rows, operation: example_line_item_upsert, checkpoint: per_record}
  - {id: totals, operation: example_order_totals_get}
success: ["all_records_verified"]
```

Expressions use CEL, which always terminates and never converts types silently. Conversions such as `to_inches` are a small library of tested host functions. There is no general scripting in YAML; anything that truly needs code goes in a reviewed plug-in.
