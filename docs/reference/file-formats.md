# Target file formats

Every field of every target file. [Spec section 205](../spec/2-kernel/205-target-files.md) shows worked examples; this is the field-by-field reference the JSON Schemas are written from in Phase 0, task B3.

This is a design draft. Field names can still change while the schemas are written; after the first release, a change is a new schema version with a migration.

**Who owns each kind of file.** The kernel owns `target.yaml`, `auth.yaml`, `types.yaml`, `operations/`, `pipelines/`, `sources/`, `tests/`, and `fixtures/`. Every other kind is brought by a pack, which registers its schema: `screens/` by the web pack and, for desktop surfaces, the desktop pack; `endpoints/` by the HTTP pack; `mappings/` by the MCP pack. The packs designed and not yet built bring their own, each given field by field in its spec section: `commands/` (command pack), `statements/` (data pack), `documents/` (file pack), `channels/` (stream pack), and `registers/` (device pack). A pack may read a kind it does not own; the compiler hands it the validated entries as data. An entry of a kind nobody registered does not compile.

Conventions: names are lowercase with underscores. *Required* fields are marked. A field that takes an *expression* takes CEL ([expressions](#expressions)). A field that takes a *condition* takes one of the [conditions](#conditions).

## `target.yaml`

| Field | Meaning |
| --- | --- |
| `schema_version` (required) | Version of these formats the files are written in |
| `target` (required) | The target's name; the prefix of every tool name |
| `title` | Name shown to people |
| `surfaces` (required) | Map of surface name to a surface (below) |
| `environments` | Map of environment name to an environment (below); at least `production` is implied |
| `default_environment` | Used when a call names none |
| `environment` | Fixed run settings: `locale`, `timezone`, `viewport` |
| `modules` | Module names, for large applications |
| `defaults` | `timeouts`, `limits`, and `capture` level applied to every operation unless overridden |
| `exclusive_session` | True when the application allows one session per account |
| `drift_risk` | `low`, `medium`, or `high`; sets how often canaries run |
| `authorization` | Reference to the record of who agreed to this target being wrapped; required to publish on a server |

**Surface.**

| Field | Meaning |
| --- | --- |
| `kind` (required) | A surface kind that an enabled pack registers. The first four packs register `web`, `http`, `mcp`, and `desktop`; the designed packs add `program`, `database`, `file`, `broker`, and `device_link`. The fields below the kind are the registering pack's |
| `origin`, `base`, `server`, `app` | The address for the kind: a web origin, an API base address, an MCP server address, or a desktop application identifier. Other kinds have their own, given in their pack's section |
| `stands` | For a surface that may sit under an application, such as a database or a file: `alone`, or `beneath` naming the surface of the application that owns it. With neither, no write binding on the surface compiles |
| `profile` | A platform profile name, or `auto` to detect |
| `same_session_as` | Another surface whose sign-in this one shares |
| `rate` | Requests allowed per period |

**Environment.**

| Field | Meaning |
| --- | --- |
| `kind` (required) | `test` or `production` |
| `surfaces` | Address overrides per surface |
| `test_records` | Record keys that drafts and publish gates may write to |
| `approve_writes` | A stricter approval rule for this environment |

## `auth.yaml`

One entry per group of surfaces that share a sign-in.

| Field | Meaning |
| --- | --- |
| `surfaces` (required) | The surfaces this entry covers |
| `mode` (required) | `use_existing`, `managed_session`, or `custom` |
| `provider` (required) | An auth provider from the technical reference |
| `probe` (required) | A cheap signed-in read: an operation name or an endpoint |
| `expiry_signals` | List of signals that the session has ended: `redirect_host`, `status` with `path`, or `visible` with a locator |
| `timeouts` | Declared `idle` and `absolute` durations, when known |
| `sign_out` | An endpoint or action that ends the session |
| `credentials` | For `custom`: vault references only, such as `vault://example/service/password` |
| `attempt_limit` | Automatic sign-in attempts with stored credentials; default 1 |
| `movable` | `auto` or `false`; `false` pins the session to its runner |

## `types.yaml`

Field types say how a value is formatted, normalized, compared, and classified.

| Field | Meaning |
| --- | --- |
| `base` (required) | `text`, `integer`, `decimal`, `money`, `measurement`, `date`, `datetime`, `time`, `boolean`, or `choice` |
| `unit`, `display_unit` | For measurements: the unit stored and the unit shown |
| `step` | The smallest increment, such as 1/16 inch |
| `places`, `rounding` | Decimal places and rounding mode |
| `currency` | For money |
| `format` | How the application displays and accepts the value. For a number it is the decimal mark: `.` or `,` |
| `timezone` | For dates and times: `fixed`, `user`, or a named zone. Only `UTC` and a fixed offset such as `+02:00` are built so far; a named zone or the `user` rule cannot be normalized yet |
| `case`, `whitespace` | Text comparison rules. `case` is `sensitive` or `insensitive`; `whitespace` is `collapse`, `trim`, or `exact` |
| `codes` | For choices: map of stored code to label |
| `sensitivity` | `public`, `internal`, or `restricted` |
| `untrusted` | True for free text that came from the application |

Built-in types: `text`, `integer`, `decimal`, `money`, `date`, `datetime`, `time`, `boolean`, `choice`, `length_ft_in`, `length_metric`, `percent`, `email`, `phone`, `identifier`, `url`, `file`. A `url` input accepts only destinations the operation declares. A `file` input is a blob reference on a server and a path inside an allowed root on one machine ([spec section 502](../spec/5-security/502-defending-against-a-manipulated-agent.md)). A target adds its own by extending one of them.

## `screens/*.yaml`

Brought by the web pack, and by the desktop pack for desktop surfaces.

| Field | Meaning |
| --- | --- |
| `screen` (required) | `module.name` |
| `surface` | The surface it lives on; default is the first web or desktop surface |
| `reach` (required) | How to get there: `url` with placeholders, or `from` another screen with an action |
| `ready` (required) | A condition that is true when the screen can be used |
| `frame` | A declared frame path, when the screen is inside a frame |
| `fields` | Map of field name to a field (below) |
| `actions` | Map of action name to an action (below) |
| `rows` | For a grid: how to find a row by key, and its columns as fields |
| `variants` | Named states that can appear, such as a confirmation dialog, each with a condition and a handling: `confirm`, `dismiss`, or `stop` |

**Field.**

| Field | Meaning |
| --- | --- |
| `type` (required) | A field type |
| `control` (required) | A widget driver from the technical reference |
| `locate` (required) | Ordered list of [locators](#locators) |
| `fingerprint` (required) | What must also be true of the matched element: `near_text`, `section`, `role` |
| `read` | `value`, `text`, or an attribute name; default by control |
| `commit` | What commits the value in the page: `blur`, `enter`, or `none` |

**Action.**

| Field | Meaning |
| --- | --- |
| `locate` (required) | Ordered list of locators |
| `confirm` (required) | The condition that shows the action took effect |
| `commits` | True when the action saves in the application; counted as a commit point. This is what the compiler counts ([spec section 206](../spec/2-kernel/206-the-compiler-and-the-plan.md)); an action without it is not a save |

### Locators

Tried in order; the first that matches exactly one element and passes the fingerprint wins.

| Locator | Matches by |
| --- | --- |
| `{role, name}` | Accessibility role and accessible name |
| `{label}` | The text of the label associated with the control |
| `{placeholder}` | Placeholder text |
| `{text}` | Visible text, exact unless `contains: true` |
| `{test_id}` | A test identifier attribute, when the application has stable ones |
| `{title}` | Title or tooltip text |
| `{css}` | A selector; allowed only last in the list, and flagged by the linter |
| `within` | Added to any locator to limit it to a named region of the screen |

Position alone, and XPath, are not accepted.

### Conditions

| Condition | True when |
| --- | --- |
| `{visible: locator}`, `{hidden: locator}` | The element is shown, or is not |
| `{enabled: locator}` | The element can be used |
| `{text: {locate, equals or contains}}` | The element's text matches |
| `{value: {field, equals}}` | A field holds a value, after normalization |
| `{url: pattern}` | The page address matches |
| `{response: endpoint}` | A response for that endpoint arrived, with an optional `status` |
| `{profile_ready: true}` | The platform profile's own readiness rule holds |
| `{all: [...]}`, `{any: [...]}` | Combinations |

There is no condition that waits for a length of time or for the network to go quiet.

## `endpoints/*.yaml`

Brought by the HTTP pack. The web pack's `page_http` mode reads it.

| Field | Meaning |
| --- | --- |
| `endpoint` (required) | Its name |
| `surface` (required) | An `http` surface, or a `web` surface for requests made from inside the page |
| `method`, `path` (required) | The method and a path template |
| `query`, `headers` | Templates; no secrets |
| `auth` | `session`, `token`, or `none`; supplied by the session keeper |
| `anti_forgery` | Where the token comes from (`cookie`, `meta`, or another endpoint) and the header it goes in |
| `request_schema`, `response_schema` (response required) | JSON Schema files |
| `effect` (required) | `read` or `write` |
| `idempotency_header` | Header that carries the record's idempotency token, if the application accepts one |
| `version` | Where a record's version is read and how it is sent back as a precondition |
| `volatile` | Request fields to ignore when comparing with a shadow |

## `operations/*.yaml`

| Field | Meaning |
| --- | --- |
| `operation` (required) | The tool name: `{target}_{module}_{object}_{verb}`. The module part is left out when the target declares no `modules`. The verb in the name is a plain word for callers; it need not be one of the built-in verbs a binding uses |
| `effect` (required) | `read`, `write`, or `irreversible` |
| `identity` | How the record is found again: `natural`, `composite`, `stamped`, `find`, or `none`, with the inputs, the stamped field, or the lookup it uses ([spec section 209](../spec/2-kernel/209-record-identity-and-dependent-records.md)); required for `write` |
| `key` | Short form of `identity`. One input is `natural`. A list of inputs is `composite`, parent first, such as `[order_id, item_code]`. The record key the ledger stores and locks is these inputs' normalized values, in the order declared |
| `commit_points` | How many times the application saves; default 1; checked by the compiler |
| `inputs` (required) | Map of name to `type`, `description`, `required`, `default` |
| `outputs` (required) | A schema, usually a reference to an endpoint's response |
| `bindings` (required) | One or more bindings (below) |
| `description` (required) | What it does, when to prefer something else, side effects, limits |
| `examples` | One to three example inputs |
| `permissions` | Uploads, downloads, and destinations beyond the surfaces its bindings use |
| `decision_points` | Named questions a person may have to answer during a run |

**Binding.** Common fields: `mode` (required), `status` (`draft`, `shadow`, `verified`, `retired`), `read_back` (required for writes), `precondition`.

| Mode | Its fields |
| --- | --- |
| `ui`, `desktop` | `screen`, `verb`, `fields` (map of screen field to expression), `action` |
| `page_http`, `http` | `endpoint`, `path_params`, `query`, `body` (maps of name to expression) |
| `api` | `operation_id` from the imported description, and `args` |
| `mcp`, `site_tools` | `tool` and `args` |

A lookup used by `identity: find` may declare a `settle` time for applications whose searches lag behind their writes. `read_back` names an endpoint, a screen field, or an upstream tool, and may say `prefer: response` to use the save response when it carries the stored values.

**Placeholders.** A placeholder in an endpoint's `path` or a screen's `reach` address takes the operation input of the same name. `path_params` is needed only to rename or compute one. A placeholder that matches no input and has no entry does not compile.

**`read_back` is also the record's lookup.** For `natural` and `composite` identity it is the only declared way to read the record, so the preview, the freshness check, and reconcile all use it as well as the check after a write ([spec section 208](../spec/2-kernel/208-runs-the-ledger-and-resume.md)). It must therefore work on its own before any write, reached from the identity inputs alone, and it must report a record that does not exist as absent, not as a failure. `prefer: response` applies only straight after a save; the other three reads always go to the application.

## `pipelines/*.yaml`

| Field | Meaning |
| --- | --- |
| `pipeline` (required) | The tool name, for its outcome |
| `effect` | Derived by the compiler; may be declared higher |
| `inputs` | As for operations |
| `source` | `template`: the name and version of a file under `sources/`, which names the reader and the mapping. A pipeline with a source has a reserved input named `source`, of type `file`, that carries the file to read; a source whose reader is `operation` has none |
| `limits` (required for writes) | `max_records`, and optional time and rate limits |
| `steps` (required) | Ordered list of steps (below) |
| `success` (required) | Conditions for `completed`, such as `all_records_verified`. A record found `unchanged` counts as verified |
| `headroom` | Minimum session time left before starting |

**Step.**

| Field | Meaning |
| --- | --- |
| `id`, `operation` (required) | A name and the operation to call |
| `with` | Map of the operation's inputs to expressions. Inputs are passed only here; nothing is passed by matching names |
| `for_each` | An expression giving the rows to repeat over, in order |
| `checkpoint` | `per_record` or `per_step`: where a snapshot is taken as evidence ([spec section 217](../spec/2-kernel/217-traces-evidence-and-repair.md)). It does not decide where a write can resume; the ledger makes every commit point a resume point whatever this says |
| `when` | An expression; the step is skipped when false |
| `expect` | An expression that must be true of the result, or the run stops |

A step can only stop on failure. There is no continue-on-error.

## `sources/*.yaml`

| Field | Meaning |
| --- | --- |
| `template` (required) | Name and version |
| `reader` (required) | `xlsx`, `csv`, `json`, or `operation` |
| `fingerprint` (required) | What identifies this template version |
| `locate` | Named ranges or header text that find the data |
| `mapping` (required) | Map of row field name to a source column or an expression. Each row the source yields has these fields; a pipeline step passes them to an operation's inputs through `with`, so a source never names an operation |
| `rows` | An expression filtering which rows are records |
| `missing` | Which text values, if any, count as empty |

## `tests/*.yaml` and `fixtures/`

| Field | Meaning |
| --- | --- |
| `test` (required) | The operation or pipeline under test |
| `kind` (required) | `offline` against a fixture, or `live` against a test record |
| `fixture` | The fixture folder, for offline tests |
| `inputs` (required) | Inputs to call with |
| `expect` (required) | The expected result, or the expected trace signature |
| `restore` | For live write tests: true to put the original values back afterwards |

A fixture folder holds recorded snapshots and exchanges and a marker file stating that it was scrubbed.

## Expressions

Expressions are written in CEL's syntax, in the part of it that `a2m-expr` reads ([decision 0072](../decisions/0072-expressions-are-a-cel-subset-on-the-engines-own-parser-and-evaluator.md)). They always terminate and never convert types silently. There is no float: a number with a decimal point does not compile. A decimal is written `decimal("12.50")`; `+`, `-`, `*`, and the comparisons work on two decimals, and division is `div(a, b, places, rounding)` ([decision 0073](../decisions/0073-decimals-use-operators-division-is-a-function-and-how-a-field-type-normalizes.md)).

| Available | Where |
| --- | --- |
| `input` | Everywhere in an operation or pipeline |
| `result` | In `expect`, `success`, and later steps |
| `row`, `source` | Inside `for_each` and mappings |
| `steps` | In a pipeline: `steps.<id>.result` is an earlier step's result |
| `env` | The environment's name and kind |

Host functions, the starting library: `to_inches`, `to_feet_inches`, `to_unit`, `parse_decimal`, `round_to`, `parse_date`, `format_date`, `trim`, `lower`, `upper`, `code_for`, `label_for`, `coalesce`. The library as built adds `decimal`, `div`, `parse_int`, `parse_money`, `parse_bool`, `snap_to_step`, `parse_datetime`, `format_datetime`, `parse_time`, `excel_serial_to_date`, `nfc`, `collapse_space`, `size`, and one `as_` function for each field-type base, which is that base's normalizer. An argument that is a rule, such as places, a rounding mode, a unit, a format, or a zone, is written as a literal. Each is tested, documented by `anything2mcp docs`, and versioned with the engine. Anything not expressible this way is a reviewed plug-in, not a script.

## Open points

Found by walking the sheet-entry pipeline through these formats on 2026-11-16. Each is settled in the slice named, not before.

- **A record that does not exist yet, in a `ui` binding.** The formats cannot yet say how a screen shows that a record is absent, or how an `upsert` reaches the create form when the update form is a different one. Hypothesis H-23; slice 3.8.
- **The kind of record.** A record key is built from identity inputs only, so two operations on the same record share a lock only if they declare the same inputs in the same order. Hypothesis H-24; slice 3.8.
- **The fields of `identity`.** Only the strategy names and the `key` short form are fixed. The long form for `stamped` and `find` is written with the first operation that needs one; slice 3.8.
- **The type of a bare column in a mapping.** The compiler type-checks `with` against the operation's inputs, so every row field needs a type before any file is read; slice 3.9.
- **Whether `checkpoint` is still needed.** Since the ledger keeps one row per commit point, it only places snapshots. A candidate for removal in a review round with the lens "less".
