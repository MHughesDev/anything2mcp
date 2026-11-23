# `a2m-pack-device`

The device pack: the `device` driver over a device's link. It sends one frame at a time, identifies the device against its map and the serial its surface declares before anything else is sent, checks every value against its register's range twice before a frame is built, gives the bytes of a write out to be recorded before they are sent, and never sends a write twice. The pack is designed in [spec section 310](../../spec/3-packs/310-the-device-pack.md); this document says how its crate is put together and what is built. What building it settled is in decisions [0106](../../decisions/0106-the-device-pack-speaks-its-first-protocol-itself-and-checks-a-range-before-a-frame-is-built.md), [0107](../../decisions/0107-the-device-mode-and-the-device-pack-as-building-it-settled-it.md), and [0137](../../decisions/0137-a-device-is-held-to-its-serial-a-write-frame-is-recorded-before-it-is-sent-and-a-run-has-a-frame-ceiling.md). Firmware, built last, is settled in [0316](../../decisions/0316-firmware-is-inspected-by-an-optional-provider-and-updated-by-one-operation-through-the-devices-own-sequence.md).

## Facts

| | |
| --- | --- |
| Side | Pack: device |
| Layer | 10, Packs |
| May depend on | Layers 1 to 5. It has no base crate and depends on no other pack crate |
| Depends on | `a2m-core` (`Plan`, `Step`, `Value`, `Decimal`, `Finding`, `Hazard`), `a2m-files` (file kind, lowering, lint, and drafting registration), `a2m-auth` (the lease type in the driver's signature), `a2m-provider` (`Pack`, `Driver`, `Discoverer`, `Describer`, `SandboxPort`), `a2m-telemetry` (the clock a finding is dated with). Outside: `serde`, `serde_json`, and `tokio` with its `io-util` feature. No protocol library ([decision 0106](../../decisions/0106-the-device-pack-speaks-its-first-protocol-itself-and-checks-a-range-before-a-frame-is-built.md)) |
| Roadmap packages | Packages K2, K3, K4 of the [roadmap](../../plan/roadmap.md) |
| Carries out | Spec section [310](../../spec/3-packs/310-the-device-pack.md); [201](../../spec/2-kernel/201-archetypes-and-modes.md) (the device archetype); [213](../../spec/2-kernel/213-effects-hazards-and-consent.md) (hazards); the entry bar of section [202](../../spec/2-kernel/202-kernel-and-packs.md) |

## Owns

- The `device` mode, registered by [decision 0107](../../decisions/0107-the-device-mode-and-the-device-pack-as-building-it-settled-it.md), and the surface kind `device_link`.
- The `registers/` file kind: one map per file, with every address the pack may touch, its layout, its access, its range, and its hazard. It compiles into the pack's own section of the plan.
- The lowering of `read`, `update`, and `submit` into `open`, `build_frame`, `write_frame`, `read_frame`, and `expect` steps, with the pack's own step payload.
- The protocol interface inside the crate, `Link`, and its first implementation: a subset of the Modbus application protocol over TCP, written here.
- Turning a register's raw bits into a typed value and back, with both range checks.
- The `device_identity` finding kind, the discoverer `device_identity`, its describer, and the drafting rule from an identity finding to the start of a map.
- The firmware update: the map's `firmware_update` section (the mechanism `register_blocks`), the `firmware` binding field, and the steps `BuildFirmware`, `Install`, `AfterUpdate`, `Precondition`, and `Checksum` of the step payload ([decision 0316](../../decisions/0316-firmware-is-inspected-by-an-optional-provider-and-updated-by-one-operation-through-the-devices-own-sequence.md)). One step commits and carries the device's whole sequence; the commit command's reply is read by the confirm steps; the read-back waits for the device within the map's window and compares its checksum with the image's.
- The optional provider `device_firmware`, with a manifest of its own (`anything2mcp-firmware-provider.toml`): the discoverer `firmware_image`, its describer, and the finding kind of the same name, at most `inferred`, from the source `artifact`. `add_to` does not register it; `add_firmware_inspection_to` does, and the product crate does not call it yet.
- The `device_register_map` finding kind, the discoverer `device_register_map`, its describer, and the two drafting rules from a published map to a map of readable registers and one read of each.

It must never: send a frame to an address its map does not name; build a frame from a value outside its register's range; round a value on the way to a device; send a write frame from a step that does not commit, or where nothing may be written; send anything more on a link after a frame's fate became unknown; send a write twice; read a register whose reading changes it; scan addresses; read a register during discovery; send a write frame whose bytes no earlier step gave out to be recorded, or other bytes than those; write to a device whose serial its surface does not declare for the run's environment; write a register with the hazard `physical`; send a frame past the run's ceiling. The driver holds the last five by checks of its own, whatever the plan it is handed says.

## What it registers

| Registration | Content |
| --- | --- |
| Provider | `device` |
| Mode | `device`, archetype device, rank 60. One read and one write at a time on a session, an intent window of one, and exclusive use of the link |
| Surface kind | `device_link`, with a session. Fields: `address` (a socket address, such as `192.0.2.7:502`; a host name is refused, because a lookup is a request the plan did not state), `protocol` (one of the crate's `PROTOCOLS`: today `modbus_tcp` alone), `unit` (0 to 255), and `serials` (optional: the serial of the device that answers in each environment, as text, by the environment's name) |
| Step kinds | `open`, `build_frame`, `write_frame`, `read_frame`, `expect` |
| Verbs | `read`, `update`, `submit` |
| File kind | `register`, in `registers/`, for `device_link` surfaces |
| Binding fields | `device_map` (the map's name), `read` (output to register), `registers` (one register to the expression for its value), `read_back` (output to register) |
| Finding kinds | `device_identity`, `device_register_map` |
| Discoverers, describers | `device_identity`, at `discovered`; its describer counts the devices and keeps every word a device said under `untrusted`. `device_register_map`, source `published_description`, at most `inferred`; it sends no frame; its describer names each row it leaves out by its line and why, and keeps the file's own names under `untrusted` |
| Drafting rules | From a `device_identity` finding to a `registers/` file; from a `device_register_map` finding to a `registers/` file and to `operations/` files |
| Lint rules | `device.writable_without_hazard`, `device.writable_without_range`, and `device.command_hazard_none`, errors in every compile mode, as rules that look across files. The rules below are checked where a file is compiled or a binding lowered, under the pack's own codes |
| Effects | Network to the declared surfaces; a session for `device_link`; changes a device |
| Settings | `reply_within_ms` (default 1000) and `min_gap_ms` (default 250, which is four frames a second at most): the pace of a map that states none. `max_frames_per_run` (default 100): the most frames one run may send on one link |

### The map

| Field | Meaning |
| --- | --- |
| `device_map` (required) | The map's name, which a binding names |
| `surface` | A surface of the kind `device_link` |
| `model` (required), `firmware` (required, not empty) | The model and the firmware versions the map was proven against |
| `identify.reads` | Which of `vendor`, `model`, `firmware_version`, and `serial` the `open` step reads. All four when it is left out. It must list `model` and `firmware_version` |
| `pace` | `min_gap` and `reply_within` as `20ms` or `5s`; `max_rate` as `20/s` |
| `registers` (required) | By name: `address` (`table`, one of `coil`, `discrete`, `holding`, `input`, and `offset`), `data` (`bit`, `u16`, `i16`, `u32`, `i32`, `text`), `words` (text only), `word_order` (`high_first` by default, or `low_first`), `scale` (the value of one raw step), `type` (a field type), `access` (`read`, `write`, `read_write`), `range` (`min`, `max`, in the register's unit), `hazard`, `confirm_by`, `read_effect` (`none`, `clears`, `advances`) |
| `safe_state`, `on_link_loss` | Compiled into the plan. Nothing acts on them yet |

### Rules checked when a file is compiled or a binding lowered

| Code | Rule |
| --- | --- |
| `device.map_without_link` | A map is on a surface of the kind `device_link` |
| `device.identify_incomplete` | `identify.reads` lists `model` and `firmware_version` |
| `device.time_malformed`, `device.rate_malformed` | A time is a whole number with `ms` or `s`; a rate is a whole number above zero with `/s` |
| `device.data_not_of_table`, `device.table_read_only` | A `coil` or `discrete` address holds a bit, the other two hold words; a register in a table that can only be read is not writable |
| `device.words_not_needed`, `device.words_missing`, `device.address_past_end` | Only text states `words`, and it must; a register ends inside its table |
| `device.text_not_writable` | A text register is `access: read`. Writing text is not built |
| `device.scale_invalid` | A scale is a positive exact number, on a register that holds a number |
| `device.type_not_of_data` | The field type fits the data and the scale: a bit takes a boolean or a choice; a scaled number a decimal; an unscaled number also an integer or a choice |
| `device.range_invalid` | A range is two exact numbers, least first, each a whole number of the register's steps that its data can hold |
| `device.confirm_by_unknown` | `confirm_by` names a readable register of the map whose `read_effect` is `none` |
| `device.map_not_encodable`, `device.step_not_encodable` | The compiled map and each step encode |
| `device.unknown_map`, `device.unknown_register`, `device.unknown_output` | A binding names a map, registers of that map, and outputs of its operation |
| `device.read_missing`, `device.registers_missing`, `device.field_not_of_verb` | A `read` names what it reads; a write names what it writes; a binding states no field its verb does not take |
| `device.register_not_readable`, `device.read_has_effect` | A register read is readable, and reading it changes nothing. Reading a register whose reading changes it is not built |
| `device.register_not_writable`, `device.one_register_per_write` | A write names one writable register. One frame that writes several registers is not built |
| `device.write_without_serial` | A binding that writes is on a surface that declares the serial of at least one device under `serials`. The driver holds the run's own environment to it |
| `device.writable_without_hazard`, `device.physical_not_built`, `device.hazard_not_declared` | The written register declares a hazard; it is not `physical`, whose writes are not built; a hazard other than `none` needs an operation that is `irreversible` and declares at least that hazard |
| `device.update_without_read_back`, `device.read_back_elsewhere` | An `update` writes a register that can be read without effect, and reads the written register again under `read_back` |
| `device.command_hazard_none` | A `submit` writes no register whose hazard is `none` |

## Interface

```rust
/// The one call the product crate makes.
pub fn add_to(builder: &mut RegistryBuilder);

/// The pack itself, for a caller that builds a registry by hand.
pub struct DevicePack;                  // implements `Pack`
pub const MANIFEST: &str;               // the embedded manifest
pub const PACK: &str;                   // "device"

/// The failure codes, for the crates that match on them.
pub mod codes {
    pub const ALL: &[(FailureClass, &str)];          // every code the manifest declares
    pub const VALUE_OUT_OF_RANGE: FailureCode;       // and one constant for each code below
}
```

Everything else is private. Inside the crate, the protocol interface is the only code that knows frames:

```rust
pub(crate) trait Link: Send {
    fn frame(&mut self, request: &Request) -> Option<Vec<u8>>;
    fn send(&mut self, frame: &[u8], within: Duration) -> BoxFuture<Result<(), SendError>>;
    fn receive(&mut self, within: Duration) -> BoxFuture<Result<Vec<u8>, LinkError>>;
    fn interpret(&self, request: &Request, reply: &[u8]) -> Option<Reply>;
}
pub(crate) const PROTOCOLS: &[&str] = &["modbus_tcp"];
```

A `Request` is one of identify, read bits, read words, write one bit, and write words. `frame` builds the bytes of a request and sends nothing; `send` puts bytes that `frame` built on the link. They are apart so that the bytes of a write exist, and are recorded, before any of them leaves. A frame built is the next frame of the link. A `SendError` says whether no byte left or the fate is unknown; a `LinkError` is a closed link, a time-out, or bytes that are not a reply. The Modbus link carries functions 1 to 6, 16, and 43 with type 14, and the protocol's exception replies. Its transaction identifier counts from one on every link, in the order frames are built, so one run sends the same bytes each time. After any failure to read a reply it drops the stream, because a late reply would be read as the answer to the next request. An identification that does not fit in one reply is not read.

**The compiled section** (format 1) holds every map of the target by name, each with its surface, model, firmware list, identity fields, pace, and registers, and the range of each register both in its unit and as raw numbers. **The step payload** says what one step does: `identify` or `report` for `open`; `build` (with the register, the argument that holds its value, and how many frames the read-back sends after the write) for `build_frame`; `ask_read` or `write` (with the register and the argument that holds its value) for `write_frame`; `reply` for `read_frame`; `value`, `before`, or `acknowledged` for `expect`. Every step refers to its map.

**How a binding lowers.** Every binding starts with `open`.

| Verb | `act` | `confirm` | `read_back` |
| --- | --- | --- | --- |
| `read` | `open`; for each output, `write_frame` a read request, `read_frame`, `expect` the value | None | None |
| `update` | `open`; read the register's present value and keep it for reconcile; `build_frame` the write; `write_frame` it, the one step that commits | `read_frame`; `expect` an acknowledgement | `open`, which sends nothing on a link already identified; read each register under `read_back` |
| `submit` | `open`; `build_frame` the write; `write_frame` it, which commits | `read_frame`; `expect` an acknowledgement | None |

**How a write frame is recorded before it is sent.** `build_frame` commits nothing and sends nothing. It runs every check a write passes, builds the frame, and gives its bytes out: under `frame_<register>` for reconcile, as hexadecimal, and as the capture `frame` with the bytes and their SHA-256 digest. The engine stores what a step before the committing one gives for reconcile durably before it calls the committing step, so the bytes are in the ledger's evidence before the first of them leaves. `write_frame` runs the checks again on the value it is given, and sends the frame that was built for the same register and the same value, and no other: a write frame that no step built, or whose value changed since, is refused unsent. While a built frame waits, no other frame is sent on the link.

Every step is declared not idempotent. A step's time limit is the map's `reply_within`, else the target's default timeout `device_reply`, else 1000 ms. Identity is `natural` or `none`; `composite`, `stamped`, and `find` are refused with a reason.

**What `act` returns.** `open`: nothing, or, in discovery, what the device reported, each value redacted. `build_frame`: the frame, as above. `write_frame` and `read_frame`: nothing. `expect`: the register's value under its output, or under `before_<register>` for reconcile, redacted; or nothing for an acknowledgement. Every `open` that sent a frame and every `expect` keeps one capture, `exchange`: the frame it sent, as hexadecimal, and the length of the reply. Every step that succeeds also returns the capture `acting`: `means` (the kind of step, what it does, the register it concerns, and whether it compared with a recorded frame) and `measured` (the frames the run has sent, and the result of that comparison), so that five runs of one read give one trace signature. A write's bytes are never among the means.

**What a read-back does with the recorded frame.** `Prepared::recorded()` holds `frame_<register>` for the record, within a pass and on a resume. An `expect` of a register's value, when a frame is recorded for it, reads the frame back into the write it carries (`Link::written`; the Modbus link reads functions 5, 6, and 16 back) and sets what the register holds beside what that write would have put there, in the register's own layout. It keeps the result as the capture `frame_check` (`matches`, `differs`, or `unreadable`) and in `measured`. It is evidence: the value is returned as read and the engine compares it with the intended and earlier values, so a device that rounds a value still verifies, and a frame that does not read back is said and does not stop the read. The record has no key from an input: `StepInput::key` is `None`, and no step of this driver reads it; a register is found by its address in the plan.

**How a value is read and written.** A raw number becomes an exact decimal by its scale; a bit becomes a truth value, or the text of its code for a choice; text is read two characters to a word, its trailing zero bytes dropped, and refused if it holds a control character. A value to write must be of the register's kind. It is checked against the map's range in the register's unit, then turned into a raw number, which must be a whole number of steps; the raw number is checked against the range converted by the scale, and against what the register's bits hold. A register that holds a number and has no range is refused here too. A choice on a bit takes the code `0` or `1`.

**What a `device_identity` finding holds.** The fields the device reported, as JSON with its keys in order: each one of `vendor`, `model`, `firmware_version`, and `serial`. Its subject is the device, by its serial, model, and firmware. It is at `discovered`, with partial coverage: the device was asked what it is and nothing else.

**A published register map, as the pack reads it** ([decision 0188](../../decisions/0188-credentials-over-plain-links-drafted-workbooks-and-published-device-maps.md)). A file the discovery job is handed through its checked paths whose name ends in `.csv`, read in this layout, and no other:

| Part | Rule |
| --- | --- |
| Text | UTF-8, a byte order mark allowed; one record a line; cells divided by commas, trimmed, never quoted. A file that holds a quote or a control character, or a row of another width than its header, is not a map |
| Notes | Before the header, a line that starts with `#` is a note. Two are read: `# model: <model>` and `# firmware: <version>, <version>`. Any other note is skipped |
| Header | The first other line names the columns, in any order, each once. Required: `name`, `table` (`input`, `holding`, `coil`, `discrete`), `offset` (a whole number), `data` (`u16`, `i16`, `u32`, `i32`, `text`, `bit`). Optional: `words` (text only, 1 to 125), `word_order` (`high_first`, `low_first`), `scale` (a positive exact number; empty is the raw number), `unit`, `access` (`read`, `read_write`, `write`; empty is `read`). Any other column: not a map |
| Rows | At most 4096. An empty cell states nothing |
| Bounds | A file of at most 1 MiB, whatever the job's bound; a cell or a note part of at most 64 characters, a whole note of at most 128. Past any of them the file is not a map ([decision 0197](../../decisions/0197-no-client-takes-a-proxy-from-the-environment-and-stand-only-protocols.md)) |

The finding is one a file: its subject is the file by digest, and it holds the surface, the file's name, the model and firmware versions the notes state, and each row as stated, with its line. One finding a file and not one a register, because the drafter hands a rule one finding at a time and a map is one document. The map rule drafts `registers/{file}.yaml` on the surface, with the model and the firmware versions the file states (each a question where it states none), `identify.reads: [model, firmware_version]`, and each input or holding register whose data type the file states, with its address, data, words, word order, and scale, and `access: read`: no range, no hazard, no safe state. The read rule drafts `operations/{file}_{register}_get.yaml` for each: `{target}_{register}_get`, one output of the register's name, `untrusted_text` for text, `decimal` for a scaled number, `integer` for a raw one. A map file is hostile input ([decision 0197](../../decisions/0197-no-client-takes-a-proxy-from-the-environment-and-stand-only-protocols.md)): what it says reaches a drafted file only through the YAML writer and only from a closed set of characters. A register's name is spelled from `[a-z0-9_]`, at most 40 characters, and a description repeats it in backticks as the name the map gives. A unit is copied only from ASCII letters and digits and `% / . - ° ² ³ · µ Ω`, at most 16 characters; a model or a firmware version only from ASCII letters, digits, spaces, and `. - _ +`, at most 40. A unit, a model, or a version outside its set is not copied: its question is asked instead, with the file's words quoted apart from the pack's sentence (`open_question_quoting`). A name made from words that read as an instruction still becomes a tool's name; a person reads it when they publish, and the description says the name is provisional. An offset is compared without overflow. A unit the file states is said in the read's description, as the map's; a number whose unit it does not state holds that question as its output's type, and only that read waits on it. A coil, a discrete input, a register that can only be written, a row with no data type the pack knows, an address the protocol cannot hold (an unknown table, an offset that is not a whole number, a register that ends past 65535), and a scale, word count, word order, or access the pack cannot read give no register and no read.

**A register named as a secret is never drafted.** A row of a published map whose name, as the file writes it or as the register and output name it becomes, the compiler takes for a secret's (`names_a_secret` in `a2m-files`) is left out of the drafted map and has no read; the describer says it was left out, "so that a secret is never an output". The drafted reads have no input ([decision 0302](../../decisions/0302-a-field-named-as-a-secret-is-never-drafted-as-an-input-or-an-output.md)).

## Used by, and uses

| Used by | For |
| --- | --- |
| The product crate | `add_to`, once |
| `a2m-engine`, through the handle | The steps of a `device` binding |
| `a2m-files`, through the gathered set | The file kind, the lowering, the lint rules, and the drafting rule |
| `a2m-actions`, through the registry | The discoverer and the describer |

It uses the redactor of its `PackContext` on every value, capture, and finding, and the run's destinations to decide whether the link may be opened. It opens the link itself, over TCP. It uses nothing of the engine, the scheduler, the store, the supervisor, or another pack. The lease it is handed is not used: the first protocol has no sign-in.

## State

A `LinkState` behind `Prepared`, for one run on one link: the link, the plan's maps, the maps the device on this link was compared with, the serial the surface declares for the run's environment, the write frame that was built and waits to be sent, how many more frames the run may send, the frame that waits for its reply, the reply that waits to be read, the time of the last frame, and the posture. When a frame's fate becomes unknown the link is set to none, and nothing more is sent on it in that run. Nothing is kept between runs: release drops the link, and the next run opens its own and identifies the device again.

## Failures

| Class | Code | When | Sent |
| --- | --- | --- | --- |
| `input` | `value_out_of_range` | A value outside its register's range in either check; a register with no range; a value between two steps, or more than the register holds; a choice code that is not `0` or `1` on a bit | Not sent |
| `input` | `schema_invalid` | A value that is absent or not of the register's kind; an address that is not a socket address | Not sent |
| `input` | `payload_unreadable` | The plan's maps or a step's payload do not decode, or were compiled in another format; a step out of order; a write frame that no step built and gave out first, or whose value changed since; a write to a register that declares no hazard | Not sent |
| `policy` | `surface_not_declared` | The surface is not a `device_link`, or its address is not among the run's destinations | Not sent |
| `policy` | `read_only_posture` | A write frame built or sent from a step that does not commit, or in a posture that may not write. The contract's code; the handle refuses the same first | Not sent |
| `policy` | `device_serial_not_declared` | A write where the surface declares no serial for the run's environment | Not sent |
| `policy` | `discovery_identifies_only` | Any frame but identification in the discovery posture | Not sent |
| `policy` | `frame_budget_exceeded` | A frame past the run's ceiling; a write whose frame and read-back the frames left would not cover | Not sent |
| `unknown_variant` | `device_identity_mismatch` | A field under `identify` is not reported, the model differs, the firmware version is not in the map's list, or the serial is not the one the surface declares for the run's environment; a register step on a map the device was not compared with; in discovery, a device that reports no model or firmware version | Answered; not sent |
| `unknown_variant` | `physical_not_built` | A step that writes a register with the hazard `physical`. The compiler refuses such a binding first | Not sent |
| `unknown_variant` | `unexpected_reply` | A read, or the identification, answered in no declared form | Answered |
| `rejected` | `device_refused` | An exception: function, address, or value, or an unexplained one on a read | Answered |
| `transient` | `device_busy` | The device answered that it is busy, on a read or a write. A write is not sent again by itself | Answered |
| `transient` | `timeout`, `connection_lost`, `frame_corrupt` | On a read: no reply in time, the link closed, a damaged reply. Before any frame: the device did not accept the link in time, or refused it. A gateway that answers that the device behind it did not is `timeout` | Unknown on a read; answered for a gateway; not sent before any frame |
| `possible_commit` | `no_confirmation` | A write frame left and the link failed while it was sent, closed, timed out, or gave a damaged reply; the reply acknowledges something else; or the device answered with a failure that does not say whether it applied the write | Unknown |

`session_in_use`, `approval_missing`, and `read_back_differs` are not raised here: the scheduler, the policy engine, and the engine's comparison raise them. `sensor_out_of_tolerance`, `device_not_returned`, `device_protected`, and `image_protected` are not raised, and the manifest does not declare them.

## Invariants

| Invariant | Proven by |
| --- | --- |
| A registry with only this pack builds, its digest is stable, and what it declares is what it registers | `a_registry_with_only_the_device_pack_builds_and_its_digest_is_stable`, in `tests/registry.rs` |
| The manifest declares exactly the codes the pack raises, each with its one class, and the defaults of the pace and the frame ceiling | `every_failure_code_of_the_pack_is_declared_with_its_one_class`, in `tests/registry.rs` |
| The example target compiles twice, and from disk, to the same bytes | `the_example_target_compiles_twice_to_the_same_bytes`, in `tests/compile.rs` |
| An unknown key or word in a map or a surface is refused | `an_unknown_key_or_word_in_a_map_or_a_surface_is_refused`, in `tests/compile.rs` |
| A writable register without a hazard or a range does not compile, nor does a command with the hazard `none`, a bad range, a table that does not hold its data, or a bad time | `a_writable_register_without_a_hazard_or_a_range_does_not_compile`, in `tests/compile.rs` |
| A binding names registers of its map, reads the written register back, reads no register whose reading changes it, and declares what it could harm; a `physical` write does not compile | `a_binding_names_registers_of_its_map_and_declares_what_it_could_harm`, in `tests/compile.rs` |
| A raw number becomes a decimal by its scale exactly; a value outside its range, or between two steps, gives no request | `a_raw_number_becomes_a_decimal_by_its_scale_exactly`, `a_value_outside_its_range_gives_no_request`, and `signed_and_two_word_values_are_laid_out_by_the_map`, in `src/codec.rs` |
| A frame and a reply are read only in the form the protocol declares | `a_request_is_framed_as_the_standard_writes_it`, `a_reply_is_read_only_in_the_form_declared_for_its_request`, `identification_is_read_object_by_object`, in `src/modbus.rs` |
| A read identifies the device first and sends only read frames | `a_read_identifies_the_device_and_sends_only_read_frames`, in `tests/reads.rs`, against the stand's frame log |
| Five runs of one read give one trace signature | `five_runs_of_one_read_give_one_trace_signature`, in `tests/reads.rs`, through the handle |
| A firmware version outside the map stops a read | `a_firmware_version_outside_the_map_stops_a_read`, in `tests/reads.rs` |
| A register step on a map the device was not compared with sends nothing | `a_step_that_reads_without_identifying_first_sends_nothing`, in `tests/reads.rs` |
| A link that is not declared is never opened | `a_link_that_is_not_declared_is_never_opened`, in `tests/reads.rs` |
| What the device does to a read is named | `what_the_device_does_to_a_read_is_named`, in `tests/reads.rs` |
| A secret in what the device says reaches no value and no failure | `a_secret_in_what_the_device_says_reaches_no_value_and_no_failure`, in `tests/reads.rs` |
| A write lands once, by the stand's count; the value before it is kept for reconcile; a value the device rounds verifies after normalization | `a_write_lands_once_and_a_value_the_device_rounds_still_verifies`, in `tests/writes.rs` |
| An acknowledgement is not a read-back: a value the device changes reads back different | `a_value_the_device_changes_is_a_different_value_on_read_back`, in `tests/writes.rs` |
| A value outside its range, or of another kind, sends no frame | `a_value_outside_its_range_sends_no_frame`, in `tests/writes.rs` |
| A link lost after a write frame, no reply, or a wrong echo is `possible_commit`; nothing is sent again; reconcile identifies the device again and reads the register | `a_link_lost_after_a_write_frame_is_a_possible_commit_and_nothing_is_sent_again`, in `tests/writes.rs` |
| A refusal and a busy device are named, and a write is never sent twice | `a_refusal_and_a_busy_device_are_named_and_a_write_is_never_sent_twice`, in `tests/writes.rs` |
| A write frame needs the ledger's token and a posture that may write | `a_write_frame_needs_the_ledgers_token_and_a_posture_that_may_write`, in `tests/writes.rs` |
| In the posture that writes a declared test record, a write frame not marked `commits` is refused unsent and the device holds what it held; the marked one writes once | `a_write_frame_not_marked_as_the_commit_point_is_refused_where_test_records_may_be_written`, in `tests/writes.rs` |
| An update never compiles as a read, nor a command as a plain write | `a_write_never_compiles_as_a_read_and_a_command_never_as_a_plain_write`, in `tests/compile.rs` |
| A drafting rule that drafts a writable register is refused, and its file is not written | `a_drafting_rule_that_drafts_a_writable_register_is_refused`, in `src/map.rs` |
| A command is sent once and has nothing to read back | `a_command_is_sent_once_and_has_nothing_to_read_back`, in `tests/writes.rs` |
| A device that reports another serial than its surface declares is refused, for reads and writes; where none is declared a read goes on and no write frame is built | `a_device_with_another_serial_is_refused_and_nothing_is_written_where_none_is_declared`, in `tests/writes.rs` |
| The bytes of a write are given out for reconcile by a step before the one that commits; the frame the stand receives is those bytes; a frame that was not built first, or whose value changed, is not sent | `the_bytes_of_a_write_are_recorded_before_the_frame_is_sent`, in `tests/writes.rs` |
| A hand-built step that writes a `physical` register sends nothing | `a_hand_built_step_that_writes_a_physical_register_is_refused_by_the_driver`, in `tests/writes.rs` |
| A write is not started when the frames left would not cover it and its read-back | `a_write_is_not_started_when_the_frames_left_would_not_cover_its_read_back`, in `tests/writes.rs` |
| A run stops at its frame ceiling: one hundred frames by default, by the stand's count | `a_run_stops_at_its_frame_ceiling`, in `tests/reads.rs` |
| The driver in the discovery posture sends nothing but identification | `a_driver_in_the_discovery_posture_sends_nothing_but_identification`, in `tests/reads.rs` |
| A map with no pace sends four frames a second at most; a map's own pace is the larger of its gap and what its rate allows | `a_map_with_no_pace_sends_four_frames_a_second_at_most`, in `src/driver.rs`, on a clock the test controls |
| Discovery sends one identification frame and changes nothing; the draft holds only what the device stated | `discovery_identifies_the_device_and_changes_nothing`, in `tests/discovery.rs` |
| An update runs the device's own sequence once, commits once, and is read back after the restart: version, and the checksum the image carries | `an_update_goes_through_the_devices_own_sequence_and_is_read_back_after_the_restart`, in `tests/firmware.rs`, by the stand's frames and update count |
| An image that is damaged, of another version, too large, off the list of digests, or not hexadecimal sends no write frame; a precondition that does not hold stops the update before anything is written | `an_image_that_will_not_be_sent_sends_no_write_frame`, `an_image_off_the_operations_list_of_digests_is_not_sent`, and `an_update_is_not_started_while_a_precondition_does_not_hold`, in `tests/firmware.rs` |
| A refused image ends refused and the device is told to leave update mode; a lost link at the commit, or a device that does not come back, is `possible_commit` and nothing is sent again | `a_device_that_refuses_the_image_is_told_to_leave_update_mode_and_the_run_ends_refused`, `a_link_lost_at_the_commit_is_a_possible_commit_and_nothing_is_sent_again`, and `a_device_that_does_not_come_back_is_a_possible_commit_and_nothing_is_retried`, in `tests/firmware.rs` |
| An update without a recovery route, not irreversible with the hazard `device_function`, or on a map with no mechanism does not compile | `an_update_without_a_declared_recovery_route_does_not_compile` and `an_update_is_irreversible_with_the_hazard_device_function_and_needs_a_mechanism`, in `tests/firmware.rs` |
| The firmware provider is off unless added; an image given as a file is read as `inferred`, nothing is sent, a secret-named value is withheld, and a protected image is a named stop | `the_provider_is_off_unless_it_is_added_and_declares_what_it_raises`, `an_image_given_as_a_file_is_read_and_described_as_inferred_and_nothing_is_sent`, and `an_image_of_another_form_or_under_another_name_is_not_read_and_a_protected_one_stops`, in `tests/inspection.rs` |

The tests ask [the simulated device](../../../targets/example-device/README.md) what it stored, how often each register was written, and which frames it received.

## Build order

1. Done: the manifest and registration; the `registers/` schema, its rules, and the `device_link` surface kind. `scale` is on the register, not in field types.
2. Done: identification and `read`, with five runs giving one signature through the handle, and a firmware version outside the map stopping.
3. Done: the discoverer, its describer, and the drafting rule, for identification only.
4. Done: `update` and `submit` of registers with the hazard `none` or `device_function`, with the fault tests: a dropped reply, no reply, a wrong echo, a refusal, busy, out of range, a link lost after a write.
5. Not built: writes with the hazard `physical`. The compiler refuses them with `device.physical_not_built`, and the driver refuses a step that would send one with `physical_not_built`. `confirm_by`, `safe_state`, and `on_link_loss` compile into the plan and nothing acts on them. The safe-state operation, the alert that offers it, and the kill switch reaching the link are not built.
6. Not built: firmware inspection and the firmware update operation.
7. Not built: one frame that writes several registers; reading a register whose reading changes it; writing text; a float register; an object dictionary; an identification spread over several replies.
8. Not built: serial links, a console on a link, and any second protocol.
9. Not built: reconcile against the safe state for a device that reverts on link loss. The value before a write is kept; the comparison is not written.
3a. Done: the discoverer `device_register_map`, its describer, and the two drafting rules from a published map, in `src/published.rs` ([decision 0188](../../decisions/0188-credentials-over-plain-links-drafted-workbooks-and-published-device-maps.md)). Proven against the simulated device by `a_published_map_drafts_reads_that_read_the_stand_and_send_no_frame_to_draft`, in `tests/discovery.rs`: no frame is sent to draft; the finding is `inferred`, `published_description`; the drafted reads compile without the one that holds the unit question; two drafted reads return what the stand holds through the handle, with only read frames sent and no register written; a device whose firmware is outside the drafted map stops the read; the rows with no data type and with an address the protocol cannot hold are named by line. The layout's parser and the rows it leaves out are also held by the unit test `a_map_is_read_in_its_layout_and_each_row_is_judged_by_name`. The hostile-input rules ([decision 0197](../../decisions/0197-no-client-takes-a-proxy-from-the-environment-and-stand-only-protocols.md)) by `a_hostile_published_map_reaches_the_draft_only_as_closed_words_and_quoted_questions` (`tests/discovery.rs`) and the unit tests `an_offset_at_the_top_of_the_number_range_is_an_address_the_protocol_cannot_hold` and `a_long_cell_is_no_map_and_only_a_closed_set_of_characters_is_copied`. Not built: a map given in another layout, objects of an object dictionary, a unit carried in a field type.
10. Done: a ceiling on the frames one run sends on one link, the pack's own, from the setting `max_frames_per_run`. The provider contract gives a step a deadline and no count, so the ceiling is counted on what `prepare` acquired; a run that prepares a link twice starts a second count.
11. Done: the serial the surface declares is compared in `open`; the frame of a write is built and given out by `build_frame` before `write_frame` sends it; the driver refuses a register read in the discovery posture and a write to a `physical` register ([decision 0137](../../decisions/0137-a-device-is-held-to-its-serial-a-write-frame-is-recorded-before-it-is-sent-and-a-run-has-a-frame-ceiling.md)).
12. Done: what the kernel holds the pack to after the safety audit ([decision 0155](../../decisions/0155-four-packs-state-what-saves-what-writes-and-when-their-guard-lifts.md)). `Lowering::commits`: `write_frame` is `AsDeclared`, because the one kind sends both the frame that writes a register and the frame that asks for one, and the lowering marks only the write; `open`, `build_frame`, `read_frame`, and `expect` are `Never`. So the manifest lists no `commit_steps`, and the driver refuses a write frame not marked `commits` in every posture. `Lowering::least_effect`: `submit` is `irreversible`, `update` is `write`, `read` is `read`; a register's hazard asks for more by `device.hazard_not_declared`. `FileKind::writes`: a map describes a write when any register is not plainly `access: read`.
12. Not proven here: the engine loop over the steps of a write, and the scheduler's exclusive lock on the link. The tests call the handle directly. That the engine stores what `build_frame` gives for reconcile before it calls the committing step is the engine's rule, proven in the engine's own tests, and has not been run with this mode.
13. Done: the second pass of the provider contract ([decision 0175](../../decisions/0175-the-data-and-device-packs-take-up-the-second-pass-of-the-provider-contract.md)): step inputs built by the constructor, the `acting` capture on every step, and the read of a register set beside the recorded frame. Proven by `five_runs_of_one_read_give_one_trace_signature`, `a_resumed_write_is_read_against_the_frame_that_was_recorded`, and the unit test `a_frame_that_was_built_for_a_write_reads_back_into_the_same_write`. Not taken up: `with_said`, because a device's refusal is a code and the pack names it itself; `Lowering::evidence` states `acknowledged` for a `submit` whose lowered read-back is empty, and `read_back` for everything else (decision 0175).
14. Not proven here: Windows and macOS. The tests have run on Linux only.

## Open points

- A read goes on where the surface declares no serial for the run's environment: the device is then held to its model and firmware alone. Stopping reads too would be stricter. It was left because a read moves nothing and the compiler cannot see which environments a target will run in.
- The unit address and the protocol are one for every environment of a surface, while the serial is declared for each. An environment whose device answers under another unit address needs a surface of its own.
- The frame a step keeps as a capture is evidence only as far as the engine keeps captures. What is durable before a write is what `build_frame` gives for reconcile.
- The frame ceiling is counted for each link `prepare` opens. The contract gives a driver no count for the whole run.
- An operation that writes a register with the hazard `none` is declared `irreversible` in the example target, because the kernel takes a record key only from an input and a register is found by its name in the map. The read-back still runs.
- A choice register is held, and written in `safe_state.holds`, as the text of its stored code.
- The identification frame keeps the pack's default gap, not the map's; it is the first frame on a link, so no gap applies to it today.
- What the kernel could not give the pack, an identity precondition, a record key named by the pack, a range on field types, and the rest, is listed in [decision 0107](../../decisions/0107-the-device-mode-and-the-device-pack-as-building-it-settled-it.md).
