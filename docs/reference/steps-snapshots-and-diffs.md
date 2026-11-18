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
| Target, environment, identity | Action: `create`, `update`, or `unchanged` | Current value, raw and normalized |
| Counts: create, update, unchanged | The fields below | Proposed value, raw and normalized |
| When the current values were read | | Whether it changes |

**The diff hash.** An approval is bound to a hash of the diff, so the hash has to be the same wherever it is computed.

1. Take the tool's version hash, the target, the environment, the identity, and for each record its key, action, and each field's name with its normalized current and proposed values.
2. Leave out timestamps and raw display forms.
3. Order records by key and fields by name.
4. Write it in a canonical form: fixed field order, values as strings, UTF-8, no optional spacing.
5. Hash those bytes with SHA-256.

Consequences:

- The same proposal against the same current values always has the same hash, on any machine.
- If a record's current values have changed by the time it is written, it no longer matches the approved diff, and the run stops at that record as `stale` ([spec section 208](../spec/2-kernel/208-runs-the-ledger-and-resume.md)). The comparison is made record by record against the values the preview stored, not by hashing the whole diff again.
- A field labelled restricted enters the hash as a hash of its value, and is shown to the approver only if policy lets that approver see it.

**What the approver sees.** Counts first; then records that change, with current and proposed values side by side; unchanged records collapsed; anything the tool will do besides writing fields, such as an upload, stated in words.
