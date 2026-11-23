# Target file formats

Every field of every target file. [Spec section 205](../spec/2-kernel/205-target-files.md) shows worked examples; this is the field-by-field reference the schemas in the code were written from: the kernel's file kinds in `a2m-files` (package B4), and each pack's own kinds in its crate. Where this page and the schemas in the code differ, the code is what loads, and one of the two is to be corrected.

This is a design draft. Field names can still change while the schemas are written; after the first release, a change is a new schema version with a migration.

**Who owns each kind of file.** The kernel owns `target.yaml`, `auth.yaml`, `types.yaml`, `operations/`, `pipelines/`, `sources/`, `tests/`, and `fixtures/`. Every other kind is brought by a pack, which registers its schema: `screens/` by the web pack and, for desktop surfaces, the desktop pack; `endpoints/` by the HTTP pack; `mappings/` by the MCP pack. The other packs bring their own, each given field by field in its spec section and listed [below](#kinds-the-other-packs-bring): `commands/` (command pack), `statements/` (data pack), `documents/`, `folders/`, and `logs/` (file pack), `channels/` and `schemas/` (stream pack), and `registers/` (device pack). A pack may read a kind it does not own; the compiler hands it the validated entries as data. An entry of a kind nobody registered does not compile.

**The holding folder.** `held/` at the top of a target's folder holds drafts that wait on an open question. The drafter writes there each draft that holds one, and each file it made from the same finding that names, as a whole value, something a held file defines (an operation that names a held endpoint, and so on to a fixed point); a file of the finding that names nothing held stays in place. Each is written at the path it would have had outside (`held/operations/order_get.yaml`, `held/endpoints/get_order.yaml`). The loader does not read it, so nothing in it is compiled, digested, or part of a plan; a folder named `held` anywhere deeper is an ordinary folder, and no pack may register a file kind in `held`. `draft.lint` lists what is held with each question. A person answers the question and moves the file to its place ([decision 0298](../decisions/0298-a-draft-with-an-open-question-waits-in-a-holding-folder.md)).

Conventions: names are lowercase with underscores. *Required* fields are marked. A field that takes an *expression* takes CEL ([expressions](#expressions)). A field that takes a *condition* takes one of the [conditions](#conditions).

## `target.yaml`

| Field | Meaning |
| --- | --- |
| `schema_version` (required) | Version of these formats the files are written in |
| `target` (required) | The target's name; the prefix of every tool name |
| `title` | Name shown to people |
| `surfaces` (required) | Map of surface name to a surface (below) |
| `environments` | Map of environment name to an environment (below); at least `production` is implied |
| `default_environment` | Used when a call names none; `production` unless given |
| `environment` | Fixed run settings that hold for every kind of surface: `locale`, and `timezone`, which is `UTC` or an offset such as `+02:00`. What only one kind of surface has, such as a viewport, is a field of that surface, which its pack registers |
| `modules` | Module names, for large applications |
| `defaults` | What applies unless overridden. `timeouts` is a map from a name to milliseconds, which a pack's lowering gives to a step that states none. `limits` holds `max_records` and `max_duration_ms`, which a pipeline that states none takes. `capture` is the capture level of a run: one of the levels of [spec section 217](../spec/2-kernel/217-traces-evidence-and-repair.md). The compiler applies the timeouts and the limits, so the plan holds the result and the engine applies no default; the capture level travels in the plan |
| `exclusive_session` | True when the application allows one session per account |
| `drift_risk` | `low`, `medium`, or `high`; sets how often canaries run |
| `authorization` | Reference to the record of who agreed to this target being wrapped; required to publish on a server |

**Surface.**

| Field | Meaning |
| --- | --- |
| `kind` (required) | A surface kind that an enabled pack registers. The first four packs register `web`, `http`, `mcp`, and `desktop`; the designed packs add `program`, `database`, `file`, `broker`, and `device_link`. The fields below the kind are the registering pack's. A field the pack marks as naming a program takes an absolute path, and the plan declares that program for the surface; a relative path is `kernel.program_invalid` |
| `origin`, `base`, `address`, `app` | The address for the kind: a web origin (`web`), an API base address (`http`), an MCP server address (`mcp`; a local server also names its `program` and `digest`), or a desktop application (`desktop`: `program`, its absolute path, which is its address, with `application` and optionally `digest`, as given [below](#kinds-the-other-packs-bring)). Other kinds have their own, given in their pack's section and listed [below](#kinds-the-other-packs-bring) |
| `stands` | For a surface that may sit under an application, such as a database or a file: `alone`, or `beneath` naming the surface of the application that owns it. With neither, no write binding on the surface compiles |
| `profile` | A platform profile name, or `auto` to detect |
| `same_session_as` | Another surface whose sign-in this one shares |
| `rate` | Requests allowed per period: `requests` and `per_ms`, the period in milliseconds |

**Environment.**

| Field | Meaning |
| --- | --- |
| `kind` (required) | `test` or `production` |
| `surfaces` | Address overrides per surface |
| `test_records` | Record keys that drafts and publish gates may write to. Each is one plain value, or a list of them for a key of several parts, parent first, written as the system shows it; text goes in quotes |
| `approve_writes` | The approval rule of this environment: one of the values of `policy.approve_writes` in [spec section 226](../spec/2-kernel/226-configuration.md). Policy applies whichever of this and the installation's own rule is the stricter, so a target can tighten the rule and never loosen it |

## `auth.yaml`

The file is a map with one key, `auth`: a list with one entry per group of surfaces that share a sign-in.

| Field | Meaning |
| --- | --- |
| `surfaces` (required) | The surfaces this entry covers |
| `mode` (required) | `use_existing`, `managed_session`, or `custom` |
| `provider` (required) | An auth provider from the technical reference |
| `probe` (required) | A cheap signed-in read: a `read` operation of the target, or an entry of a pack's file kind on one of the entry's surfaces, such as an endpoint. Any other name does not compile, except in a draft: there a probe that names nothing, where no operation file has that name, compiles as not yet drafted, with the warning `kernel.probe_pending`, and the sign-in is marked as not confirmed ([decision 0252](../decisions/0252-a-fixed-place-a-review-that-shows-what-runs-and-a-sign-in-before-its-probe.md)) |
| `probe_inputs` | For a probe that is an operation: the inputs it is called with. Every required input with no default is given here, so the probe can be run as written |
| `expiry_signals` | List of signals that the session has ended: `redirect_host`, `status` with `path`, or `visible` with a locator |
| `timeouts` | Declared `idle_ms` and `absolute_ms` durations, in milliseconds, when known |
| `sign_out` | An endpoint or action that ends the session |
| `settings` | The provider's own settings, as a map of names to plain values, such as `token_endpoint` or `header`. Each provider's names are in the [crate design](../architecture/crates/a2m-auth.md); a name the provider does not know is refused by the provider's own check when the version is published, because the compiler holds no list of providers. Never a credential |
| `credentials` | For `custom`: a map from the role the provider names, such as `password` or `client_secret`, to a vault reference such as `vault://example/service/password`, whose first part is the target's own name: a reference of another target is refused ([decision 0248](../decisions/0248-a-sign-in-goes-only-where-a-published-version-says-and-a-credential-belongs-to-its-target.md)). References only |
| `attempt_limit` | Automatic sign-in attempts with stored credentials; default 1 |
| `movable` | `auto` or `false`; `false` pins the session to its runner |

## `types.yaml`

Field types say how a value is formatted, normalized, compared, and classified. The file is a map with one key, `types`, from a field type's name to its fields. A key that means nothing for the type's base is refused.

| Field | Meaning |
| --- | --- |
| `base` (required) | `text`, `integer`, `decimal`, `money`, `measurement`, `date`, `datetime`, `time`, `boolean`, or `choice` |
| `unit`, `display_unit` | For measurements: the unit stored and the unit shown |
| `step` | The smallest increment, such as 1/16 inch |
| `places`, `rounding` | Decimal places and rounding mode |
| `currency` | For money |
| `format` | For a date or a time: the pattern the application displays and accepts, such as `MM/DD/YYYY` |
| `timezone` | For dates and times: `fixed`, `user`, or a named zone. Only `UTC` and a fixed offset such as `+02:00` are built so far; a named zone or the `user` rule cannot be normalized yet |
| `case`, `whitespace` | Text comparison rules. `case` is `sensitive` or `insensitive`; `whitespace` is `collapse`, `trim`, or `exact` |
| `decimal_mark` | For numbers: `.` or `,`, the decimal mark the application shows; `.` unless given |
| `true_text`, `false_text` | For booleans: the text the application shows for each; `true` and `false` unless given |
| `codes` | For choices: map of stored code to label |
| `sensitivity` | `public`, `internal`, or `restricted` |
| `untrusted` | True for free text that came from the application |

Built-in types: `text`, `integer`, `decimal`, `money`, `date`, `datetime`, `time`, `boolean`, `choice`, `length_ft_in`, `length_metric`, `percent`, `email`, `phone`, `identifier`, `url`, `file`, and `untrusted_text`, which is `text` marked untrusted, the type a drafted output of the system's text takes ([decision 0179](../decisions/0179-what-each-pack-drafts-so-that-a-scout-ends-with-reads-that-work.md)). A `url` input accepts only destinations the operation declares. A `file` input is a blob reference on a server and a path inside an allowed root on one machine ([spec section 502](../spec/5-security/502-defending-against-a-manipulated-agent.md)). A target adds its own by extending one of them, under a name of its own: a target's type named as a built-in is refused (`kernel.built_in_type_redefined`), but for `money` and `choice`, which a target completes ([decision 0182](../decisions/0182-the-file-data-and-stream-packs-after-the-first-drafted-reads.md)).

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
| `rows` | For a table: its `locate` and `fingerprint`, its `header`, the text of every heading in order, and its `columns`, each a name with the `header` it is under and its `type`. An output that is a list takes the column of its name. Finding one row by its key is not built |
| `variants` | Named states that can appear, such as a confirmation dialog, each with a condition `when` and a handling: `confirm`, `dismiss`, `stop`, or `absent`, which says the record the screen shows does not exist. As built: `when` is `{visible: locator}`, and the handling is `stop` or `absent` |

**Field.**

| Field | Meaning |
| --- | --- |
| `type` (required) | A field type |
| `control` (required) | A widget driver from the technical reference |
| `locate` (required) | Ordered list of [locators](#locators) |
| `fingerprint` (required) | What must also be true of the matched element: `section`, a locator of the one element it is inside, and optionally `near_text`, a text that section shows ([decision 0113](../decisions/0113-a-screens-fingerprint-is-a-section-absent-is-a-variant-and-a-table-is-read-by-its-headings.md)) |
| `read` | `value`, `text`, or an attribute name; default by control |
| `commit` | What commits the value in the page: `blur`, `enter`, or `none` |

**Action.**

| Field | Meaning |
| --- | --- |
| `locate` (required) | Ordered list of locators |
| `fingerprint` (required) | As for a field |
| `confirm` (required) | The condition that shows the action took effect |
| `commits` | True when the action saves in the application; counted as a commit point. This is what the compiler counts ([spec section 206](../spec/2-kernel/206-the-compiler-and-the-plan.md)); an action without it is not a save |

### Locators

Tried in order; the first that matches exactly one element and passes the fingerprint wins.

| Locator | Matches by |
| --- | --- |
| `{role, name}` | Accessibility role and accessible name. Both are written; `name: ""` matches an element with no accessible name |
| `{label}` | The text of the label associated with the control |
| `{placeholder}` | Placeholder text |
| `{text}` | Visible text, exact unless `contains: true` |
| `{test_id}` | A test identifier attribute, when the application has stable ones |
| `{title}` | Title or tooltip text |
| `{css}` | A selector; allowed only last in the list, and flagged by the linter |
| `within` | Added to any locator to limit it to a named region of the screen |

Position alone, and XPath, are not accepted.

A desktop screen uses `{role, name}` alone, both exact, with a role from the desktop pack's closed list of role words; its `control` is `desktop_text` ([decision 0202](../decisions/0202-the-desktop-packs-control-has-a-name-of-its-own.md)). An element with no accessible name cannot be located.

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

Brought by the HTTP pack. The web pack's `page_http` mode reads entries of the same form from a folder of its own, `page_requests/`: `endpoint`, `surface`, `method`, `path`, `effect`, `responses` (`ok`, `absent`, and for a write `refused`), and `response_schema`, with one more field, `page`, the path of the page that makes the request ([decision 0190](../decisions/0190-the-page-http-mode-reads-from-inside-the-page-in-an-observing-tab.md)). A read is `method: GET`, `effect: read`. A write is `method: PUT`, `effect: write`, with `request_schema`, `request_encoding` (`json` or `form`), and `anti_forgery`: `from` one of `meta`, `input` (a hidden input), or `cookie`, each with the name the page uses, then one `header` or one `field` it is sent in, and `refused`, the answers by which the application refuses the token; or the word `none` for an endpoint that takes no token. `POST`, `PATCH`, and `DELETE` do not compile yet, and no drafting rule writes a write entry ([decision 0206](../decisions/0206-the-page-http-mode-writes-by-put-with-the-pages-own-token-in-an-acting-tab.md)).

| Field | Meaning |
| --- | --- |
| `endpoint` (required) | Its name |
| `surface` (required) | An `http` surface, or a `web` surface for requests made from inside the page |
| `method`, `path` (required) | The method and a path template |
| `query`, `headers` | Templates; no secrets |
| `auth` | `session`, `token`, or `none`; supplied by the session keeper |
| `anti_forgery` | Where the token comes from (`cookie`, `meta`, or another endpoint) and the header it goes in |
| `request_schema`, `response_schema` (response required for a JSON answer) | The shape of each body, written in the entry in a closed subset of JSON Schema: `type`, `nullable`, `properties`, `required`, `additionalProperties`, `items`, `enum`. Any other word is refused |
| `answer` | The form of the answer, below. Left out, the answer is JSON, checked against `response_schema` |
| `responses` | The answers the endpoint declares. `ok`: accepted statuses, 200 unless given. `absent`: statuses that mean the record does not exist, on a lookup. `refused`, `conflict`, `unavailable`: lists of a `status` with an optional `when`, a map from a top-level field of the body to the text it must hold; a `conflict` may name the field that holds the record's version now, as `current`. An answer that matches nothing declared stops the step |
| `signed` | True when the front end computes or signs the request; an `http` binding on it does not compile |
| `max_response_bytes` | The most bytes of an answer that are read |
| `effect` (required) | `read` or `write` |
| `read_asserted` | `true` on an endpoint marked `read` whose method HTTP does not mark safe, such as a search behind `POST`: the developer's word that it changes nothing. Without it such an endpoint does not compile; with it, it is sent outside a live run only from a published version, against an environment of kind `test` ([decision 0249](../decisions/0249-the-http-pack-reads-in-bounded-work-and-sends-nothing-that-may-write-outside-a-live-run.md), [decision 0270](../decisions/0270-the-http-pack-sends-an-asserted-read-only-from-a-published-version-and-bounds-what-it-leaves-running.md)) |
| `idempotency_header` | Header that carries the record's idempotency token, if the application accepts one |
| `version` | Where a record's version is read, `read: {field: ...}` or `read: {header: ...}`, and how it is sent back as a precondition, `send: {body: ...}` or `send: {header: ...}`. A binding on an endpoint that sends a version names a `read_back` endpoint that reads one |
| `volatile` | Request fields to ignore when comparing with a shadow |

**The answer's form** ([decision 0217](../decisions/0217-the-http-pack-reads-answers-that-are-not-json.md)). `answer.form` is `json`, `xml`, `text`, or `bytes`; `answer.content_type` lists the media types the answer's `Content-Type` must be one of, `application/*+xml` standing for a family. Unless listed: `xml` takes `application/xml`, `text/xml`, and `application/*+xml`; `text` takes `text/plain`; `bytes` takes `application/octet-stream`; `json` is not checked. An answer that is not JSON has no `response_schema`: the values it declares are its record, which a binding's `extract` names as it names a JSON body's properties.

| Form | Fields | The record |
| --- | --- | --- |
| `xml` | `namespaces`: prefix to URI, the only prefixes a path may use; `fields`: a map of fields by name, each `path` or `each`; `max_depth` (64 unless set) and `max_elements` (100,000) | A `path` is steps from the root element joined by `/`, each a name or `prefix:name`, the last may be `@name` for an attribute; otherwise the element's text, its two ends' white space removed. `type` is `string` (the default), `integer`, `number`, or `boolean`; `required: false` lets the path match nothing. `each` is a path to a repeated element and gives a list: with `fields`, of records whose paths start at that element; without, of the elements' text. A path that matches nothing where required, or more than one place, stops the read. No function, predicate, axis, wildcard, or nested `each` |
| `text` | `charset`: `utf-8` (the default), `us-ascii`, or `iso-8859-1`; `pattern`: a regular expression the whole text must match; `fields`: the `type` and `required` of its named groups | `text`, the whole body; and one field for each named group of the pattern |
| `bytes` | None | `length`, a whole number, and `digest`, the SHA-256 of the body as 64 hexadecimal characters |

```yaml
endpoint: order_xml_get
surface: api
method: GET
path: "/views/xml/orders/{order_id}"
effect: read
responses: {absent: [404]}
answer:
  form: xml
  namespaces: {o: "urn:example:orders"}
  fields:
    status: {path: "o:order/@status"}
    customer: {path: "o:order/o:customer"}
    items:
      each: "o:order/o:items/o:item"
      fields:
        item_code: {path: "@code"}
        quantity: {path: "o:quantity", type: integer}
```

## Kinds the other packs bring

Each pack registers these file kinds and surface kinds in its manifest. The fields are given in the pack's spec section and the design of its crate.

| Folder | File kind | Surface kind and its address keys | Pack |
| --- | --- | --- | --- |
| `endpoints/` | `endpoint`, [above](#endpointsyaml) | `http`: `base`, and `unencrypted_credentials`, which is `accepted` only on a surface that may send a credential over plain `http` to another machine; any other value, or none, refuses it ([decision 0188](../decisions/0188-credentials-over-plain-links-drafted-workbooks-and-published-device-maps.md)) | [Section 303](../spec/3-packs/303-the-http-pack.md); [`a2m-pack-http`](../architecture/crates/a2m-pack-http.md) |
| `commands/` | `command` | `program`: `path`, and `digest` | [Section 306](../spec/3-packs/306-the-command-pack.md); [`a2m-pack-command`](../architecture/crates/a2m-pack-command.md) |
| `statements/` | `statement` | `database`: `file`, `store`, and `root`. A read-back read is written `effect: read` with `form: table` and a `record`: it is the one read of a record by its key, and the pack writes its text ([decision 0141](../decisions/0141-the-data-pack-holds-a-statement-to-one-read-or-one-keyed-write-and-a-connection-to-one-file.md)). A field of a record may say `nullable: true` where its column takes a null; an output read from it may be absent, and discovery drafts the flag for every such column that is not part of the key ([decision 0315](../decisions/0315-every-declared-output-is-in-every-result-on-every-path.md)) | [Section 307](../spec/3-packs/307-the-data-pack.md); [`a2m-pack-data`](../architecture/crates/a2m-pack-data.md) |
| `documents/` | `document` | `file`: `folder`, and `root`. A document that a binding writes must state `may_be_open`, `true` or `false`; a field of delimited text whose value may begin with `=`, `+`, `-`, or `@` must say `may_begin_formula: true`, or such a value is refused ([decision 0149](../decisions/0149-the-file-pack-after-its-safety-audit.md)). Its formats include `ini` and `property_list`, which are read and updated one value at a time ([decision 0246](../decisions/0246-the-file-pack-reads-folders-settings-files-and-growing-files.md)) | [Section 308](../spec/3-packs/308-the-file-pack.md); [`a2m-pack-file`](../architecture/crates/a2m-pack-file.md) |
| `folders/` | `folder` | `file`, as for `documents/` | `depth` (0 to 4), `max_entries`, `digest_max_bytes`, `content_max_bytes`, `writes` (any of `create`, `update`, `rename`, `delete`), and `may_be_open`, which must be `false` where anything is written. [Decision 0246](../decisions/0246-the-file-pack-reads-folders-settings-files-and-growing-files.md); [section 308](../spec/3-packs/308-the-file-pack.md); [`a2m-pack-file`](../architecture/crates/a2m-pack-file.md) |
| `logs/` | `log` | `file`, as for `documents/` | `file`, `encoding` (`utf-8`), `max_lines`, and `max_bytes`. Read only. [Decision 0246](../decisions/0246-the-file-pack-reads-folders-settings-files-and-growing-files.md); [section 308](../spec/3-packs/308-the-file-pack.md); [`a2m-pack-file`](../architecture/crates/a2m-pack-file.md) |
| `channels/`, `schemas/` | `channel`, `message_schema` | `broker`: `address`, and `protocol` | [Section 309](../spec/3-packs/309-the-stream-pack.md); [`a2m-pack-stream`](../architecture/crates/a2m-pack-stream.md) |
| `registers/` | `register` | `device_link`: `address`, `protocol`, `unit`, and `serials`, the serial of the device in each environment, by the environment's name; a write is refused where none is declared for the run's environment; the layout of a published register map that the pack drafts a map from is given in the pack's design ([decision 0137](../decisions/0137-a-device-is-held-to-its-serial-a-write-frame-is-recorded-before-it-is-sent-and-a-run-has-a-frame-ceiling.md)) | [Section 310](../spec/3-packs/310-the-device-pack.md); [`a2m-pack-device`](../architecture/crates/a2m-pack-device.md) |
| `page_requests/` | `page_request`, [`endpoints/`](#endpointsyaml) in form, for reads and for writes by PUT, with one more field, `page` | `web`, as for `screens/` | [`a2m-pack-web-ui`](../architecture/crates/a2m-pack-web-ui.md); [decision 0190](../decisions/0190-the-page-http-mode-reads-from-inside-the-page-in-an-observing-tab.md) |
| `screens/` | `screen`, [above](#screensyaml), with `control: desktop_text` | `desktop`: `program`, the absolute path of the application's program, which is its address; `application`, the name it gives itself on the accessibility bus; and optionally `digest`, the SHA-256 of the program. Reads only, on Linux only ([decision 0198](../decisions/0198-the-desktop-pack-reads-linux-applications-through-a-look-only-back-end.md)) | [Section 305](../spec/3-packs/305-the-desktop-pack.md); [`a2m-pack-desktop`](../architecture/crates/a2m-pack-desktop.md) |
| `mappings/` | `mapping` | `mcp`: `address`; a local server also names its `program` and `digest`; `unencrypted_credentials: accepted` as on an `http` surface ([decision 0188](../decisions/0188-credentials-over-plain-links-drafted-workbooks-and-published-device-maps.md)) | [Section 304](../spec/3-packs/304-the-mcp-pack.md); [`a2m-pack-mcp`](../architecture/crates/a2m-pack-mcp.md) |

## `operations/*.yaml`

| Field | Meaning |
| --- | --- |
| `operation` (required) | The tool name: `{target}_{module}_{object}_{verb}`. The module part is left out when the target declares no `modules`. The verb in the name is a plain word for callers; it need not be one of the built-in verbs a binding uses |
| `effect` (required) | `read`, `write`, or `irreversible` |
| `identity` | How the record is found again: `natural`, `composite`, `stamped`, `find`, or `none`, with the inputs, the stamped field, or the lookup it uses ([spec section 209](../spec/2-kernel/209-record-identity-and-dependent-records.md)); required for `write`. With `natural` and no `inputs`, `place: fixed` says the record is the one place every binding names in the target's files, which its file kind declares fixed; no input is its key ([decision 0252](../decisions/0252-a-fixed-place-a-review-that-shows-what-runs-and-a-sign-in-before-its-probe.md)) |
| `key` | Short form of `identity`. One input is `natural`. A list of inputs is `composite`, parent first, such as `[order_id, item_code]`. The record key the ledger stores and locks is these inputs' normalized values, in the order declared |
| `commit_points` | How many times the application saves; default 1 for a write, and at least 1; a read states none; checked by the compiler |
| `inputs` (required) | Map of name to `type`, `description`, `required`, `default` |
| `outputs` (required) | Map of name to `type`, a field type, `list`, true for a list of that type, and `may_be_absent`, true where the system may hold no value for the output ([decision 0305](../decisions/0305-an-output-that-may-be-absent-is-declared-so-in-the-plan-and-the-output-schema.md)). An output that may be absent is returned as `null` under its own name, and the tool's output schema admits `null` for it and requires every output. A pack that can read it from what the system declares sets it itself (the HTTP pack: an output whose path is not certain in the endpoint's `response_schema`), and refuses `may_be_absent` on a value its shape makes certain, as `http.absent_untrue`; a value of the type `any` is not certain. The data pack sets it for a field declared `nullable`, and the file pack for a folder entry's `size`, `modified`, and `digest`. The stream pack sets it on every output but a body field the channel's schema requires (on a read, a list, or a take) and a list's `count`, `more`, and `coverage`; the MCP pack on every output but the mapping's `key` and the `text` of a tool whose result is text. A list output that may be absent admits `null` for each item. The engine holds every result to this, whatever pack gave it: a `null`, or no value, in an output that may not be absent stops the run with `output_unexpected`, and one that may be absent and was not observed is returned as `null`, on a write's path as on a read's ([decision 0315](../decisions/0315-every-declared-output-is-in-every-result-on-every-path.md)). A reference to a pack's schema is not built yet |
| `title` | Name shown to people; the operation's name unless given |
| `hazard` | What the operation could damage beyond its record ([spec section 213](../spec/2-kernel/213-effects-hazards-and-consent.md)); `none` unless given. Any other hazard requires the effect `irreversible` |
| `bindings` (required) | One or more bindings (below) |
| `description` (required) | What it does, when to prefer something else, side effects, limits. Plain text of at most 1,024 characters, with no control character (a tab or a line end is allowed); the compiler refuses more as `kernel.description_unbounded` ([decision 0307](../decisions/0307-what-the-evidence-level-is-for-and-what-a-review-shows-about-a-tools-evidence.md), point 6) |
| `binding_order` | `listed` keeps the bindings in the order of the file. Without it the compiler orders them by each mode's `rank` in its provider's manifest, lower first, and by place in the file among equal ranks ([decision 0399](../decisions/0399-a-description-says-who-wrote-it-a-drafter-reports-what-it-left-undrafted-and-bindings-are-ordered-by-rank.md)). A test's `binding` `position` counts places in this file whatever order the compiler gives the plan |
| `description_by` | Who wrote the description: `rule` or `builder`; a builder's unless given. A drafter writes the first, right after the description; a person or an agent who rewrites a drafted description removes the line or writes the second. The compiler puts it in the plan, and the MCP door sends it with the tool |
| `examples` | One to three example inputs, each a map from an input's name to a value of its type |
| `permissions` | Uploads, downloads, and destinations beyond the surfaces its bindings use. A destination is one exact address: a host name or an address, with a port or a scheme where needed, and no wildcard, range, path, or user information. For a web tool it is an origin its pages may load from or send to, and needs its scheme, such as `https://assets.example.com`; one without a scheme gives the tool's tab nothing ([decision 0292](../decisions/0292-a-web-tab-sends-only-to-its-surface-and-the-destinations-its-tool-declares.md)). An upload or a download is one name |
| `decision_points` | Questions a person may have to answer during a run: a list, each with a `name`, unique within the operation, and the `question` as the person is asked it. A run asks only what its operation declares |

**Binding.** Common fields: `mode` (required), `status` (`draft`, `shadow`, `verified`, `retired`), `read_back` (required for writes), `precondition`.

| Mode | Its fields |
| --- | --- |
| `ui`, `desktop` | `screen`, `verb`, `fields` (map of screen field to expression), `action` |
| `page_http`, `http` | `endpoint`, `path_params`, `query`, `body` (maps of name to expression). For `http`, also `extract`: a map from an output's name to where its value is in the answer, as property names joined by dots, with one `*` for a list; an output with no entry is the answer's field of its own name |
| `api` | `operation_id` from the imported description, and `args` |
| `mcp`, `site_tools` | `tool` and `args` |

A lookup used by `identity: find` may declare a `settle` time for applications whose searches lag behind their writes. `read_back` names an endpoint, a screen field, or an upstream tool, and may say `prefer: response` to use the save response when it carries the stored values.

**Placeholders.** A placeholder in an endpoint's `path` or a screen's `reach` address takes the operation input of the same name. `path_params` is needed only to rename or compute one. In an endpoint's `path` a placeholder is one whole segment, such as `/orders/{order_id}`. In a screen's `reach` it is one whole segment too, and its value must be one plain segment, of letters, digits, `-`, `.`, `_`, and `~`, not dots only: any other value is refused before the browser is sent anywhere ([decision 0187](../decisions/0187-the-web-pack-matches-a-screen-by-its-address-and-reads-by-key.md)). A placeholder that matches no input and has no entry does not compile.

**`read_back` is also the record's lookup.** For `natural` and `composite` identity it is the only declared way to read the record, so the preview, the freshness check, and reconcile all use it as well as the check after a write ([spec section 208](../spec/2-kernel/208-runs-the-ledger-and-resume.md)). It must therefore work on its own before any write, reached from the identity inputs alone, and it must report a record that does not exist as absent, not as a failure. `prefer: response` applies only straight after a save; the other three reads always go to the application.

## `pipelines/*.yaml`

| Field | Meaning |
| --- | --- |
| `pipeline` (required) | The tool name, for its outcome |
| `description` (required) | As for operations |
| `description_by` | As for operations |
| `title` | Name shown to people |
| `effect` | Derived by the compiler; may be declared higher |
| `inputs` | As for operations |
| `source` | `template`: the name and version of a file under `sources/`, which names the reader and the mapping. A pipeline with a source has a reserved input named `source`, of type `file`, that carries the file to read; a source whose reader is `operation` has none |
| `limits` (required for writes) | `max_records`, and optionally `max_duration_ms` |
| `steps` (required) | Ordered list of steps (below) |
| `success` (required) | Conditions for `completed`: a list of expressions, each of which may read `all_records_verified`. A record found `unchanged` counts as verified |
| `headroom_ms` | Minimum session time left before starting, in milliseconds |

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
| `template` (required) | The template's name |
| `reader` (required) | `xlsx`, `csv`, `json`, a plug-in's reader, or `operation` for rows that are another operation's result |
| `versions` (required unless the reader is `operation`) | List of the template versions the definition knows, each with `version` and `fingerprint`. A fingerprint has `parts`, each with `kind`, `place`, and `value`, and may state its `digest`; the compiler works the digest out from the parts and refuses one that differs |
| `locate` | Where the data lies: `document` (the default), `{named: ...}` for a table, a defined name, or a key, or `{sheet: ...}` |
| `columns` (required) | The header text of every column the definition reads, each once. A column in the data that is not listed is a problem |
| `fields` (required) | Map of row field name to its `type`, a field type, and either `column`, the header it is read from, or `expression`; and `required`, true unless given. Each row the source yields has these fields, each of its field type; a pipeline step passes them to an operation's inputs through `with`, so a source never names an operation |
| `key` | The row fields that together identify a row; two rows with one key are a problem |
| `rows` | An expression filtering which rows are records |
| `missing` | Which text values, if any, count as empty |
| `hidden` | `reject` (the default) or `include`: what is done with a hidden row or column |
| `options` | The reader's dialect: `delimiter`, one character, a comma unless given |

An `expression` and `rows` read `row`, the record of the row's cells by header text, and `source`. A cell has no type until a file is read, so each must end in a type of its own, as `upper(row.Code)` and `row.Code != ""` do. The compiled form is the `SourceSpec` in `a2m-files`, which `a2m-sources` loads ([decision 0087](../decisions/0087-the-source-readers-interface-as-building-it-settled-it.md)).

## `tests/*.yaml` and `fixtures/`

| Field | Meaning |
| --- | --- |
| `test` (required) | The operation or pipeline under test |
| `name` | The test's name; the file's name unless given |
| `kind` (required) | `offline` against a fixture, or `live` against a test record |
| `fixture` | The name of the fixture's folder under `fixtures/`; required for offline tests |
| `inputs` (required) | Inputs to call with |
| `expect` (required) | The expected result, or the expected trace signature |
| `restore` | For live write tests: true to put the original values back afterwards |
| `binding` | For a test of an operation: the binding it tests, `{mode: <mode>, position: <n>}`, the position counted from 1 among the operation's bindings and needed only where two share the mode. Without it, the test runs the binding that would act live: the first `verified`, else the first not retired. The result names the binding that ran and the digest of what it does; the publish gate asks a passing test of every binding that may act ([decision 0200](../decisions/0200-evidence-names-its-binding-and-a-recorded-fixture-says-nothing-of-the-original.md)) |

A fixture folder holds a marker file stating that it was scrubbed, `fixture.yaml` with `scrubbed: true`, and the recorded answers an offline test is replayed from ([decisions 0177](../decisions/0177-a-targets-offline-tests-run-against-a-replay-of-their-fixtures.md) and [0180](../decisions/0180-a-targets-offline-tests-cover-writes-and-gate-the-publish.md)):

| File | What it holds |
| --- | --- |
| `<name>.json` | The answer of each step named `<name>`: the entry of the pack's file the step refers to (a call, an endpoint, a statement, a document, an upstream tool, or the last part of an entry such as `surface/tool`), or, for a step that refers to none, its kind. An object of the values the step observed, or `null` for a record that is absent, which answers every step of that name; or a list of those, given in order to the steps of that name, each once. An exact number is written as text |
| `<name>.request.json` | For a committing step: the values the engine must send it, by argument name, an object or a list in the order the saves are sent. Compared field by field with what was sent; a save sent with no recorded request fails the test |
| Any other file | A file a step or a source reads. A pipeline's source is named by the test's input `source`, a file of this folder, read through the checked paths |

An answer file holds what the step observed, not the system's raw answer: for an HTTP read, the named values its extract step takes from the answer, which are the operation's outputs and any value the plan also asks for, such as a version. A list is a JSON list of values. A record is an object with one entry named `record`; a bare object is refused as a value. So an operation with the outputs `customer` and `lines`, where `lines` is a list of records, has this answer file:

```json
{"customer": "Example Customer One", "lines": [{"record": {"item_code": "L-300", "quantity": 2}}]}
```

A step with no answer left fails the test as `fixture_no_answer`, and nothing is sent. In `expect`, a field the result does not hold may name the run itself: `state` (its end state, `completed` unless given), `records_verified` (records read back and matched, or found unchanged), `failure` (the code it stopped with), and `sent` (how many saves were sent). A test of a write runs in a ledger of its own, thrown away when it ends.

A write's fixture answers the look before the save and the read-back after it differently, so the step they share is answered by a list, in order: the record as it was (or `null`, absent), then as the save left it. One object would answer both, find the record unchanged, and send nothing. A write's test must have `<name>.request.json`, and passes only when what was sent equals it; without one it is `not_run`. A test that expects the record to hold the intended values already, and nothing to be sent, says `sent: 0`: it proves that path, and by itself does not pass the publish gate for the write ([decision 0196](../decisions/0196-the-engine-and-the-registry-after-the-acknowledgement-and-the-first-hour.md)).

`fixture.record` writes a read's fixture folder and its test from one real read in the sandbox, through the binding it is given by `mode` and `position` or else the one that would act live: each step's answer as above, every value redacted and then put in place of by a made-up one chosen without looking at the original, except for what the replay needs ([decision 0200](../decisions/0200-evidence-names-its-binding-and-a-recorded-fixture-says-nothing-of-the-original.md)): text by text of one fixed alphabet and one length for its kind of type, chosen by its place, and in a key field the same original by the same text, so the test's input and the fixture agree; a choice by its type's first declared code, whatever it was; whole numbers by `1`, exact numbers, and numbers shown as text, by `1` of the type's places; dates, moments, and lengths of time by fixed ones; `true` and `false` alternating by place; `null` kept; lists cut to three items and otherwise kept, each by the list's field type. A name in an answer is kept only where the operation declares it as an output, its declared record type lists it, or the target's files state it for what the steps refer to; every other key is put in place of by a made-up key. The test's `expect` is the replay's own result. A value whose type has no rule stops it, and nothing is written; each file is made new, and on any failure the files written are removed.

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
- **The type of a bare column in a mapping.** Settled by [decision 0087](../decisions/0087-the-source-readers-interface-as-building-it-settled-it.md): every row field names a field type, whether it reads a bare column or an expression, so every row field has a type before any file is read. The fields of `sources/*.yaml` that carry this, and the columns, the key, the hidden rule, and the delimiter that `a2m-sources` reads, are written into the table above with the compiler's slice.
- **Whether `checkpoint` is still needed.** Since the ledger keeps one row per commit point, it only places snapshots. A candidate for removal in a review round with the lens "less".
