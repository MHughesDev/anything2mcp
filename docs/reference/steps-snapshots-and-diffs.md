# Steps, snapshots, and diffs

Three internal formats that several parts of the system share: the steps a compiled plan is made of, the snapshot an agent sees of a screen, and the diff a person approves. All three are built: the steps as the plan's types in `a2m-core`, the snapshot in the browser engine, and the diff with its hash in the engine and the ledger (package 3.5). Where this page and the code differ, the code is right and this page is to be corrected.

## Steps

The compiler lowers each binding's verb into a fixed list of steps ([spec section 206](../spec/2-kernel/206-the-compiler-and-the-plan.md)). A driver knows only how to carry out its own step kinds. Fixed in Phase 1 for `ui`, Phase 4 for the HTTP kinds, Phase 8 for the rest.

**The engine loop**, the same in every mode, has the eight parts of [spec section 207](../spec/2-kernel/207-the-engine-loop.md): `guard`, `choose`, `prepare`, `intend`, `act`, `confirm`, `read_back`, `record`. These are parts of a loop, not steps. The steps below are what the parts `act`, `confirm`, and `read_back` expand into, and "step" alone always means one of these.

| Driver | Step | Does |
| --- | --- | --- |
| `ui`, `desktop` | `goto` | Reach a screen by its `reach` rule |
| | `wait` | Wait for a condition, up to a timeout |
| | `locate` | Resolve a field or action to exactly one element and check its fingerprint |
| | `set` | Set a field through its widget driver |
| | `invoke` | Perform an action, such as pressing a button |
| | `read` | Read a field's current value |
| | `handle` | Deal with a declared variant, such as a dialog |
| | `upload`, `download` | Attach or receive a file |
| `ui` only | `observe` | Capture the response for a named endpoint while the page acts |
| `http` | `token` | Fetch a declared anti-forgery token. `page_http` has no such step: its `request` reads the page's token inside the page in the same call that sends the write ([decision 0206](../decisions/0206-the-page-http-mode-writes-by-put-with-the-pages-own-token-in-an-acting-tab.md)) |
| | `request` | Send a request built from an endpoint and expressions |
| | `validate` | Check the response against its schema |
| | `extract` | Take values out of the response |
| `api` | `call` | Call an operation from an imported description |
| `mcp`, `site_tools` | `call_tool` | Call a tool with typed arguments |

Every step carries: its kind, what it refers to, a timeout, and the failure class to raise if it cannot be done. A step never contains logic; branching exists only as declared variants and `when` expressions.

## Snapshots

A snapshot is what `screen.snapshot` returns and what evidence stores: a compact view of the current page or window, built from the accessibility tree. Fixed in Phase 1.

| Per snapshot | Per element |
| --- | --- |
| Address and title | A reference, stable within this snapshot |
| Which screen models match, if any | Role and accessible name |
| The platform profile detected | Value, where it has one |
| Frames, flattened, each element carrying its frame path | States: disabled, checked, expanded, required, invalid |
| Whether it was cut to fit the size limit, and a cursor to continue | Heading level or position in a list, where meaningful |

Rules:

- Hidden elements and decoration are left out.
- Values of password inputs and of fields labelled restricted are masked.
- The default is cut to about 2,000 tokens, deepest content first.
- The text form shown to agents is one element per line, indented by depth, reference first:

```text
page "Order A-100" https://app.example.com/orders/A-100/items
matches: order.line_item_form
  e1 heading "Line Item Details" level=2
  e2 textbox "Item code" value="L-101" required
  e3 textbox "Length" value="12' 6\\""
  e4 button "Save"
```

References such as `e3` are for exploring. Target files never contain them; they contain locators.

## Diffs

A diff is what a preview produces, what a person approves, and what an approval is bound to. Fixed in Phase 3.

| Per diff | Per record | Per field |
| --- | --- | --- |
| Tool and version hash | Record key | Name |
| Target, environment, identity | Action: `create`, `update`, `unchanged`, or `write` (its binding cannot read it) | Current value, raw and normalized |
| Counts: create, update, unchanged, write | The fields below | Proposed value, raw and normalized |
| When the current values were read | | Whether it changes |

**The diff hash.** An approval is bound to a hash of the diff, so the hash has to be the same wherever it is computed.

1. Take, in this order: the fixed purpose `anything2mcp diff 1`, the tool's name, its version hash, the target, the environment, and the identity (empty when there is none). Then the count of records, and for each record its key, its operation, its action, its evidence when that is not `read_back`, the binding the preview chose, and every binding it could take; then the count of its fields, and for each field its name with its normalized current and proposed values.
2. Leave out timestamps, raw display forms, and whether a field changes, which follows from the values.
3. Order records by key, then by step and place in the step, and fields by name.
4. Write each part as its name's length, its name, its value's length, and its value, each length eight bytes, least significant first (the field hasher of `a2m-core`). A value is in the canonical form of `a2m-core`: JSON with no optional spacing, names in order, a decimal as the text of the number with no zeros after its last digit (`{"decimal":"0.8"}`), a current value that is absent kept apart from every value by a leading byte.
5. Hash those bytes with SHA-256.

Until 2026-11-23 this list said that values are written "as strings" and named neither the tool, the operation, the evidence, nor the bindings. The engine has hashed as written above since those parts were added; the list was wrong, and the worked example below is held to the engine by a test.

**A worked example.** A tool creates record 3, updates record 1, and leaves record 2 as it is. `note` is labelled restricted, so the diff holds the hash of its normalized value and never its raw form; `0.10` was normalized to `0.1`. The diff as stored:

```json
{
  "tool": "example_items_item_save",
  "plan": "a98b37d20edd4d62b92554bb546f5168095fa0893ffb9680077c64b1209d9ba5",
  "target": "example",
  "environment": "test",
  "identity": "account:example_writer",
  "records": [
    {
      "step_seq": 0,
      "record_seq": 0,
      "operation": "example_items_item_save",
      "key": [3],
      "action": "create",
      "fields": [
        {"changes": true, "current": null, "current_raw": null, "name": "name", "proposed": "Bolt"},
        {"changes": true, "current": null, "current_raw": null, "name": "price", "proposed": {"decimal": "2.5"}},
        {"changes": true, "current": null, "current_raw": null, "name": "note", "proposed": "210cbba52e0424ec6b2d2d9b6a9430530c722eb32cd0996a50b55c23b5bff566"}
      ]
    },
    {
      "step_seq": 0,
      "record_seq": 1,
      "operation": "example_items_item_save",
      "key": [1],
      "action": "update",
      "fields": [
        {"changes": false, "current": "Nut", "current_raw": "Nut", "name": "name", "proposed": "Nut"},
        {"changes": true, "current": {"decimal": "0.75"}, "current_raw": {"decimal": "0.75"}, "name": "price", "proposed": {"decimal": "0.8"}},
        {"changes": true, "current": "d6d13014ef18d45030eec6ac0a66eab592e7f07e1feda93538935b3b434a8de8", "current_raw": null, "name": "note", "proposed": "80270e39ab5a8e50f949b1287e9432cef723e843964056ef04e1f185a4d3b301"}
      ]
    },
    {
      "step_seq": 0,
      "record_seq": 2,
      "operation": "example_items_item_save",
      "key": [2],
      "action": "unchanged",
      "fields": [
        {"changes": false, "current": "Washer", "current_raw": "Washer", "name": "name", "proposed": "Washer"},
        {"changes": false, "current": {"decimal": "0.1"}, "current_raw": {"decimal": "0.1"}, "name": "price", "proposed": {"decimal": "0.1"}}
      ]
    }
  ]
}
```

Its hash is `1b52a16688289f1af2c3a9b6a0cf2c7b84e468b6c628835ea70dff0e70e09017`. The test `the_worked_example_in_the_reference_hashes_to_the_value_it_prints` in `a2m-engine` reads this example and computes the hash on Windows, macOS, and Linux.

Consequences:

- The same proposal against the same current values always has the same hash, on any machine.
- If a record's current values have changed by the time it is written, it no longer matches the approved diff, and the run stops at that record as `stale` ([spec section 208](../spec/2-kernel/208-runs-the-ledger-and-resume.md)). The comparison is made record by record against the values the preview stored, not by hashing the whole diff again.
- A field labelled restricted enters the hash as a hash of its value, and is shown to the approver only if policy lets that approver see it.

**What the approver sees.** Counts first; then records that change, with current and proposed values side by side; unchanged records collapsed; anything the tool will do besides writing fields, such as an upload, stated in words.
