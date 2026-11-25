# Names added to the architecture page

Names that did not come from the glossary, with their source and meaning. Written for adding to the glossary.

## Where the glossary and the code disagree

- Bridge: the glossary's term is an archetype (code that runs inside a host application). The page's "Bridge" is the small program an MCP client starts, which the code calls the stdio proxy (a2m-mcp, src/proxy.rs). Chosen: Stdio Proxy. The command `anything2mcp bridge` keeps its name.
- Control: the glossary's term is Control (everything the engine asks of the control plane). The owner's example gives Control Port. Chosen: Control Port.
- Choose: the glossary names the engine loop's second part "choose"; the owner's example turns "Choose a binding" into Binding Selector. Chosen: Binding Selector. The other loop parts keep glossary words where a part is shown on its own: Guard. Intend, act, confirm, record are shown as Intent Recording, Single Act, Verification, and Result Store.
- Registered name: the glossary's term for a word whose values arrive with packs and crates. The code and AGENTS.md call such a word a "checked name" (a2m-core, src/name.rs). Chosen: Checked Names.
- Held folder: the glossary's term is a folder held open while a pack acts on one entry (CheckedPath::hold_folder). The folder where drafts with an open question wait (`held/`) is a different thing. Chosen: Holding Folder, from the owner's examples.
- Reconcile: the glossary has the verb only. The page names the engine's resume part Reconciliation.
- Health state, Template version, Screen model, Read-back: the glossary has the word, and the page needs one name per pack or per place, so the names carry a qualifier (Health Check, Template Fingerprints, Web Screen Models and Desktop Screen Models, HTTP Read-Back and the other pack read-backs).

## Old names I was unsure how to rename

- Rest or finish -> Preview Outcome: the part ends a preview by completing a run with nothing to change, or by asking policy and resting for approval; no code name fits.
- Neither: stop -> Unresolved Outcome: the third reconcile outcome (values match neither); named after its two sibling outcomes.
- Who does what -> Division of Work: spec section 402 title; no code type.
- Standing -> Place Reading: code model.rs "standing"; prefixed to be unique from other uses of the word.
- Copy across -> Message Relay: the part copies messages unchanged between standard input and the local channel.
- Say whether it was sent -> Send Outcome: reports sent, not sent or unknown for each failure; a related glossary term is Application's verdict.
- What the store is -> Store Kind: whether a database is the system of record (alone) or beneath an application.
- Set and invoke -> Input Actions: sets values, invokes, selects, expands.
- The three clocks -> Session Clocks, and Watches and canaries -> Watches and Canaries (canaries are planned, not built).
- Where a sign-in may go -> Sign-In Guard: the check of every address a sign-in would send a credential to.
- Numbers of the nine ledger rules were dropped from their names (Intent Before Action and so on); the group Ledger Rule keeps them together.
- Tests and fixtures, held/, target.yaml, auth.yaml, types.yaml, operations/ and pipelines/ keep their file spellings where the page shows a file or folder; the others are Title Case.
- Holding Folder is the name of two parts (the scout's held-drafts part and the project's held/ folder) because they are one thing seen from two views.
- Group labels over a name were renamed to singular Title Case nouns (for example Thin Client, Front Door, Stored State, Shared Server Extension, Target System).

## Names

| Name | Kind (part, group, walk) | Where it appears (view id) | Source (code name and path, or "coined") | One-sentence meaning |
| --- | --- | --- | --- | --- |
| Absent Actions | part | registry | coined | The set of things no action does: approve a write or publish locally, read a secret back, or run as another person. |
| Absolute Stops | group | policy | coined | The group in the Policy Engine view that holds Kill Switches, Guardrails. |
| Access Rules | group | policy | coined | The group in the Policy Engine view that holds Roles and Grants, Permission Manifest. |
| Accessibility Backend | part | pack-desktop | code: a2m-pack-desktop: src/backend | The one implementation per operating system of the accessibility interface. |
| Act Page | part | pages | coined | The page that shows a person's act an agent asked for, pinned to a hash. |
| Acted Mark | part | engine-write | coined | The step of the write path that tells the ledger the write was sent, using up the token and keeping the outcome and any assigned key. |
| Action Definition | part | registry | code: a2m-actions: src/def.rs | The single definition of an action: name, schemas, lowest role, listing, handler, and health needs. |
| Action Groups | group | registry | coined | The group in the Action Registry view that holds Tool Use Actions, Target Build Actions, Provider Actions, Publish Actions, Admin Actions. |
| Action Lookup | part | reg-dispatch | coined | The dispatcher step that looks an action up by name. |
| Action Phase | group | http-driver | coined | The group in the HTTP Driver view that holds Anti-Forgery Token, Request Sender. |
| Action Routes | part | rest-face | coined | The one generated route per action. |
| Action Stage | group | engine-loop | coined | The group in the Engine Loop view that holds Read Execution. |
| Actionability and Input | part | web-browser | coined | The check that an element can take an action and the action itself, in one in-page call. |
| Address Readers | group | netstate | coined | The group in the Network State view that holds Network Interfaces, Network Routes, Name Servers. |
| Admin Actions | part | registry | coined | The group of actions for toolsets, grants, policy, sessions, runners, kill switches, audit, secrets, and keys. |
| Admission | part | scheduler | coined | The scheduler's fixed-order decision whether a run is accepted, returned as an existing run, or refused with a next step. |
| Admission Stage | group | engine-loop | coined | The group in the Engine Loop view that holds Guard, Binding Selector. |
| Agent Side | group | bridge | coined | The group in the Stdio Proxy view that holds Standard Streams. |
| AI Agent | part | system | coined | Any program that speaks MCP and calls the published tools; it can ask for a change but never approve one, publish a tool, or sign in. |
| Analysis Stage | group | scout | coined | The group in the Scout and Drafter view that holds Scenario Comparison, Drafter. |
| Answer Forms | part | pack-http | code: a2m-pack-http: src/forms.rs | The forms an answer may take, JSON by default. |
| Answer Shaping | part | reg-dispatch | coined | The dispatcher step that redacts and marks the answer and cuts it to the size cap. |
| Answer Stage | group | reg-dispatch | coined | The group in the Dispatcher view that holds Answer Shaping. |
| Answer Validation | part | http-driver | coined | The acceptance of only a declared status and a body that fits its shape. |
| Anti-Forgery Token | part | http-driver | coined | The fetching of an anti-forgery token the endpoint declares it needs. |
| Anything2MCP on One Machine | part | system | coined | One program on the person's own computer: the local engine, its stores on disk, and the thin clients and front doors around it. |
| Append-Only History | part | ledger-rules | coined | The ledger rule that events are never updated or deleted, except whole runs by the retention job. |
| Approval Bound to Diff | part | ledger-rules | coined | The ledger rule that a write run enters running only with an unexpired, unused approval whose diff hash equals the run's, or a policy decision that allowed it. |
| Approval Recorder | part | pages | coined | The part that records an approval only when the presence prompt succeeded. |
| Approval Page | part | pages | coined | The page that shows one stored change and its hash for approval. |
| Approval Sequence | group | pages | coined | The title of the numbered walk through the Person Pages view, told in 7 steps. |
| Argument Builder | part | pack-command | coined | The building of the argument list from the template, with each value checked against its field type. |
| Atomic Compilation | part | compiled-target | coined | The rule that a target compiles whole or not at all, with problems reported together. |
| Atomic Replace | part | pack-file | coined | The writing of a temporary file in the same folder and replacing the old file in one step. |
| Attended Path | group | policy | coined | The group in the Policy Engine view that holds Approvals. |
| Audit Chain | part | ledger | code: a2m-ledger: src/audit.rs | The ledger's chain of entries per workspace, each linked to the one before, so that removing or changing an entry shows. |
| auth.yaml | part | project | code: a2m-files: src/schema.rs | The file giving the sign-in method per surface, the cheap signed-in read, signs of expiry, and declared timeouts. |
| Baseline Comparison | part | watches | coined | The comparison of a check's digests with the baseline. |
| Binding Selector | part | engine-loop | coined | The part of the engine loop that picks the first binding that is verified, has a ready driver, has not drifted, and has a valid session. |
| Blocked Programs | part | pack-command | coined | The programs the command pack will not start, such as shells given inline code and launchers. |
| Bounded List | part | pack-stream | coined | The bounded listing of what a channel holds now. |
| Browser Crate | group | pack-web | coined | The group in the web view that holds Browser Engine. |
| Browser Engine | part | web-browser, pack-web | coined | The web pack's crate that starts the engine's own browser and opens, drives, and contains its tabs. |
| Browser Launch | part | web-browser | coined | The start of a browser through the supervisor with a data directory the engine owns. |
| Browser Profile | part | system | coined | The browser's data directory that the engine owns, in which the engine's own browser runs and which no other program's cookie store ever replaces. |
| Browser Sign-In | part | web-ui | coined | The web pack's browser sign-in, such as a form login with a stored credential tried once. |
| Browser Startup | group | web-browser | coined | The group in the Browser Engine view that holds Browser Launch, Tab Postures. |
| Built-In Auth Providers | part | session-keeper | coined | The ways of signing in that the session keeper ships with. |
| Call Checks | group | policy | coined | The group in the Policy Engine view that holds Kill Switches, Guardrails, Roles and Grants, Permission Manifest, Limits and Quotas. |
| Call Handling | group | registry | coined | The group in the Action Registry view that holds Dispatcher. |
| Call Mapping | part | mcp-face | coined | The mapping from a tool name to an action or to a call of a published tool. |
| Call Sequence | group | engine-loop | coined | The title of the numbered walk through the Engine Loop view, told in 8 steps. |
| Callers | group | system | coined | The group in the Anything2MCP on One Machine view that holds AI Agent, Developer, Calling Program, Person. |
| Calling Program | part | system | coined | Code that calls the engine through a generated client, for example to run a pipeline over a spreadsheet and follow it to its end. |
| Capture Levels | part | telemetry | coined | The levels that decide which captures are kept and in what form. |
| Capture Redaction | part | telemetry | coined | The running of captured data through the redactor, checked again on the way out. |
| Catalog Discoverer | part | pack-data | coined | The discoverer that reads the store's own catalog inside a read-only transaction. |
| Cell Typing | part | sources | coined | The turning of a cell into a typed value by the field type. |
| Changed Record | walk | engine-loop | coined | A worked case in the Engine Loop view, told in 5 steps, which begins: The write ran as a preview first. |
| Channel File | part | pack-stream | code: a2m-pack-stream: src/channel.rs | The channels file kind that fixes a channel's kind, key, schema, ownership, retention, and duplicate window. |
| Check Cycle | group | watches | coined | The group in the Watches and Canaries view that holds Due Times, Check, Baseline Comparison. |
| Checked Names | part | core | coined | The checked names, such as a mode or a failure code, whose values arrive with packs and crates. |
| Checked Paths | part | contract | code: a2m-provider: src/path.rs | The paths a pack may open, which can only be made by checking against its manifest's roots. |
| Checking Stages | group | compiled-target | coined | The group in the Compiled Target view that holds Type-Checking, Lowering, Compiled Target Analysis. |
| Clean Capture | group | telemetry | coined | The group in the Logs and Traces view that holds Capture Redaction, Recordable Headers. |
| Closed Vocabularies | part | core | coined | The closed vocabularies, each an enum whose names are the documents' names. |
| Command Declaration | group | pack-command | coined | The group in the command view that holds Commands File, Program Check. |
| Command Discovery | group | pack-command | coined | The group in the command view that holds Help Discoverer, Command Drafting Rules. |
| Command Drafting Rules | part | pack-command | coined | The rules by which the command pack writes a commands draft from findings. |
| Command Line | part | command-line, system | coined | The thin client whose commands are each one action of the registry, sent to the running engine. |
| Command Proof | group | pack-command | coined | The group in the command view that holds Command Read-Back, Blocked Programs. |
| Command Read-Back | part | pack-command | coined | The read-back of the command pack's write, made by an independent read of what the program changed. |
| Command Run | group | pack-command | coined | The group in the command view that holds Argument Builder, Supervised Start, Exit Status and Output. |
| Command Run Sequence | group | pack-command | coined | The title of the numbered walk through the command view, told in 6 steps. |
| Command Sequence | group | command-line | coined | The title of the numbered walk through the Command Line view, told in 4 steps. |
| Command Tree | part | command-line | coined | The command tree built from the registry. |
| Command-Line Tool | part | system | coined | A command-line program the person already uses, reached through the command pack. |
| Commands File | part | pack-command | code: a2m-pack-command: src/model.rs | The commands file kind that describes a program's commands, as an endpoints file does for a service. |
| Compilation Sequence | group | compiled-target | coined | The title of the numbered walk through the Compiled Target view, told in 7 steps. |
| Compile Time | group | expr | coined | The group in the Expressions view that holds Type Environment, Expression Compilation. |
| Compiler Output | group | compiled-target | coined | The group in the Compiled Target view that holds Compiled Target Contents, Compiled Target Hash, Atomic Compilation, Drafts and Versions. |
| Compiler Registrations | part | contract | code: a2m-files: src/register.rs | What a pack registers with the compiler: file kinds, lowering of each verb, lint rules, and drafting rules. |
| Compiler Stages | group | compiled-target | coined | The group in the Compiled Target view that holds Parsing, Schema Validation, Resolution, Type-Checking, Lowering, Compiled Target Analysis, Linting, Compiled Target Description, Emission. |
| Condition Waits | part | web-browser | coined | The waits that end on a named event or in-page observer, with a deadline. |
| Connection Phase | group | http-driver | coined | The group in the HTTP Driver view that holds Connection Preparation. |
| Connection Preparation | part | http-driver | coined | The opening of a warm client for the session and origin. |
| Containment | group | web-browser | coined | The group in the Browser Engine view that holds Origin Confinement, Crash and Dialog Reports. |
| Content Check | part | sources | coined | The confirmation that an input is the format it claims, from its content and not its name. |
| Continuation | group | engine-resume | coined | The group in the Reconciliation view that holds Run Continuation. |
| Control Port | part | engine-loop | coined | The engine's one port to the control plane, through which it asks for the compiled target, the policy decision, a session lease, a record lock, a budget, and approval, and records an intent. |
| Core Type Roles | group | core | coined | The title of the numbered walk through the Core Types view, told in 5 steps. |
| Core Types | part | core, system | code: a2m-core | The shared words of the product as types: archetypes, effect classes, hazards, failure classes, evidence levels, statuses, and the compiled target's shape. |
| Crash and Dialog Reports | part | web-browser | coined | The detection of a crash, a disconnect, and a dialog, with a report of whether input reached the page. |
| Crash Recovery | walk | system | coined | A worked case in the Anything2MCP on One Machine view, told in 4 steps, which begins: An intent was recorded and the engine stopped before it could record the outcome. |
| Current Record Read | part | engine-resume | coined | The step of reconciliation that reads the record through the mode its intent was stored for, never another. |
| Data Declaration | group | pack-data | coined | The group in the data view that holds Statements File, Store Kind. |
| Data Discovery | group | pack-data | coined | The group in the data view that holds Catalog Discoverer, Data Drafting Rules. |
| Data Drafting Rules | part | pack-data | coined | The rules by which the data pack drafts one record kind per table with its key, a read by key, and a bounded list read. |
| Data Limits | group | pack-data | coined | The group in the data view that holds Data Refusals. |
| Data Read-Back | part | pack-data | coined | The read-back of the data pack's write, made by a separate read of the row from the store. |
| Data Reads | group | pack-data | coined | The group in the data view that holds Store-Enforced Read-Only, Typed Row Fetch. |
| Data Refusals | part | pack-data | coined | The things the data pack refuses to do, such as statements assembled from inputs. |
| Data Store | part | system | coined | A database the person already uses, reached through the data pack. |
| Data Writes | group | pack-data | coined | The group in the data view that holds Transactional Write, Data Read-Back. |
| Database Write Sequence | group | pack-data | coined | The title of the numbered walk through the data view, told in 6 steps. |
| Decision Stage | group | engine-loop | coined | The group in the Engine Loop view that holds Approval Check. |
| Declarations | group | contract | coined | The group in the Provider Contract view that holds Manifest, Provider Registry. |
| Definition Pins | part | pack-mcp | coined | The digest of each mapped tool's definition, compared with the live definition before a call. |
| Definition Source | group | registry | coined | The group in the Action Registry view that holds Action Definition, Action Catalog. |
| Definition to Call | group | registry | coined | The title of the numbered walk through the Action Registry view, told in 5 steps. |
| Describer | part | web-probe | code: a2m-pack-web-probe: src/describe.rs | The part of the probe that shows an agent, in compact text, what was seen and what was not looked at. |
| Description Findings | part | http-description | coined | The findings made of each operation of a description, at the inferred level. |
| Description Parser | part | http-description | coined | The parser of OpenAPI 3.0 and 3.1 and Swagger 2.0 descriptions within bounds. |
| Description Reader | part | http-description, pack-http | coined | The reading of a description a service publishes about itself into findings. |
| Description Reading | group | http-description | coined | The title of the numbered walk through the Description Reader view, told in 4 steps. |
| Description Source | part | http-description | coined | The getting of a description from the sources folder or from the surface. |
| Desktop Action | group | pack-desktop | coined | The group in the desktop view that holds Desktop Locator, Input Actions, Event Waits. |
| Desktop Application | part | system | coined | A desktop application the person already uses, reached through the desktop pack. |
| Desktop Learning | group | pack-desktop | coined | The group in the desktop view that holds Desktop Probe, Desktop Screen Models. |
| Desktop Locator | part | pack-desktop | coined | The resolution of a screen model's query to one element of the accessibility tree. |
| Desktop Probe | part | pack-desktop | coined | The desktop probe and snapshot, which read a window's shape and never a value, as findings of kind window. |
| Desktop Protection | group | pack-desktop | coined | The group in the desktop view that holds Desktop Serialization, Desktop Refusals. |
| Desktop Reach | group | pack-desktop | coined | The group in the desktop view that holds Window Identification, Accessibility Backend. |
| Desktop Refusals | part | pack-desktop | coined | The things the desktop pack refuses to do, such as acting by position or in an unproven window. |
| Desktop Screen Models | part | pack-desktop | coined | The desktop pack's screen models, each a named window with how to reach it, its fields, and how to know it is ready. |
| Desktop Serialization | part | pack-desktop | coined | The rule that work changing a person's screen runs one request at a time per desktop session. |
| Desktop Write Sequence | group | pack-desktop | coined | The title of the numbered walk through the desktop view, told in 6 steps. |
| Developer | part | system | coined | The person, or the coding agent working for them, who builds tools with the build commands and the same steps offered as MCP tools. |
| Device Action | group | pack-device | coined | The group in the device view that holds Frame Send, Pacing. |
| Device Declaration | group | pack-device | coined | The group in the device view that holds Register Map, Device Discoverer. |
| Device Discoverer | part | pack-device | coined | The discoverer and drafting rules of the device pack, which ask a device to identify and list itself. |
| Device Identification | part | pack-device | coined | The reading of the device's identity and its comparison with the map before any frame. |
| Device Limits | group | pack-device | coined | The group in the device view that holds Firmware, Device Refusals. |
| Device Proof | group | pack-device | coined | The group in the device view that holds Device Read-Back, Lost Link. |
| Device Read-Back | part | pack-device | coined | The read-back of the device pack's write, made by reading the register again. |
| Device Refusals | part | pack-device | coined | The things the device pack refuses to do, such as defeating protections or writing outside the map. |
| Diff Builder | part | engine-preview | coined | The step of a preview that puts every record, its proposed values, its binding, and the binding's evidence level into the diff and hashes it. |
| Digest Reconciliation | part | pack-file | coined | The decision, after a stop, of whether a save landed by the digest of the file at the path. |
| Discovery | part | scout | coined | The scout stage that runs the enabled discoverers for each surface inside the sandbox. |
| Discovery Sandbox | part | engine-loop | coined | The place where drafts and discoverers run and where a write is impossible. |
| Dispatch Stage | group | scheduler | coined | The group in the Scheduler view that holds Queues and Dispatch, Exclusive Resources, Record Lock Waits, Holds. |
| Dispatcher | part | reg-dispatch, registry | code: a2m-actions: src/dispatch.rs | The part that takes one request and does the same seven things for every action, in order. |
| Division of Work | part | scout | coined | The division of work: rules for the runtime, judgment for the developer or their agent, authority for a person. |
| Doubt Checks | group | engine-resume | coined | The group in the Reconciliation view that holds Lock Confirmation, Current Record Read. |
| Doubt Rules | group | ledger-rules | coined | The group in the Ledger Rules view that holds Reconciliation After Doubt, Proof Before Verification, One Run per Call. |
| Draft Check | part | scout | coined | The scout stage that lints and compiles the drafts. |
| Draft Levels | group | scout-ladder | coined | The group in the Evidence Levels view that holds Reconstructed, Schema Verified. |
| Drafted Read | walk | pack-http | coined | A worked case in the http view, told in 5 steps, which begins: The service publishes a description of itself. |
| Drafting Stage | group | http-draft | coined | The group in the Endpoint Drafting view that holds Endpoint Entries, Read Operations. |
| Drafts and Versions | part | compiled-target | coined | The distinction between drafts, which compile in memory, and published versions. |
| Driver Handle | part | contract | code: a2m-provider: src/driver.rs | The only way the engine reaches a driver. |
| Driving Crate | group | pack-web | coined | The group in the web view that holds Web Drivers. |
| Due Times | part | watches | coined | The due times of a watch's checks, worked out on the store's clock. |
| Duplicates and Resume | part | pack-stream | coined | The handling of at-least-once delivery, with an idempotency token and no automatic resume of an irreversible publish. |
| Early Redaction | part | http-description | coined | The passing of the description text through the redactor before parsing. |
| Editable Formats | part | pack-file | coined | The formats the file pack can edit, those with a patch editor that passes two byte-identity tests. |
| Output Writing | part | compiled-target | coined | The compiler stage that writes the compiled target as canonical JSON and takes its hash. |
| Endpoint Catalog | part | pack-http | code: a2m-pack-http: src/endpoints.rs | The endpoints file kind: each endpoint's method, path template, request template, status codes, answer form, and shapes. |
| Endpoint Drafting | part | http-draft, pack-http | coined | The drafting of endpoint entries and one read for each safe method from findings. |
| Endpoint Entries | part | http-draft | coined | The endpoint file drafted with field types from the schema. |
| Endpoint Inference | part | web-probe | coined | The probe step that groups exchanges into endpoints and infers their shapes by comparing digests. |
| Engine Finder | part | bridge | coined | The part of the proxy that takes the start lock, starts the engine if needed, and checks the local channel belongs to the user. |
| Engine Logs | part | telemetry | coined | The structured, levelled, size-rotated, and redacted logs of the engine. |
| Engine MCP Endpoint | part | bridge | coined | The engine's own MCP endpoint, reached on the local channel. |
| Engine Observation | group | telemetry | coined | The group in the Logs and Traces view that holds Spans and Metrics, Engine Logs. |
| Engine Side | group | bridge | coined | The group in the Stdio Proxy view that holds Engine MCP Endpoint. |
| Entry Stage | group | scheduler | coined | The group in the Scheduler view that holds Admission. |
| Envelope Sealing | part | vault | coined | The sealing of a value into an envelope bound to its owner's key and opening it only under the context it was sealed for. |
| Evaluation | part | expr | coined | The evaluation of a loaded expression against bound values with a cost limit. |
| Event Waits | part | pack-desktop | coined | The waits that end on an accessibility event. |
| Events | part | ledger | code: a2m-ledger: src/ledger.rs | The ledger's append-only list of what happened in a run. |
| Evidence Tables | group | ledger | coined | The group in the Ledger view that holds Blobs, Stored Findings, Versions. |
| Exact Decimals | part | core | coined | The exact decimal arithmetic used for any business value. |
| Exclusive Link | part | pack-device | coined | The opening of the device link with exclusive use for a run. |
| Exit Status and Output | part | pack-command | coined | The check of the exit status and output against declared meanings. |
| Expiry and Block Signs | part | web-ui | coined | The recognition of a sign-in page, an edge block, or an expiry sign in a page or response. |
| Expression Compilation | part | expr | coined | The parsing and checking of an expression and its turning into the compiled target's expression form. |
| Expression Lifecycle | group | expr | coined | The title of the numbered walk through the Expressions view, told in 5 steps. |
| Expressions | part | expr, system | code: a2m-expr | The small language a target file uses to compute an input or compare a value, with decimal arithmetic and no floats in a comparison. |
| Extension Points | group | contract | coined | The group in the Provider Contract view that holds Driver, Discoverer, Compiler Registrations, Auth Provider, Source Reader. |
| Face Generation | group | registry | coined | The group in the Action Registry view that holds Generated Faces. |
| Failed Compile | walk | compiled-target | coined | A worked case in the Compiled Target view, told in 5 steps, which begins: Every file is parsed with the place of each value kept, then checked against its schema. |
| Failure | part | core | code: a2m-core: src/failure.rs | The one form in which anything in the kernel reports it could not do what was asked. |
| Field-Type Normalizers | part | expr | coined | The normalizer for each field-type base: decimals, dates, and text case and whitespace. |
| File Change | group | pack-file | coined | The group in the file view that holds Place Locator, Span Patching, Untouched Check. |
| File Discoverer | part | pack-file | coined | The discoverer and drafting rules of the file pack, reporting sheets, tables, headers, and the presence of formulas and macros. |
| File Discovery | group | pack-file | coined | The group in the file view that holds File Discoverer. |
| File Limits | group | pack-file | coined | The group in the file view that holds File Refusals. |
| File Opening | group | pack-file | coined | The group in the file view that holds Path Opening, Editable Formats. |
| File Read-Back | part | pack-file | coined | The read-back of the file pack's write, made by opening the file again from disk and reading the same places. |
| File Refusals | part | pack-file | coined | The things the file pack refuses to do, such as writing a format it cannot write back without loss. |
| File Saving | group | pack-file | coined | The group in the file view that holds Atomic Replace, File Read-Back, Digest Reconciliation. |
| File Write Sequence | group | pack-file | coined | The title of the numbered walk through the file view, told in 7 steps. |
| Files and Folders | part | system | coined | Files and folders the person already uses, reached through the file pack. |
| Finding Drafting | group | http-draft | coined | The title of the numbered walk through the Endpoint Drafting view, told in 5 steps. |
| Finding Levels | group | scout-ladder | coined | The group in the Evidence Levels view that holds Inferred, Discovered. |
| Findings Stage | group | http-description | coined | The group in the Description Reader view that holds Description Findings. |
| Finishing Stages | group | compiled-target | coined | The group in the Compiled Target view that holds Linting, Compiled Target Description, Emission. |
| Firmware | part | pack-device | coined | The device pack's treatment of firmware: inspection of a supplied image is discovery only, and an update is irreversible. |
| Format Readers | part | sources | coined | The built-in readers for workbooks, delimited text, and JSON, and the extension point a pack's own reader fills. |
| Foundation Crates | group | system | coined | The group in the Anything2MCP on One Machine view that holds Core Types, Expressions, Telemetry, Network State, Sources. |
| Four Faces | walk | registry | coined | A worked case in the Action Registry view, told in 5 steps, which begins: The action is defined once, with its name, schemas and lowest role. |
| Frame Send | part | pack-device | coined | The building of one frame from the map and typed values and its sending. |
| Freshness Check | part | engine-write | coined | The step of the write path that reads the record again and compares it when the preview is older than the freshness window or policy is strict. |
| Front Doors | group | system | coined | The group in the Anything2MCP on One Machine view that holds MCP Face, REST Face, Person Pages. |
| Generated Faces | part | registry | code: a2m-actions: src/faces.rs | The command, MCP tool, REST route, and client method generated from each action. |
| Generated SDK | part | system | coined | The Python and TypeScript clients generated from the engine's own description of its actions. |
| Guidance File | part | project | coined | The file written by init that tells an agent starting in the folder how to work. |
| Handler Execution | part | reg-dispatch | coined | The dispatcher step that runs the action's own handler. |
| Handler Stage | group | reg-dispatch | coined | The group in the Dispatcher view that holds Handler Execution. |
| Hardware Device | part | system | coined | A hardware device the person already uses, reached through the device pack. |
| Health Check | part | reg-dispatch | coined | The dispatcher step that checks the engine's health against what the action needs. |
| Held Draft | walk | scout | coined | A worked case in the Scout and Drafter view, told in 5 steps, which begins: Scenario Comparison sets what the system says about itself against what was seen. |
| Held Drafts | group | project | coined | The group in the Project view that holds Holding Folder. |
| Help Discoverer | part | pack-command | coined | The discoverer that runs only the help and version forms the target declares. |
| History Tables | group | ledger | coined | The group in the Ledger view that holds Audit Chain, Watch and Shadow Results. |
| Holding Folder | part | project, scout | coined | The folder where the drafter puts a draft that holds an open question, at the path it would have had. |
| Host Functions | part | expr | coined | The closed list of host functions for text, numbers, dates, and measures. |
| HTTP Action | group | pack-http | coined | The group in the http view that holds HTTP Driver, Sign-In Division. |
| HTTP Discovery | group | pack-http | coined | The group in the http view that holds Description Reader, Endpoint Drafting. |
| HTTP Driver | part | http-driver, pack-http | coined | The http pack's driver that carries out each step of a request through a warm connection. |
| HTTP Knowledge | group | pack-http | coined | The group in the http view that holds Endpoint Catalog, Shape Check, Answer Forms. |
| HTTP Limits | group | pack-http | coined | The group in the http view that holds HTTP Refusals. |
| HTTP Read-Back | part | http-driver | coined | The read-back of the http pack's write, made by an independent read of the record from the service. |
| HTTP Refusals | part | pack-http | coined | The things the http pack refuses to do, such as sending to an unresolved origin or following a redirect. |
| HTTP Service | part | system | coined | An HTTP service the person already uses, reached through the http pack. |
| HTTP Step Sequence | group | http-driver | coined | The title of the numbered walk through the HTTP Driver view, told in 6 steps. |
| Identification | part | scout | coined | The scout stage that probes each declared surface for its kind, its pack, and its sign-in. |
| Input Actions | part | pack-desktop | coined | The setting of a value, invoking, selecting, and expanding through accessibility patterns. |
| Input Reading | part | engine-preview | coined | The step of a preview that validates the call's inputs and reads the source once, storing its bytes. |
| Input Validation | part | reg-dispatch | coined | The dispatcher step that checks the input against the action's full schema. |
| Intent Before Action | part | ledger-rules | coined | The ledger rule that a record reaches acted only if its intent was durably stored before the write was sent. |
| Intent Recording | part | engine-write | coined | The step of the write path that sends the intended values to the ledger and waits until they are durably stored. |
| Interchange Types | group | core | coined | The group in the Core Types view that holds Failure, Finding, Compiled Target Types. |
| Job Leases | part | scheduler | coined | The leases the scheduler grants a runner for one job, each with a number that only increases. |
| Job Sequence | group | scheduler | coined | The title of the numbered walk through the Scheduler view, told in 7 steps. |
| Kernel Control | group | system | coined | The group in the Anything2MCP on One Machine view that holds Action Registry, Policy Engine, Scheduler, Session Keeper. |
| Kernel Execution | group | system | coined | The group in the Anything2MCP on One Machine view that holds Engine Loop, Provider Contract, Scout and Drafter, Watches and Canaries. |
| Kernel File Kinds | group | project | coined | The group in the Project view that holds target.yaml, auth.yaml, types.yaml, operations/, pipelines/. |
| Kernel Handle | part | registry | code: a2m-actions: src/kernel.rs | The in-process object that opens the stores in order, holds the kernel's health, and gives handlers what the engine and keeper ask for. |
| Kernel Services | group | contract | coined | The group in the Provider Contract view that holds Driver Handle, Checked Paths, Supervisor, Plug-In Host. |
| Key Hierarchy | group | vault | coined | The group in the Vault view that holds Root Key, Key Layers. |
| Key Layers | part | vault | coined | The layers of keys: the root wraps workspace keys, which wrap owner keys, which wrap each item's data key. |
| Key Use | group | vault | coined | The group in the Vault view that holds Envelope Sealing, Secret Wrapper. |
| Key-to-Redaction Sequence | group | vault | coined | The title of the numbered walk through the Vault view, told in 6 steps. |
| Landed Outcome | part | engine-resume | coined | The reconcile outcome in which the current values equal the intended ones, so the record is verified. |
| Latest Read | part | pack-stream | coined | The reading of the last retained value for a key. |
| Leading Reads | part | engine-preview | coined | The read steps that sit before the first write step, run during a preview, each with its expectation checked. |
| Learning Crate | group | pack-web | coined | The group in the web view that holds Web Probe. |
| Ledger | part | ledger, system | code: a2m-ledger | The record of every run: its state, each record's intent, what was sent, what was read back, approvals, and traces. |
| Ledger Contents | group | ledger | coined | The group in the Ledger view that holds Runs, Records, Events, Record Locks, Blobs, Stored Findings, Versions, Audit Chain, Watch and Shadow Results. |
| Ledger Mechanics | group | ledger | coined | The group in the Ledger view that holds Run States, Record States, Intent Token, Ledger Rules, Ledger Store. |
| Ledger Rules | part | ledger-rules, ledger | coined | The nine rules that must always hold in the ledger, each one a test and each enforced where the data is. |
| Ledger Store | part | ledger | code: a2m-ledger: src/store.rs, sqlite.rs, migrate.rs | The ledger's storage: one SQLite file with one writing thread and two reading threads. |
| Ledger Write Sequence | group | ledger | coined | The title of the numbered walk through the Ledger view, told in 7 steps. |
| Limits and Quotas | part | policy | coined | The maximum records per run, the quotas per caller, and the checks that apply them. |
| Linting | part | compiled-target | coined | The compiler stage that applies the kernel's and the packs' lint rules. |
| Local Channel | group | bridge | coined | The group in the Stdio Proxy view that holds Engine Finder, Message Relay. |
| Local Channel Client | part | command-line | coined | The client that connects to the engine on the local channel and sends the action. |
| Local Engine | group | system | coined | The frame in the Anything2MCP on One Machine view drawn around the columns Front Doors, Kernel Control, Kernel Execution, Packs, which together make one process. |
| Local Listeners | part | netstate | coined | The sockets listening on the machine. |
| Local Storage | group | system | coined | The group in the Anything2MCP on One Machine view that holds Project, Compiled Target, Ledger, Vault, Browser Profile. |
| Lock Acquisition | part | engine-write | coined | The step of the write path that asks the ledger for the lock on one record in one target and environment. |
| Lock Confirmation | part | engine-resume | coined | The step of reconciliation that takes or confirms the record's lock under the new lease number. |
| Loopback Listener | part | pages | coined | The listener that serves the person's pages on the loopback address only while one is pending. |
| Loss Handling | group | scheduler | coined | The group in the Scheduler view that holds Lost Leases. |
| Lost Answer | walk | ledger | coined | A worked case in the Ledger view, told in 5 steps, which begins: The write was sent and the engine stopped before it saw the answer. |
| Lost HTTP Answer | walk | pack-http | coined | A worked case in the http view, told in 4 steps, which begins: The driver builds the request from the template, places the session's credential, and sends it once. |
| Lost Leases | part | scheduler | coined | The scheduler's handling of a job whose lease ran out: it is declared lost, stopped as possible_commit, and a reconcile job is queued after a quarantine. |
| Lost Link | part | pack-device | coined | The handling of a link lost after a write, which may have been applied and is marked possible_commit. |
| Step Expansion | part | compiled-target | coined | The compiler stage that turns each binding's verb into the fixed list of steps its driver runs. |
| Machine Actions | part | registry | coined | The group of actions for the local engine, its settings, doctor, network view, browser install, plug-ins, and built-in docs. |
| Machine Scope | group | registry | coined | The group in the Action Registry view that holds Machine Actions, Absent Actions, Kernel Handle. |
| Match Wait | part | pack-stream | coined | The waiting, within a time limit, for the next message matching a key or correlation field. |
| MCP Call Sequence | group | mcp-face | coined | The title of the numbered walk through the MCP Face view, told in 6 steps. |
| MCP Calling | group | pack-mcp | coined | The group in the mcp view that holds Upstream Connection, Tool Call, MCP Read-Back. |
| MCP Discovery | group | pack-mcp | coined | The group in the mcp view that holds MCP Tool Discoverer, MCP Drafting Rules. |
| MCP Drafting Rules | part | pack-mcp | coined | The rules by which the mcp pack drafts a pass-through read for each tool the server marks read-only. |
| MCP Endpoint | part | mcp-face | coined | The HTTP endpoint of the MCP face, one per toolset, carried on the local channel. |
| MCP Face | part | mcp-face, system | code: a2m-mcp | The front door that presents the published tools and the built-in a2m tools to an agent and hands every call to the action registry. |
| MCP Intake | group | mcp-face | coined | The group in the MCP Face view that holds MCP Endpoint, Protocol Revisions. |
| MCP Limits | group | pack-mcp | coined | The group in the mcp view that holds Untrusted Text, MCP Refusals. |
| MCP Mapping | group | mcp-face | coined | The group in the MCP Face view that holds Tool List, Call Mapping, Sign-In and Approval Links. |
| MCP Output | group | mcp-face | coined | The group in the MCP Face view that holds MCP Results, Progress and Long Runs. |
| MCP Read-Back | part | pack-mcp | coined | The read-back of the mcp pack's write, made by an independent read through the upstream server. |
| MCP Refusals | part | pack-mcp | coined | The things the mcp pack refuses to do, such as reaching a tool the mapping does not name. |
| MCP Results | part | mcp-face | coined | The structured content, summary, links, and annotations of an MCP answer. |
| MCP Server | part | system | coined | An existing MCP server the person already uses, reached through the mcp pack. |
| MCP Setup | group | pack-mcp | coined | The group in the mcp view that holds Mapping File, Definition Pins. |
| MCP Tool Discoverer | part | pack-mcp | coined | The discoverer that lists an upstream server's tools through the sandbox port as findings. |
| Message Broker | part | system | coined | A message broker the person already uses, reached through the stream pack. |
| Message Publish | part | pack-stream | coined | The encoding of one message from a typed template, its check against the schema, and its sending. |
| Message Relay | part | bridge | coined | The part of the proxy that copies messages unchanged. |
| Message Take | part | pack-stream | coined | The taking and acknowledging of a message, compiled only on a channel the runtime owns. |
| Name Servers | part | netstate | coined | The name servers the machine is set to use. |
| Naming Stage | group | http-draft | coined | The group in the Endpoint Drafting view that holds Unique Names. |
| Network Bounds | part | netstate | coined | The bounds on every list, with cuts recorded and privacy choices applied. |
| Network Interfaces | part | netstate | coined | The machine's network interfaces, their addresses, and state. |
| Network Neighbours | part | netstate | coined | The neighbour table the system already holds. |
| Network Readers | group | netstate | coined | The group in the Network State view that holds Network Interfaces, Network Routes, Name Servers, Network Neighbours, Local Listeners, System Proxy. |
| Network Reading | group | netstate | coined | The title of the numbered walk through the Network State view, told in 3 steps. |
| Network Routes | part | netstate | coined | The machine's routing table. |
| Place Reading | part | netstate | coined | The join of interfaces and routes into what kind of place each address is and which interface carries each preferred default route. |
| Network State | part | netstate, system | code: a2m-netstate | The crate that reads which networks this machine is on and what kind of place an address is. |
| Normalized Comparison | part | engine-loop | coined | The shared comparison that turns values into the form their field type says, so that equal values compare equal. |
| Not-Landed Outcome | part | engine-resume | coined | The reconcile outcome in which the current values equal those seen before the write, so the record is planned again. |
| Notification | part | pages | coined | The system notification and the link in the result of the waiting run. |
| Nullable Rule | part | http-draft | coined | The rule that a property not under required is written nullable. |
| Observation Stage | group | scout | coined | The group in the Scout and Drafter view that holds Identification, Discovery. |
| One Lock per Record | part | ledger-rules | coined | The ledger rule that at most one run holds the lock for a record key in a target and environment. |
| One Run per Call | part | ledger-rules | coined | The ledger rule that at most one unfinished run exists for the same caller, tool version, and input hash. |
| One Writer per Run | part | ledger-rules | coined | The ledger rule that only the holder of a run's current lease number may change its records. |
| OpenAPI Document | part | rest-face | coined | The OpenAPI description of every route, generated from the registry. |
| operations/ | part | project | code: a2m-files: src/schema.rs | The folder holding one file per operation. |
| Origin Confinement | part | web-browser | coined | The holding of every request of a tab to the surface's origin and the destinations its tool declared. |
| Outcomes | group | policy | coined | The group in the Policy Engine view that holds Policy Decision, Result Masking. |
| Output and Exit Codes | part | command-line | coined | The printing of each response and the choice of exit code from the response's exit class. |
| Pacing | part | pack-device | coined | The rule that a link carries one request at a time, with a pause between requests. |
| Pack File Group | group | project | coined | The group in the Project view that holds Pack File Kinds. |
| Pack File Kinds | part | project | coined | The folders a pack brings, such as screens, endpoints, commands, and registers. |
| Pack Registration | group | contract | coined | The title of the numbered walk through the Provider Contract view, told in 6 steps. |
| Packs | group | system | coined | The group in the Anything2MCP on One Machine view that holds web, http, mcp, desktop, command, data, file, stream, device. |
| Page Interaction | group | web-browser | coined | The group in the Browser Engine view that holds Web Locator, Actionability and Input, Condition Waits. |
| Page Lifecycle | group | web-browser | coined | The title of the numbered walk through the Browser Engine view, told in 7 steps. |
| Page Observation | group | web-browser | coined | The group in the Browser Engine view that holds Page Snapshots, Page Requests. |
| Page Sequence | group | pack-web | coined | The title of the numbered walk through the web view, told in 6 steps. |
| Page Snapshots | part | web-browser | coined | The snapshots of a page built from the accessibility tree in one form shared by evidence and discovery. |
| Page Write | walk | pack-web | coined | A worked case in the web view, told in 5 steps, which begins: The binding names the ui mode. |
| Parsing | part | compiled-target | coined | The compiler stage that reads every YAML file and keeps the position of each value. |
| Parsing Stage | group | http-description | coined | The group in the Description Reader view that holds Description Parser. |
| Path Opening | part | pack-file | coined | The receipt of a path resolved and checked to lie inside an allowed root. |
| Peer Readers | group | netstate | coined | The group in the Network State view that holds Network Neighbours, Local Listeners, System Proxy. |
| Per-Record Preview | group | engine-preview | coined | The group in the Preview view that holds Record Lookup. |
| Permission Manifest | part | policy | code: a2m-policy: src/rules.rs | The declaration by each operation of the surfaces, downloads, uploads, and destinations it will touch. |
| Person | part | system | coined | The human at this machine, who alone approves a write, publishes a version, and signs in. |
| Person Confirmation | part | watches | coined | The confirmation by a person on the engine's page, before which a watch checks nothing. |
| Person Display | group | pages | coined | The group in the Person Pages view that holds Approval Page, Act Page, Sign-In Window. |
| Person Level | group | scout-ladder | coined | The group in the Evidence Levels view that holds Approved. |
| Person Pages | part | pages, system | coined | The front door of pages a person sees, for approval, for a person's act, and for sign-in, proven by the operating system's presence prompt. |
| Person Prompt | part | session-keeper | coined | The request to a person to sign in again, made only to the session's own identity. |
| Person Request | group | pages | coined | The group in the Person Pages view that holds Notification, Loopback Listener. |
| Person Requirement | part | reg-dispatch | coined | The dispatcher step that requires proof of presence from the person's own surface for an act that is a person's. |
| pipelines/ | part | project | code: a2m-files: src/schema.rs | The folder holding one file per pipeline. |
| Place Locator | part | pack-file | coined | The finding of each named place in the parsed form with its position in the bytes. |
| Compiled Target Analysis | part | compiled-target | coined | The compiler stage that counts commit steps and works out each effect class and manifest. |
| Compiled Target Contents | part | compiled-target | coined | What a compiled target holds: header, target, field types, pack sections, operations with steps, pipelines, expressions, and tools. |
| Compiled Target Description | part | compiled-target | coined | The compiler stage that generates each tool's name, description, schemas, and annotations. |
| Compiled Target Hash | part | compiled-target | code: a2m-core: src/digest.rs | The digest of all the compiled target's bytes, to which a version and an approval are tied. |
| Compiled Target Types | part | core | coined | The compiled target and its steps as immutable data, with their canonical encoding and hash check. |
| Plug-In Host | part | contract | coined | The host that runs providers written in other languages as separate processes speaking a fixed protocol. |
| Policy Decision | part | policy | coined | The stored decision of the policy engine for one call, with the rules that decided it and the policy version. |
| Policy Query | part | reg-dispatch | coined | The dispatcher step that gets one decision from the policy engine and refuses to go on without it. |
| Policy Refusal | walk | policy | coined | A worked case in the Policy Engine view, told in 4 steps, which begins: A call arrives. |
| Positioned Parse | part | sources | coined | The parsed form that keeps the position of every value in the bytes. |
| PostgreSQL Ledger | part | system | coined | The ledger's same schema on a database server, for more than one engine process, built after the first release. |
| Pre-Frame Checks | group | pack-device | coined | The group in the device view that holds Device Identification, Exclusive Link. |
| Pre-Handler Stages | group | reg-dispatch | coined | The group in the Dispatcher view that holds Action Lookup, Input Validation, Health Check, Policy Query, Person Requirement. |
| Pre-Send Steps | group | engine-write | coined | The group in the Write Path view that holds Lock Acquisition, Freshness Check, Intent Recording. |
| Pre-Write Rules | group | ledger-rules | coined | The group in the Ledger Rules view that holds Intent Before Action, Approval Bound to Diff, One Writer per Run. |
| Preparation | part | engine-loop | coined | The part of the engine loop that asks for a session lease through the Control Port and lets the driver get what the binding needs. |
| Presence Prompt | part | pages | coined | The operating system's own user-presence prompt. |
| Presence Proof | group | pages | coined | The group in the Person Pages view that holds Presence Prompt, Approval Recorder. |
| Preview | part | engine-preview, engine-loop | coined | The first stage of a write, which builds the diff before anyone is asked: it reads the source, runs the leading reads, and classes each record. |
| Preview Inputs | group | engine-preview | coined | The group in the Preview view that holds Input Reading, Leading Reads. |
| Preview Outcome | part | engine-preview | coined | The step of a preview that either completes a run with nothing to change, or asks policy whether the write may go without a person, or rests the run for approval. |
| Preview Result | group | engine-preview | coined | The group in the Preview view that holds Diff Builder, Preview Outcome. |
| Preview Stage | group | engine-loop | coined | The group in the Engine Loop view that holds Preview. |
| Probe Observation | group | web-probe | coined | The group in the Web Probe view that holds Surface Identification, Snapshot Findings. |
| Probe Presentation | group | web-probe | coined | The group in the Web Probe view that holds Site-Declared Tools, Describer. |
| Probe Sequence | group | web-probe | coined | The title of the numbered walk through the Web Probe view, told in 5 steps. |
| Process Commands | part | command-line | coined | The commands that act on the process, not on the engine: serve, bridge, and runner. |
| Process Group | group | command-line | coined | The group in the Command Line view that holds Process Commands. |
| Program Check | part | pack-command | coined | The check of the program's path, ownership, digest, and version before it is started. |
| Progress and Long Runs | part | mcp-face | coined | The handling of a long action, which returns a run id and reports progress. |
| Project Level | group | project | coined | The group in the Project view that holds Project Settings, Guidance File, Working Output, Handed Files. |
| Project Settings | part | project | coined | The anything2mcp.toml file saying which targets and environments the project holds. |
| Proof Before Verification | part | ledger-rules | coined | The ledger rule that a record reaches verified only with a value read from the system that equals the intended value after normalization. |
| Proof Phase | group | http-driver | coined | The group in the HTTP Driver view that holds HTTP Read-Back. |
| Proof Stage | group | scout | coined | The group in the Scout and Drafter view that holds Draft Check, Trial. |
| Protocol Revisions | part | mcp-face | coined | The protocol revisions the MCP face serves, the newest stateless one and an older one. |
| Proven Levels | group | scout-ladder | coined | The group in the Evidence Levels view that holds Executed, Behavior Verified. |
| Provider Actions | part | registry | coined | The group of actions for listing packs, running discoverers, handling findings, and running the scout. |
| Provider Contract | part | contract, system | code: a2m-provider | The one interface between the kernel and the packs: driver, discoverer, auth provider, source reader, and the manifest. |
| Provider Registry | part | contract | code: a2m-provider: src/registry.rs | The immutable snapshot of every provider, its modes, and what each supplied, rebuilt whole when a provider is switched. |
| Proxied Message | group | bridge | coined | The title of the numbered walk through the Stdio Proxy view, told in 3 steps. |
| Publish Actions | part | registry | coined | The group of actions for publishing, promoting, activating, rolling back, bundling, and importing versions. |
| Publish Proof | group | pack-stream | coined | The title of the numbered walk through the stream view, told in 7 steps. |
| Queues and Dispatch | part | scheduler | coined | The scheduler's bounded queue per target and environment, from which jobs are handed out. |
| Rate Budgets | part | scheduler | coined | The request budgets for each surface, granted to runs in batches. |
| Reach Stage | group | command-line | coined | The group in the Command Line view that holds Local Channel Client. |
| Read | walk | system | coined | A worked case in the Anything2MCP on One Machine view, told in 8 steps, which begins: The agent calls a published tool. |
| Read Execution | part | engine-loop | coined | The part of the engine loop that works out each step's inputs, spends the rate budget, and calls the driver through its handle for a read. |
| Read Operations | part | http-draft | coined | The one read drafted for each safe method. |
| Read Sequence | group | engine-loop | coined | The group in the Engine Loop view that holds Guard, Binding Selector, Preparation, Read Execution, Result Store. |
| Reader Limits | group | netstate | coined | The group in the Network State view that holds Network Bounds. |
| Reading Stages | group | compiled-target | coined | The group in the Compiled Target view that holds Parsing, Schema Validation, Resolution. |
| Reconcile Outcomes | group | engine-resume | coined | The group in the Reconciliation view that holds Landed Outcome, Not-Landed Outcome, Unresolved Outcome. |
| Reconciliation | part | engine-resume, engine-loop | coined | The part of the engine loop that, after a stop, reads every record that was in doubt before anything else is written. |
| Reconciliation After Doubt | part | ledger-rules | coined | The ledger rule that no new intent is stored while a record of the run is in intent or acted under an older lease. |
| Record Lifecycle | walk | ledger | coined | A worked case in the Ledger view, told in 6 steps, which begins: A call becomes a run row in the state queued. |
| Record Lock Waits | part | scheduler | coined | The scheduler's queue of runs that want a record lock the ledger reports as held, woken when the ledger releases it. |
| Record Lookup | part | engine-preview | coined | The step of a preview that finds each record by its identity strategy and marks it create, update, or unchanged. |
| Record States | part | ledger | code: a2m-core: src/state.rs | The states a record moves through: planned, unchanged, intent, acted, verified, and failed. |
| Record Write Sequence | group | engine-write | coined | The title of the numbered walk through the Write Path view, told in 7 steps. |
| Record Writing | group | engine-loop | coined | The group in the Engine Loop view that holds Write Path. |
| Recordable Headers | part | telemetry | coined | The one list of headers an exchange may record. |
| Action Catalog | part | registry | code: a2m-actions: src/catalog.rs | The digested list of every action definition. |
| Recovery Stage | group | engine-loop | coined | The group in the Engine Loop view that holds Reconciliation. |
| Redaction Stage | group | http-description | coined | The group in the Description Reader view that holds Early Redaction. |
| Redactor | part | vault | code: a2m-vault: src/redactor.rs | The part that knows every secret the vault has released and removes it from results, logs, traces, and errors. |
| Refusal | walk | system | coined | A worked case in the Anything2MCP on One Machine view, told in 3 steps, which begins: The call arrives at a front door. |
| Register Map | part | pack-device | code: a2m-pack-device: src/map.rs | The registers file kind: addresses, protocol, types, units, access, read effects, and the serial of the device in each environment. |
| Register Write Sequence | group | pack-device | coined | The title of the numbered walk through the device view, told in 6 steps. |
| Remote Runners | part | system | coined | Later workers on other machines that claim a job, hold a lease, and report back. |
| Rendering Stage | group | command-line | coined | The group in the Command Line view that holds Output and Exit Codes. |
| Renewal Order | part | session-keeper | coined | The order of renewal attempts: silent first, then a light prompt, then a full sign-in, each rung tried once. |
| Renewal Stage | group | session-keeper | coined | The group in the Session Keeper view that holds Renewal Order, Person Prompt. |
| Reporting Stage | group | scout | coined | The group in the Scout and Drafter view that holds Scout Report. |
| Request Sender | part | http-driver | coined | The building of the request from the template and its single sending. |
| Resolution | part | compiled-target | coined | The compiler stage that links names: operations to screens, endpoints, and types, and pipelines to operations and sources. |
| REST Face | part | rest-face, system | code: a2m-api, a2m-local | The front door that offers one route for each action, used by the command line and the clients over the local channel. |
| REST Face Contents | group | rest-face | coined | The title of the numbered walk through the REST Face view, told in 5 steps. |
| REST Routes | group | rest-face | coined | The group in the REST Face view that holds Action Routes, OpenAPI Document, Run Events. |
| Result Masking | part | policy | coined | The last control before an answer leaves, which hides fields the caller may not see. |
| Result Store | part | engine-loop | coined | The part of the engine loop that sends a read's result and evidence to the control plane to be stored. |
| Resume After Stop | walk | engine-loop | coined | A worked case in the Engine Loop view, told in 4 steps, which begins: The engine stops after an intent was stored and before the answer was seen. |
| Resume Sequence | group | engine-resume | coined | The title of the numbered walk through the Reconciliation view, told in 6 steps. |
| Retention Stage | group | session-keeper | coined | The group in the Session Keeper view that holds Session Lease, Saved Session State. |
| Retrieval Stage | group | http-description | coined | The group in the Description Reader view that holds Description Source. |
| Roles and Grants | part | policy | coined | The rules giving a principal access to a scope, with roles, evaluated by the policy engine. |
| Root Key | part | vault | code: a2m-vault: src/store/ | The installation's one root key, kept in the operating system's key store or derived from a passphrase. |
| Run Continuation | part | engine-resume | coined | The step of reconciliation after every doubtful record is settled, in which planned records go through the normal Write Path. |
| Run Events | part | rest-face | coined | A page of a run's events, and the same as a resumable stream. |
| Run Recording | group | telemetry | coined | The group in the Logs and Traces view that holds Step Trace, Capture Levels, Trace Signature. |
| Run States | part | ledger | code: a2m-core: src/state.rs | The states a run moves through, from queued to the final states rejected, expired, completed, failed, and cancelled. |
| Run Tables | group | ledger | coined | The group in the Ledger view that holds Runs, Records, Events, Record Locks. |
| Run Time | group | expr | coined | The group in the Expressions view that holds Evaluation, Host Functions, Field-Type Normalizers. |
| Running Stage | group | scheduler | coined | The group in the Scheduler view that holds Job Leases, Rate Budgets. |
| Safe Methods | part | http-draft | coined | The rule that drafts are made only for methods the description marks safe. |
| Safety Types | group | core | coined | The group in the Core Types view that holds Secret, Address Scope. |
| Saved Session State | part | session-keeper | coined | The sealed session state, such as cookies, kept in the vault and restored after a restart. |
| Scenario Comparison | part | scout | coined | The scout stage that sets what a system describes against what was observed. |
| Scheduler | part | scheduler, system | code: a2m-sched | The part that admits or refuses a run against queues, limits, and budgets, hands out jobs under leases, and times watches. |
| Schema Validation | part | compiled-target | coined | The compiler stage that checks each file against its published schema and the target's schema version. |
| Scout Bounds | part | scout | coined | The limits on time, size, depth, and requests that every scout stage has. |
| Scout Findings | part | scout | coined | The facts the scout learned about a system, each with its subject, kind, level, limits, and sources. |
| Scout Pipeline | group | scout | coined | The group in the Scout and Drafter view that holds Identification, Discovery, Scenario Comparison, Drafter, Draft Check, Trial, Scout Report. |
| Scout Report | part | scout | coined | The scout's one report of what was found, drafted, proven, held, and open. |
| Scout Rules | group | scout | coined | The group in the Scout and Drafter view that holds Evidence Levels, Scout Bounds, Division of Work. |
| Scout Run | group | scout | coined | The title of the numbered walk through the Scout and Drafter view, told in 7 steps. |
| Scout Stores and Rules | group | scout | coined | The group in the Scout and Drafter view that holds Scout Findings, Open Questions, Holding Folder, Evidence Levels, Scout Bounds, Division of Work. |
| Secret | part | core | code: a2m-core: src/secret.rs | The wrapper that cannot be printed or serialized, with the two interfaces the vault implements. |
| Secret Containment | walk | vault | coined | A worked case in the Vault view, told in 6 steps, which begins: The root key is kept in the operating system's key store, or derived from a passphrase. |
| Secret Exclusion | group | vault | coined | The group in the Vault view that holds Redactor, Keyed Digest. |
| Secret Wrapper | part | vault | coined | The only form in which a secret leaves the vault. |
| Selection Stage | group | http-draft | coined | The group in the Endpoint Drafting view that holds Safe Methods. |
| Send Outcome | part | http-driver | coined | The report, for each failure, of whether the request was sent, not sent, or unknown. |
| Server Extensions | group | rest-face | coined | The group in the REST Face view that holds Webhooks, Server Listener. |
| Server Grants and Toolsets | part | system | coined | Later rules for who may see and call which tools on a shared server, compiled into policy. |
| Server Listener | part | rest-face | coined | The network listener of the later server role, with TLS, token checking, and the console. |
| Service Read Path | group | pack-http | coined | The title of the numbered walk through the http view, told in 6 steps. |
| Session Checks | part | session-keeper | coined | The target's declared cheap signed-in reads that the keeper runs, with jitter, only while the session is wanted. |
| Session Clocks | part | session-keeper | coined | The three limits on a session's life: the system's idle timeout, token expiry, and the identity provider's maximum age. |
| Session Expiry | walk | system | coined | A worked case in the Anything2MCP on One Machine view, told in 4 steps, which begins: A step fails because the system no longer accepts the session. |
| Session Lease | part | session-keeper | code: a2m-auth: src/lease.rs | The handle a run receives to use a session for one job, in place of the credential. |
| Session Lifecycle | group | session-keeper | coined | The group in the Session Keeper view that holds Session States, Session Clocks, Session Checks. |
| Session Sequence | group | session-keeper | coined | The title of the numbered walk through the Session Keeper view, told in 7 steps. |
| Session States | part | session-keeper | code: a2m-auth: src/state.rs | The states of a session: absent, signing_in, valid, suspect, renewing, needs_person, expired, and revoked. |
| Setup Stage | group | engine-loop | coined | The group in the Engine Loop view that holds Preparation. |
| Shadow Read | part | engine-loop | coined | The shadow reads of the engine loop, which predict what a binding would do without acting. |
| Shape Check | part | pack-http | code: a2m-pack-http: src/shape.rs | The check of an answer against the endpoint's response shape. |
| Shaping Stage | group | http-draft | coined | The group in the Endpoint Drafting view that holds Nullable Rule. |
| Shared Expiry | walk | session-keeper | coined | A worked case in the Session Keeper view, told in 6 steps, which begins: Ten runs each hold a lease. |
| Shared Server Additions | group | system | coined | The group in the Anything2MCP on One Machine view that holds Signed-In MCP Endpoint, Remote Runners, PostgreSQL Ledger, Server Grants and Toolsets. |
| Shared Services | group | engine-loop | coined | The group in the Engine Loop view that holds Normalized Comparison, Failure Class, Control Port, Discovery Sandbox, Shadow Read. |
| Sign-In and Approval Links | part | mcp-face | coined | The sign-in and approval links a result carries when a run rests for a person. |
| Sign-In Division | part | pack-http | coined | The division of sign-in work: the auth provider holds the credential, and the driver only carries out the scheme's round trips. |
| Sign-In Guard | part | session-keeper | coined | The check that every address a provider would send a credential, token, or the person to is among those the target declares. |
| Sign-In Stage | group | session-keeper | coined | The group in the Session Keeper view that holds Built-In Auth Providers, Sign-In Guard. |
| Sign-In Window | part | pages | coined | The visible window of the engine's own browser at a system's sign-in page. |
| Signed-In MCP Endpoint | part | system | coined | A later front door by which agents on other machines reach the engine over the network, each signed in and acting only as themselves. |
| Single Act | part | engine-write | coined | The step of the write path that calls the driver's committing step once, with the intent token. |
| Single-Use Approval | walk | policy | coined | A worked case in the Policy Engine view, told in 5 steps, which begins: A write passes the grants, the manifest and the limits. |
| Site-Declared Tools | part | web-probe | coined | The tools a page registers for agents, read as untrusted findings. |
| site_tools | part | web-ui | code: a2m-pack-web-ui: src/driver.rs | The web mode that calls tools a site registers in the browser, after checking the tool and its schema digest. |
| Snapshot Findings | part | web-probe | coined | The probe step that turns a snapshot into findings of screens, fields, and actions. |
| Source Mapping | part | sources | coined | The location of data in an input, filtering of rows, and turning them into fields as the Source Definitions say. |
| Source Output | group | sources | coined | The group in the Sources view that holds Validated Records, Positioned Parse. |
| Source Reading | group | sources | coined | The group in the Sources view that holds Content Check, Format Readers. |
| Source Understanding | group | sources | coined | The group in the Sources view that holds Template Fingerprints, Source Mapping, Cell Typing. |
| Source Files | group | project | coined | The group in the Project view that holds Source Definitions. |
| Source Definitions | part | project | glossary | The files under a target's `sources/` folder: how a kind of input file is read and mapped, compiled with the target (decision 0309). |
| Span Patching | part | pack-file | coined | The building of new bytes by replacing only the spans of the named places. |
| Spans and Metrics | part | telemetry | coined | The spans and counters with a closed set of attributes that cannot carry a value or a secret. |
| Spreadsheet Import | group | sources | coined | The title of the numbered walk through the Sources view, told in 6 steps. |
| Standard Streams | part | bridge | coined | The standard input and output through which the stdio proxy speaks MCP with the client. |
| Standing Join | group | netstate | coined | The group in the Network State view that holds Place Reading. |
| Standing Rules | group | ledger-rules | coined | The group in the Ledger Rules view that holds One Lock per Record, Final State Rule, Append-Only History. |
| State Machines | group | ledger | coined | The group in the Ledger view that holds Run States, Record States. |
| Statements File | part | pack-data | code: a2m-pack-data: src/model.rs | The statements file kind that names a database surface, its record kinds, and every statement a binding may run. |
| Stdio Proxy | part | bridge, system | code: anything2mcp bridge | A small program an MCP client starts, which copies messages between standard input and output and the engine's local channel and holds no engine logic. |
| Storage Layer | group | ledger | coined | The group in the Ledger view that holds Ledger Store. |
| Storage Stage | group | engine-loop | coined | The group in the Engine Loop view that holds Result Store. |
| Store Kind | part | pack-data | coined | The declaration of whether a database is the system of record or belongs to an application. |
| Store-Enforced Read-Only | part | pack-data | coined | The rule that a read statement runs only on a connection the store itself holds read-only. |
| Stored Findings | part | ledger | coined | What discovery found about a system, and the questions it could not settle, kept as data in the ledger. |
| Stream Declaration | group | pack-stream | coined | The group in the stream view that holds Channel File, Stream Discoverer. |
| Stream Discoverer | part | pack-stream | coined | The discoverer and drafting rules of the stream pack, listing channels and their shapes. |
| Stream Read-Back | part | pack-stream | coined | The read-back of the stream pack's write where one exists, which only a retained state channel with no outside readers allows. |
| Stream Reads | group | pack-stream | coined | The group in the stream view that holds Latest Read, Match Wait, Bounded List. |
| Stream Takes | group | pack-stream | coined | The group in the stream view that holds Message Take. |
| Stream Writes | group | pack-stream | coined | The group in the stream view that holds Message Publish, Stream Read-Back, Duplicates and Resume. |
| Supervised Start | part | pack-command | coined | The start of the program through the supervisor in a new process group, with declared handles and a fresh empty folder. |
| Surface Identification | part | web-probe | coined | The probe step that says what a web surface is: platform, framework, support tier, sign-in method, and candidate surfaces. |
| System Proxy | part | netstate | coined | The machine's proxy setting. |
| Tab Postures | part | web-browser | coined | The tabs of one browser process, each either able to act or only observing. |
| Target Build Actions | part | registry | coined | The group of actions for adding, probing, drafting, testing, and comparing targets. |
| Target Folder | group | project | coined | The group in the Project view that holds target.yaml, auth.yaml, types.yaml, operations/, pipelines/, Pack File Kinds, Tests and Fixtures, Holding Folder. |
| Target Layout | group | project | coined | The title of the numbered walk through the Project view, told in 6 steps. |
| Target Systems | group | system | coined | The group in the Anything2MCP on One Machine view that holds Web Application, HTTP Service, MCP Server, Desktop Application, Command-Line Tool, Data Store, Files and Folders, Message Broker, Hardware Device. |
| target.yaml | part | project | code: a2m-files: src/schema.rs | The target file naming surfaces, environments, locale, and defaults. |
| Logs and Traces | part | telemetry, system | code: a2m-logs | The crate for logs and traces, with every registered secret removed before anything is written. |
| Template Fingerprints | part | sources | coined | The fingerprints of a source template's structure that decide whether an input matches a known template version. |
| Final State Rule | part | ledger-rules | coined | The ledger rule that a run in a final state never changes again. |
| Test Files | group | project | coined | The group in the Project view that holds Tests and Fixtures. |
| Tests and Fixtures | part | project | coined | The expected results per operation and the recorded pages or responses used by offline tests. |
| Thin Clients | group | system | coined | The group in the Anything2MCP on One Machine view that holds Stdio Proxy, Command Line, Generated SDK. |
| Timer Work | group | scheduler | coined | The group in the Scheduler view that holds Watch Timing. |
| Tool Build | walk | system | coined | A worked case in the Anything2MCP on One Machine view, told in 6 steps, which begins: The developer, or their agent, registers a system and asks the scout to look at it. |
| Tool Call | part | pack-mcp | coined | The one step kind of the mcp pack, call_tool, with typed arguments. |
| Tool List | part | mcp-face | coined | The list of tools offered: the control tools of the use toolset and the authoring ones of the build toolset. |
| Tool Use Actions | part | registry | coined | The group of actions for searching, calling, and following tools, runs, sessions, and watches. |
| Trace Lifecycle | group | telemetry | coined | The title of the numbered walk through the Logs and Traces view, told in 5 steps. |
| Traffic Capture | part | web-probe | coined | The probe step that records the exchanges seen while a scenario runs, as shapes tied to scenario steps. |
| Traffic Learning | group | web-probe | coined | The group in the Web Probe view that holds Traffic Capture, Endpoint Inference. |
| Transactional Write | part | pack-data | coined | The write in a declared transaction with bound parameters and an expected row count. |
| Tree Stage | group | command-line | coined | The group in the Command Line view that holds Command Tree. |
| Type Environment | part | expr | coined | The names an expression may use and their types, as the caller declares them. |
| Type-Checking | part | compiled-target | coined | The compiler stage that checks every expression against input and output schemas and field types. |
| Typed Row Fetch | part | pack-data | coined | The fetching of rows into typed values up to the statement's row limit. |
| types.yaml | part | project | code: a2m-files: src/schema.rs | The file of field types: units, rounding, code lists, and how the system writes a value. |
| ui Step Sequence | group | web-ui | coined | The title of the numbered walk through the Web Drivers view, told in 5 steps. |
| Unattended Path | group | policy | coined | The group in the Policy Engine view that holds Pre-Approval Rules. |
| Unique Names | part | http-draft | coined | The rule that no two drafted tools share a name or a file. |
| Unresolved Outcome | part | engine-resume | coined | The reconcile outcome in which the values match neither, so the run stops as changed since preview. |
| Untouched Check | part | pack-file | coined | The check that every place outside the named ones is unchanged after the patch. |
| Untrusted Text | part | pack-mcp | coined | The rule that any text from an upstream server is passed on only as untrusted data. |
| Upstream Call | group | pack-mcp | coined | The title of the numbered walk through the mcp view, told in 6 steps. |
| Upstream Connection | part | pack-mcp | coined | The one connection per surface and identity to an upstream server. |
| Usage Limits | group | policy | coined | The group in the Policy Engine view that holds Limits and Quotas. |
| Validated Records | part | sources | coined | The records a source reader hands on after they are checked against their field types. |
| Validation Phase | group | http-driver | coined | The group in the HTTP Driver view that holds Answer Validation, Send Outcome. |
| Value Model | part | core | code: a2m-core: src/value.rs | The model of what a field's value can be once read from a file, a caller, or a system. |
| Value Types | group | core | coined | The group in the Core Types view that holds Value Model, Exact Decimals. |
| Verification | part | engine-write | coined | The step of the write path that normalizes the intended and read values with the field types and compares them before marking the record verified. |
| Verified Write | walk | system | coined | A worked case in the Anything2MCP on One Machine view, told in 7 steps, which begins: The call arrives and is allowed, as for a read. |
| Vocabulary Types | group | core | coined | The group in the Core Types view that holds Closed Vocabularies, Checked Names. |
| Watch and Shadow Results | part | ledger | coined | The ledger tables behind watches, with their checks, changes and baseline, and behind shadow bindings, with each comparison and promotion. |
| Watch Creation | group | watches | coined | The group in the Watches and Canaries view that holds Watch Request, Person Confirmation. |
| Watch End | part | watches | coined | The ending of a watch, by time, condition, person, caller, or withdrawal of its operation's version. |
| Watch Lifecycle | group | watches | coined | The title of the numbered walk through the Watches and Canaries view, told in 7 steps. |
| Watch Outputs | group | watches | coined | The group in the Watches and Canaries view that holds Changes, Watch End. |
| Watch Request | part | watches | coined | The action that asks for a watch over one published read, one fixed input, an interval, and an end. |
| Watch Timing | part | scheduler | coined | The part of the scheduler that works out when each check of a watch is due, on the store's clock, and admits it as the lowest priority work. |
| Watches and Canaries | part | watches, system | coined | The part that repeats a published read at an interval a person confirmed and reports what changed, together with the planned canary, a small read run before real work. |
| Web Application | part | system | coined | A web application the person already uses, reached through the web pack. |
| Web Crates | group | pack-web | coined | The group in the web view that holds Web Drivers, Web Probe, Browser Engine. |
| Web Drivers | part | web-ui, pack-web | coined | The web pack's crate of drivers over one browser: ui, page_http, and site_tools. |
| Web Helpers | group | web-ui | coined | The group in the Web Drivers view that holds Widget Drivers, Web Screen Models, Browser Sign-In, Expiry and Block Signs. |
| Web Limits | group | pack-web | coined | The group in the web view that holds Web Refusals. |
| Web Locator | part | web-browser | coined | The resolution of a query to exactly one rendered element by role and name. |
| Web Modes | group | web-ui | coined | The group in the Web Drivers view that holds ui, page_http, site_tools. |
| Web Probe | part | web-probe, pack-web | coined | The web pack's crate that says what a web surface is and what is on it, without ever acting on it. |
| Web Refusals | part | pack-web | coined | The things the web pack refuses to do, such as using the everyday browser profile or hiding automation. |
| Web Screen Models | part | web-ui | coined | The web pack's screen models, each a named page with how to reach it, its fields, and how to know it is ready. |
| Webhooks | part | rest-face | coined | The sender that reads the outbox, signs, posts, retries, and records the outcome. |
| Window Identification | part | pack-desktop | coined | The finding of the application's window and the proof, before every act, that the element is in the expected window and process. |
| Working Output | part | project | coined | The .anything2mcp/out folder of snapshots, captures, traces, and diffs, ignored by Git. |
| Write and Proof | group | engine-write | coined | The group in the Write Path view that holds Single Act, Acted Mark, Read-Back, Verification. |
| Write Call Controls | group | policy | coined | The title of the numbered walk through the Policy Engine view, told in 9 steps. |
| Write Checks | group | policy | coined | The group in the Policy Engine view that holds Pre-Approval Rules, Approvals, Policy Decision, Result Masking. |
| Write Gate | group | ledger | coined | The group in the Ledger view that holds Intent Token, Ledger Rules. |
| Write Path | part | engine-write, engine-loop | coined | The stage of a write that takes each record from lock to verified: lock, freshness check, intent, act, read back, compare. |
| Write Sequence | group | engine-loop | coined | The group in the Engine Loop view that holds Preview, Approval Check, Write Path, Reconciliation. |
| Copy Test Harness | part | pages | glossary | The program under `tools/field-harness/`, the product's own serve with the Copy Test Prompt, run in a closed container network in a field test (decision 0310). |
| Copy Test Prompt | part | pages | glossary | The presence prompt of the Copy Test Harness; it passes only the short hash the runner answers on its pipe (decision 0310). |
| Input Files | part | project | glossary | The files in the project's `inputs` folder that a person gives a scout or a pipeline; never compiled (decision 0309). |
| Waiting Reads | part | scout | glossary | The part of the scout's report that lists each read not sent because no one has signed in (decision 0303). |
| Capture Contract | part | contract | a2m-provider, src/capture.rs | The provider contract's capability for traffic capture: a capture source a pack declares, and the one check every yield passes (decision 0391). |
| Person-Driven Discoverer | part | contract | the title of decision 0395 | A discoverer whose manifest says `driven_by = "person"`, called only through `watch` (decision 0395). |
| Body Shape | part | contract | coined (a2m-provider, module shape, not built) | The one closed subset of JSON Schema in the provider contract, the module `shape` of decision 0394. |
| Safe Request Match | part | web-browser | coined | The base's `SafeRequest` list and its match by method, origin, path template, and body (decision 0394). |
| Unavailable Mark | part | mcp-face | coined | What the MCP door's list carries for a tool stopped by a drift: the fixed sentence and `anything2mcp/unavailable` (decision 0396). |
| Capture Job | part | scout | decision 0392 | The job `capture.start` asks for, which its person begins and ends, and whose findings enter the scout at the draft stage (decisions 0392, 0400). |
| Module Map | page | (the page itself) | coined | The middle panel of the page: stations and subsystems drawn as containers with their parts nested inside as modules. |
| The Seam | page | contract | coined | How the Module Map draws the Provider Contract: a band between the kernel and the packs, with the contract's extension points on it. |
| Process Map | page | (the page itself) | coined | One layer or walk drawn alone, its parts laid out along the flow of its arrows and numbered by step. |
| Reading Panel | page | (the page itself) | coined | The right panel of the page, which reads the chosen part in depth. |
