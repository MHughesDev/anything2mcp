//! The `registers/` file kind: one file per device model and map.
//!
//! A map holds every address the pack may touch, how each is encoded, whether
//! it may be written, its safe range, and its hazard. An operation names a
//! register, never an address. This module holds the schema of a map, its
//! compilation into the compiled target's section, and the lint rules that
//! refuse a writable register with no hazard or no range.

use std::collections::BTreeMap;
use std::sync::Arc;

use a2m_core::{
    Decimal, FieldBase, FileKindName, Hazard, LintCode, PackPayload, Rounding, SurfaceKind,
    ValueType,
};
use a2m_files::{
    CompileMode, CompiledSection, DefinedName, Diagnostic, Diagnostics, Doc, ExprSite, FileKind,
    JsonSchema, KindContext, Level, LintRule, Located, Place, ResolvedView, Stage,
};
use serde::{Deserialize, Serialize};

use crate::names::{Names, lint_code};

/// The version of the section's shape.
pub(crate) const SECTION_FORMAT: u16 = 1;

/// The words a map may list under `identify.reads`.
pub(crate) const IDENTITY_FIELDS: &[&str] = &["vendor", "model", "firmware_version", "serial"];

/// Which table of the device an address is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Table {
    /// Bits that can be read and written.
    Coil,
    /// Bits that can only be read.
    Discrete,
    /// Words that can be read and written.
    Holding,
    /// Words that can only be read.
    Input,
}

impl Table {
    pub const NAMES: &'static [&'static str] = &["coil", "discrete", "holding", "input"];

    /// Whether the table holds bits.
    pub const fn holds_bits(self) -> bool {
        matches!(self, Self::Coil | Self::Discrete)
    }

    /// Whether the protocol can write to the table at all.
    pub const fn writable(self) -> bool {
        matches!(self, Self::Coil | Self::Holding)
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Coil => "coil",
            Self::Discrete => "discrete",
            Self::Holding => "holding",
            Self::Input => "input",
        }
    }
}

/// How a value's bits are laid out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Data {
    /// One bit.
    Bit,
    /// One word, unsigned.
    U16,
    /// One word, signed.
    I16,
    /// Two words, unsigned.
    U32,
    /// Two words, signed.
    I32,
    /// Text, two characters to a word, the first in the high byte.
    Text,
}

impl Data {
    pub const NAMES: &'static [&'static str] = &["bit", "u16", "i16", "u32", "i32", "text"];

    /// The least and the greatest raw number the layout holds.
    pub const fn bounds(self) -> Option<(i64, i64)> {
        match self {
            Self::Bit | Self::Text => None,
            Self::U16 => Some((0, 0xFFFF)),
            Self::I16 => Some((-0x8000, 0x7FFF)),
            Self::U32 => Some((0, 0xFFFF_FFFF)),
            Self::I32 => Some((-0x8000_0000, 0x7FFF_FFFF)),
        }
    }

    /// How many words the layout takes, where that is fixed.
    pub const fn words(self) -> Option<u16> {
        match self {
            Self::Bit | Self::U16 | Self::I16 => Some(1),
            Self::U32 | Self::I32 => Some(2),
            Self::Text => None,
        }
    }
}

/// Which word of a two-word value comes first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WordOrder {
    /// The high word first.
    #[default]
    HighFirst,
    /// The low word first.
    LowFirst,
}

/// Whether a register may be read, written, or both.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Access {
    /// Read only.
    Read,
    /// Write only: a command, with nothing to read.
    Write,
    /// Both.
    ReadWrite,
}

impl Access {
    pub const NAMES: &'static [&'static str] = &["read", "write", "read_write"];

    pub const fn readable(self) -> bool {
        matches!(self, Self::Read | Self::ReadWrite)
    }

    pub const fn writable(self) -> bool {
        matches!(self, Self::Write | Self::ReadWrite)
    }
}

/// What reading a register does to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReadEffect {
    /// Nothing.
    #[default]
    None,
    /// Reading resets it.
    Clears,
    /// Reading moves it on.
    Advances,
}

/// The kind of value a register gives a caller and takes from one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ValueKind {
    /// True or false.
    Bool,
    /// A whole number: the raw value.
    Int,
    /// An exact number: the raw value times the scale.
    Decimal,
    /// A choice, held as the text of its stored code: the raw number.
    Code,
    /// Text.
    Text,
}

/// The safe range of a writable register, in the register's unit and as raw
/// numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CompiledRange {
    pub min: Decimal,
    pub max: Decimal,
    pub raw_min: i64,
    pub raw_max: i64,
}

/// A sensor register that shows the world followed a write. Declared data:
/// nothing acts on it yet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ConfirmBy {
    pub register: String,
    pub within: Decimal,
    pub settle_ms: u32,
}

/// One register of a map, compiled.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CompiledRegister {
    pub table: Table,
    pub offset: u16,
    pub data: Data,
    pub words: u16,
    pub word_order: WordOrder,
    pub scale: Option<Decimal>,
    pub kind: ValueKind,
    pub access: Access,
    pub range: Option<CompiledRange>,
    pub hazard: Option<Hazard>,
    pub read_effect: ReadEffect,
    pub confirm_by: Option<ConfirmBy>,
}

/// How fast a link may be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Pace {
    pub min_gap_ms: Option<u32>,
    pub reply_within_ms: Option<u32>,
    pub max_per_second: Option<u32>,
}

/// What safe means for a device. Declared data: nothing acts on it yet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SafeState {
    pub operation: String,
    pub holds: BTreeMap<String, String>,
}

/// What the device itself does when its link drops. Declared data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OnLinkLoss {
    pub device_reverts_to_safe_state: bool,
    pub after_ms: Option<u32>,
}

/// The device's own update mechanism, as a map declares it: the addresses
/// the sequence writes (which no register of the map names, so that no other
/// operation can write them), and the registers it reads. The mechanism is
/// `register_blocks`: one image length, one command register, and blocks
/// written to one block address (spec section 310, Firmware).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CompiledUpdate {
    /// The holding offset the update commands are written to, alone.
    pub command: u16,
    /// The holding offset the image's length in bytes is written to.
    pub length: u16,
    /// The holding offset the blocks are written to, one block a frame.
    pub block: u16,
    /// The most image bytes one block carries.
    pub block_bytes: u16,
    /// The largest image the device takes, in bytes.
    pub max_image_bytes: u32,
    /// The register that reports the update's state: 0 idle, 1 receiving,
    /// 2 verified, 3 failed.
    pub state: String,
    /// The register that says why the last update step was refused.
    pub error: String,
    /// The register that holds the device's own checksum of its image: the
    /// CRC-32 of the image.
    pub checksum: String,
    /// How long the device has to come back after its commit, in
    /// milliseconds.
    pub restart_within_ms: u32,
}

/// One map, compiled: an entry of the compiled target's section.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CompiledMap {
    pub surface: String,
    pub model: String,
    pub firmware: Vec<String>,
    pub identify: Vec<String>,
    pub pace: Pace,
    pub registers: BTreeMap<String, CompiledRegister>,
    pub safe_state: Option<SafeState>,
    pub on_link_loss: Option<OnLinkLoss>,
    /// The device's own update mechanism, where the map declares one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub firmware_update: Option<CompiledUpdate>,
}

/// The compiled target's section: every map of the target, by name.
pub(crate) type MapSection = BTreeMap<String, CompiledMap>;

/// The schema of one map.
pub(crate) fn schema() -> JsonSchema {
    let register = JsonSchema::object()
        .required(
            "address",
            JsonSchema::object()
                .required("table", JsonSchema::words(Table::NAMES))
                .required("offset", JsonSchema::int_between(0, 0xFFFF)),
        )
        .required("data", JsonSchema::words(Data::NAMES))
        .optional("words", JsonSchema::int_between(1, 125))
        .optional(
            "word_order",
            JsonSchema::words(&["high_first", "low_first"]),
        )
        .optional("scale", JsonSchema::decimal())
        .optional("type", JsonSchema::field_type())
        .required("access", JsonSchema::words(Access::NAMES))
        .optional(
            "range",
            JsonSchema::object()
                .required("min", JsonSchema::decimal())
                .required("max", JsonSchema::decimal()),
        )
        .optional("hazard", JsonSchema::words(Hazard::NAMES))
        .optional(
            "confirm_by",
            JsonSchema::object()
                .required("register", JsonSchema::name())
                .required("within", JsonSchema::decimal())
                .required("settle", JsonSchema::text()),
        )
        .optional(
            "read_effect",
            JsonSchema::words(&["none", "clears", "advances"]),
        );
    JsonSchema::object()
        .required("device_map", JsonSchema::name())
        .optional("surface", JsonSchema::surface())
        .required("model", JsonSchema::text())
        .required("firmware", JsonSchema::non_empty_list(JsonSchema::text()))
        .optional(
            "identify",
            JsonSchema::object().required(
                "reads",
                JsonSchema::non_empty_list(JsonSchema::words(IDENTITY_FIELDS)),
            ),
        )
        .optional(
            "pace",
            JsonSchema::object()
                .optional("min_gap", JsonSchema::text())
                .optional("reply_within", JsonSchema::text())
                .optional("max_rate", JsonSchema::text()),
        )
        .required("registers", JsonSchema::map_of(register))
        .optional(
            "safe_state",
            JsonSchema::object()
                .required("operation", JsonSchema::name())
                .optional("holds", JsonSchema::map_of(JsonSchema::scalar())),
        )
        .optional(
            "on_link_loss",
            JsonSchema::object()
                .required("device_reverts_to_safe_state", JsonSchema::bool())
                .optional("after", JsonSchema::text()),
        )
        .optional(
            "firmware_update",
            JsonSchema::object()
                .required("mechanism", JsonSchema::words(&["register_blocks"]))
                .required("command", JsonSchema::int_between(0, 0xFFFF))
                .required("length", JsonSchema::int_between(0, 0xFFFF))
                .required("block", JsonSchema::int_between(0, 0xFFFF))
                .required("block_bytes", JsonSchema::int_between(1, 240))
                .required("max_image_bytes", JsonSchema::int_between(1, 0xFFFF))
                .required("state", JsonSchema::name())
                .required("error", JsonSchema::name())
                .required("checksum", JsonSchema::name())
                .required("restart_within", JsonSchema::text()),
        )
}

/// A length of time written as `20ms` or `5s`, in milliseconds.
pub(crate) fn duration_ms(text: &str) -> Option<u32> {
    let (digits, factor) = match text.strip_suffix("ms") {
        Some(digits) => (digits, 1),
        None => (text.strip_suffix('s')?, 1000),
    };
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    digits.parse::<u32>().ok()?.checked_mul(factor)
}

/// A rate written as `20/s`, in frames per second.
fn per_second(text: &str) -> Option<u32> {
    let digits = text.strip_suffix("/s")?;
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    digits.parse::<u32>().ok().filter(|rate| *rate > 0)
}

/// An exact number written as a whole number or as text.
fn decimal_of(doc: &Located<Doc>) -> Option<Decimal> {
    match &doc.value {
        Doc::Int(number) => Some(Decimal::from_i64(*number)),
        Doc::Text(text) => Decimal::parse(text).ok(),
        _ => None,
    }
}

/// A scalar as the text a person wrote.
pub(crate) fn scalar_text(doc: &Located<Doc>) -> String {
    match &doc.value {
        Doc::Text(text) => text.clone(),
        Doc::Int(number) => number.to_string(),
        Doc::Bool(flag) => flag.to_string(),
        _ => String::new(),
    }
}

/// The raw number a value in the register's unit stands for, when it is a
/// whole number of the register's steps.
pub(crate) fn raw_of(value: &Decimal, scale: Option<&Decimal>) -> Option<i64> {
    let raw = match scale {
        None => *value,
        Some(scale) => {
            let raw = value.checked_div(scale, 0, Rounding::HalfEven)?;
            if raw.checked_mul(scale)? != *value {
                return None;
            }
            raw
        }
    };
    raw.to_i64()
}

struct Compiler<'a, 'b> {
    names: &'a Names,
    cx: &'a KindContext<'b>,
    problems: Diagnostics,
}

impl Compiler<'_, '_> {
    fn problem(&mut self, rule: &str, at: &Place, message: String, fix: &str) {
        if let Ok(code) = lint_code(rule) {
            self.problems
                .push(Diagnostic::error(code, Stage::Resolve, at, message, fix));
        }
    }

    fn time(&mut self, doc: Option<&Located<Doc>>) -> Option<u32> {
        let doc = doc?;
        let parsed = doc.as_text().and_then(duration_ms);
        if parsed.is_none() {
            self.problem(
                "time_malformed",
                &doc.at,
                "This is not a length of time.".to_owned(),
                "Write a whole number and a unit, such as `20ms` or `5s`.",
            );
        }
        parsed
    }

    fn map(&mut self, doc: &Located<Doc>) -> Option<(String, CompiledMap)> {
        let name = doc.text("device_map")?.to_owned();
        let surface = match self.cx.surface_of(doc) {
            Some(surface) if *surface.kind == self.names.surface_kind => {
                surface.name.as_str().to_owned()
            }
            _ => {
                let at = doc.get("surface").map_or(&doc.at, |surface| &surface.at);
                self.problem(
                    "map_without_link",
                    at,
                    format!("The map `{name}` is not on a surface of the kind `device_link`."),
                    "Name a surface of the kind `device_link` under `surface:`.",
                );
                return None;
            }
        };
        let identify = doc
            .get("identify")
            .and_then(|identify| identify.get("reads"))
            .map_or_else(
                || {
                    IDENTITY_FIELDS
                        .iter()
                        .map(|field| (*field).to_owned())
                        .collect()
                },
                |reads| {
                    reads
                        .items()
                        .iter()
                        .filter_map(|item| item.as_text().map(str::to_owned))
                        .collect::<Vec<String>>()
                },
            );
        for needed in ["model", "firmware_version"] {
            if !identify.iter().any(|field| field == needed) {
                let at = doc.get("identify").map_or(&doc.at, |identify| &identify.at);
                self.problem(
                    "identify_incomplete",
                    at,
                    format!("The map `{name}` does not read `{needed}` to identify the device."),
                    "List `model` and `firmware_version` under `identify.reads`.",
                );
            }
        }
        let pace_doc = doc.get("pace");
        let pace = Pace {
            min_gap_ms: self.time(pace_doc.and_then(|pace| pace.get("min_gap"))),
            reply_within_ms: self.time(pace_doc.and_then(|pace| pace.get("reply_within"))),
            max_per_second: pace_doc
                .and_then(|pace| pace.get("max_rate"))
                .and_then(|rate| {
                    let parsed = rate.as_text().and_then(per_second);
                    if parsed.is_none() {
                        self.problem(
                            "rate_malformed",
                            &rate.at,
                            "This is not a rate.".to_owned(),
                            "Write frames per second, such as `20/s`.",
                        );
                    }
                    parsed
                }),
        };
        let mut registers = BTreeMap::new();
        let entries = doc
            .get("registers")
            .map(Located::entries)
            .unwrap_or_default();
        for entry in entries {
            if let Some(register) = self.register(&entry.key.value, &entry.value) {
                registers.insert(entry.key.value.clone(), register);
            }
        }
        for entry in entries {
            let Some(confirm) = entry.value.get("confirm_by") else {
                continue;
            };
            let sensor = confirm.text("register").unwrap_or_default();
            let readable = registers.get(sensor).is_some_and(|sensor| {
                sensor.access.readable() && sensor.read_effect == ReadEffect::None
            });
            if !readable {
                self.problem(
                    "confirm_by_unknown",
                    &confirm.at,
                    format!("`confirm_by` names `{sensor}`, which is not a register of this map that can be read without effect."),
                    "Name a readable register of this map whose `read_effect` is `none`.",
                );
            }
        }
        let safe_state = doc.get("safe_state").map(|safe| SafeState {
            operation: safe.text("operation").unwrap_or_default().to_owned(),
            holds: safe
                .get("holds")
                .map(Located::entries)
                .unwrap_or_default()
                .iter()
                .map(|entry| (entry.key.value.clone(), scalar_text(&entry.value)))
                .collect(),
        });
        let on_link_loss = doc.get("on_link_loss").map(|loss| OnLinkLoss {
            device_reverts_to_safe_state: loss
                .get("device_reverts_to_safe_state")
                .and_then(Located::as_bool)
                .unwrap_or(false),
            after_ms: self.time(loss.get("after")),
        });
        let firmware_update = doc
            .get("firmware_update")
            .and_then(|update| self.update(update, &registers));
        Some((
            name,
            CompiledMap {
                surface,
                model: doc.text("model").unwrap_or_default().to_owned(),
                firmware: doc
                    .get("firmware")
                    .map(Located::items)
                    .unwrap_or_default()
                    .iter()
                    .filter_map(|item| item.as_text().map(str::to_owned))
                    .collect(),
                identify,
                pace,
                registers,
                safe_state,
                on_link_loss,
                firmware_update,
            },
        ))
    }

    /// The update mechanism a map declares, once every register it reads
    /// is one the map holds and can be read without effect.
    fn update(
        &mut self,
        doc: &Located<Doc>,
        registers: &BTreeMap<String, CompiledRegister>,
    ) -> Option<CompiledUpdate> {
        let number = |key: &str| doc.get(key).and_then(Located::as_int);
        let restart_within_ms = self.time(doc.get("restart_within"))?;
        let mut fine = true;
        let mut register = |this: &mut Self, key: &str, words: u16| -> String {
            let name = doc.text(key).unwrap_or_default();
            let ok = registers.get(name).is_some_and(|found| {
                found.access.readable()
                    && found.read_effect == ReadEffect::None
                    && found.table == Table::Input
                    && found.words == words
                    && found.kind != ValueKind::Text
            });
            if !ok {
                this.problem(
                    "firmware_update_invalid",
                    doc.get(key).map_or(&doc.at, |found| &found.at),
                    format!("`{key}` names `{name}`, which is not a number in an `input` register of this map that can be read without effect, of {words} word(s)."),
                    "Name a register of this map that holds the update's state, its error, or the device's checksum, in the table `input`.",
                );
                fine = false;
            }
            name.to_owned()
        };
        let state = register(self, "state", 1);
        let error = register(self, "error", 1);
        let checksum = register(self, "checksum", 2);
        if !fine {
            return None;
        }
        Some(CompiledUpdate {
            command: u16::try_from(number("command")?).ok()?,
            length: u16::try_from(number("length")?).ok()?,
            block: u16::try_from(number("block")?).ok()?,
            block_bytes: u16::try_from(number("block_bytes")?).ok()?,
            max_image_bytes: u32::try_from(number("max_image_bytes")?).ok()?,
            state,
            error,
            checksum,
            restart_within_ms,
        })
    }

    #[allow(clippy::too_many_lines)] // one check after another on one register; splitting hides the order
    fn register(&mut self, name: &str, doc: &Located<Doc>) -> Option<CompiledRegister> {
        let address = doc.get("address")?;
        let table: Table = parse_word(address.text("table")?)?;
        let offset = u16::try_from(address.get("offset")?.as_int()?).ok()?;
        let data: Data = parse_word(doc.text("data")?)?;
        let access: Access = parse_word(doc.text("access")?)?;
        let mut fine = true;
        if table.holds_bits() != (data == Data::Bit) {
            self.problem(
                "data_not_of_table",
                doc.get("data").map_or(&doc.at, |data| &data.at),
                format!("The register `{name}` is in the table `{}`, which does not hold `{}` values.", table.as_str(), doc.text("data").unwrap_or_default()),
                "A `coil` or `discrete` address holds `bit`; a `holding` or `input` address holds words.",
            );
            fine = false;
        }
        if access.writable() && !table.writable() {
            self.problem(
                "table_read_only",
                doc.get("access").map_or(&doc.at, |access| &access.at),
                format!("The register `{name}` is declared writable, and the table `{}` can only be read.", table.as_str()),
                "Declare `access: read`, or correct the table.",
            );
            fine = false;
        }
        let words = match (data.words(), doc.get("words")) {
            (Some(words), None) => words,
            (None, Some(words)) => u16::try_from(words.as_int().unwrap_or(1)).unwrap_or(1),
            (Some(_), Some(stated)) => {
                self.problem(
                    "words_not_needed",
                    &stated.at,
                    format!("The register `{name}` states `words`, and its `data` already fixes how many it takes."),
                    "Remove `words:`; only `text` states it.",
                );
                fine = false;
                1
            }
            (None, None) => {
                self.problem(
                    "words_missing",
                    &doc.at,
                    format!("The register `{name}` holds text and does not state how many words it takes."),
                    "Add `words:` with the number of words.",
                );
                fine = false;
                1
            }
        };
        if u32::from(offset) + u32::from(words) > 0x1_0000 {
            self.problem(
                "address_past_end",
                &address.at,
                format!("The register `{name}` runs past the last address of its table."),
                "Correct the offset.",
            );
            fine = false;
        }
        if data == Data::Text && access.writable() {
            self.problem(
                "text_not_writable",
                doc.get("access").map_or(&doc.at, |access| &access.at),
                format!("The register `{name}` holds text; writing text is not built."),
                "Declare `access: read`.",
            );
            fine = false;
        }
        let word_order = match doc.text("word_order") {
            Some("low_first") => WordOrder::LowFirst,
            _ => WordOrder::HighFirst,
        };
        let scale = doc.get("scale").and_then(|scale| {
            let parsed = decimal_of(scale).filter(|scale| !scale.is_zero() && !scale.is_negative());
            if parsed.is_none() || data.bounds().is_none() {
                self.problem(
                    "scale_invalid",
                    &scale.at,
                    format!("The scale of the register `{name}` is not a positive exact number on a register that holds a number."),
                    "Write the value of one raw step, such as `\"0.1\"`, or remove `scale:`.",
                );
                fine = false;
            }
            parsed
        });
        let field_type = doc
            .text("type")
            .and_then(|field_type| self.cx.target().field_type(field_type));
        let kind = match (
            data,
            field_type.map(|view| (view.compiled.base, &view.compiled.ty)),
        ) {
            (Data::Bit, None | Some((FieldBase::Boolean, _))) => Some(ValueKind::Bool),
            (Data::Bit, Some((FieldBase::Choice, _))) => Some(ValueKind::Code),
            (Data::Text, None | Some((_, ValueType::Text))) => Some(ValueKind::Text),
            (Data::Bit | Data::Text, Some(_)) => None,
            (_, Some((FieldBase::Choice, _))) if scale.is_none() => Some(ValueKind::Code),
            (_, None) if scale.is_none() => Some(ValueKind::Int),
            (_, Some((_, ValueType::Int))) if scale.is_none() => Some(ValueKind::Int),
            (_, None | Some((_, ValueType::Decimal))) => Some(ValueKind::Decimal),
            (_, Some(_)) => None,
        };
        if kind.is_none() {
            self.problem(
                "type_not_of_data",
                doc.get("type").map_or(&doc.at, |field_type| &field_type.at),
                format!("The field type of the register `{name}` does not fit its `data` and `scale`."),
                "A bit takes a boolean or a choice; a scaled number takes a decimal, money, or measurement type; an unscaled number also takes an integer or a choice.",
            );
            fine = false;
        }
        let range = doc.get("range").and_then(|range| {
            let bounds = range
                .get("min")
                .and_then(decimal_of)
                .zip(range.get("max").and_then(decimal_of));
            let raw = bounds.and_then(|(min, max)| {
                let raw_min = raw_of(&min, scale.as_ref())?;
                let raw_max = raw_of(&max, scale.as_ref())?;
                let (least, most) = data.bounds()?;
                (min <= max && raw_min >= least && raw_max <= most).then_some(CompiledRange {
                    min,
                    max,
                    raw_min,
                    raw_max,
                })
            });
            if raw.is_none() {
                self.problem(
                    "range_invalid",
                    &range.at,
                    format!("The range of the register `{name}` is not two exact numbers, least first, each a whole number of the register's steps that its `data` can hold."),
                    "Write the safe range in the register's unit, inside what the register can hold.",
                );
                fine = false;
            }
            raw
        });
        let read_effect = match doc.text("read_effect") {
            Some("clears") => ReadEffect::Clears,
            Some("advances") => ReadEffect::Advances,
            _ => ReadEffect::None,
        };
        let confirm_by = doc.get("confirm_by").and_then(|confirm| {
            Some(ConfirmBy {
                register: confirm.text("register")?.to_owned(),
                within: confirm.get("within").and_then(decimal_of)?,
                settle_ms: self.time(confirm.get("settle"))?,
            })
        });
        fine.then_some(CompiledRegister {
            table,
            offset,
            data,
            words,
            word_order,
            scale,
            kind: kind?,
            access,
            range,
            hazard: doc.text("hazard").and_then(|hazard| hazard.parse().ok()),
            read_effect,
            confirm_by,
        })
    }
}

/// One of a closed list of words, as its type.
fn parse_word<T: for<'de> Deserialize<'de>>(word: &str) -> Option<T> {
    serde_json::from_value(serde_json::Value::String(word.to_owned())).ok()
}

/// The `registers/` file kind.
pub(crate) struct RegisterKind {
    names: Arc<Names>,
    kinds: Vec<SurfaceKind>,
    schema: JsonSchema,
}

impl RegisterKind {
    pub fn new(names: Arc<Names>) -> Self {
        Self {
            kinds: vec![names.surface_kind.clone()],
            names,
            schema: schema(),
        }
    }
}

impl FileKind for RegisterKind {
    fn name(&self) -> FileKindName {
        self.names.register_kind.clone()
    }

    fn surface_kinds(&self) -> &[SurfaceKind] {
        &self.kinds
    }

    fn folder(&self) -> &str {
        "registers"
    }

    fn schema(&self) -> &JsonSchema {
        &self.schema
    }

    fn names(&self, doc: &Located<Doc>) -> Vec<Located<DefinedName>> {
        doc.get("device_map")
            .and_then(|name| {
                name.as_text().map(|text| {
                    Located::new(
                        DefinedName {
                            name: text.to_owned(),
                        },
                        name.at.clone(),
                    )
                })
            })
            .into_iter()
            .collect()
    }

    fn expressions(&self, _doc: &Located<Doc>) -> Vec<ExprSite> {
        Vec::new()
    }

    /// A map describes a write when any of its registers is not plainly
    /// `access: read`: a writable register carries the safe range and the
    /// hazard a person decides. An access that is missing, or an open
    /// question, is not known to read.
    fn writes(&self, doc: &Located<Doc>) -> bool {
        doc.get("registers")
            .map(Located::entries)
            .unwrap_or_default()
            .iter()
            .any(|entry| entry.value.text("access") != Some("read"))
    }

    fn compile(
        &self,
        docs: &[Located<Doc>],
        cx: &KindContext<'_>,
    ) -> Result<CompiledSection, Diagnostics> {
        let mut compiler = Compiler {
            names: &self.names,
            cx,
            problems: Diagnostics::new(),
        };
        let mut section = MapSection::new();
        for doc in docs {
            if let Some((name, map)) = compiler.map(doc) {
                section.insert(name, map);
            }
        }
        let mut problems = compiler.problems;
        if !problems.is_empty() {
            return Err(problems);
        }
        PackPayload::encode(&section)
            .map(|payload| CompiledSection {
                format: SECTION_FORMAT,
                payload,
            })
            .map_err(|failure| {
                if let (Ok(code), Some(doc)) = (lint_code("map_not_encodable"), docs.first()) {
                    problems.push(Diagnostic::error(
                        code,
                        Stage::Lower,
                        &doc.at,
                        failure.message().to_owned(),
                        "Keep every number in a map whole or written as text.",
                    ));
                }
                problems
            })
    }
}

/// Which of the three rules about writable registers a lint rule checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WritableRule {
    /// Every writable register declares a hazard.
    Hazard,
    /// Every writable register that holds a number declares its safe range.
    Range,
    /// A register that can only be written is a command, and a command's
    /// hazard is not `none`.
    CommandHazard,
}

/// A lint rule over every writable register of every map.
pub(crate) struct WritableRegisterRule {
    names: Arc<Names>,
    rule: WritableRule,
    code: LintCode,
}

impl WritableRegisterRule {
    pub fn new(names: Arc<Names>, rule: WritableRule) -> Result<Self, a2m_core::Failure> {
        let code = lint_code(match rule {
            WritableRule::Hazard => "writable_without_hazard",
            WritableRule::Range => "writable_without_range",
            WritableRule::CommandHazard => "command_hazard_none",
        })?;
        Ok(Self { names, rule, code })
    }
}

impl LintRule for WritableRegisterRule {
    fn code(&self) -> LintCode {
        self.code.clone()
    }

    // A range and a hazard are what stand between a caller and a device, so
    // a draft does not compile without them either.
    fn level(&self, _mode: CompileMode) -> Level {
        Level::Error
    }

    fn summary(&self) -> &str {
        match self.rule {
            WritableRule::Hazard => "Every writable register declares a hazard.",
            WritableRule::Range => {
                "Every writable register that holds a number declares its safe range."
            }
            WritableRule::CommandHazard => {
                "A register that can only be written is a command, and a command's hazard is not `none`."
            }
        }
    }

    fn check(&self, view: &ResolvedView<'_>) -> Vec<Diagnostic> {
        let mut found = Vec::new();
        for doc in view.entries(&self.names.register_kind, &self.names.surface_kind) {
            for entry in doc
                .get("registers")
                .map(Located::entries)
                .unwrap_or_default()
            {
                let (name, register) = (&entry.key.value, &entry.value);
                let access = register.text("access").unwrap_or_default();
                if access != "write" && access != "read_write" {
                    continue;
                }
                let hazard = register.text("hazard");
                let problem = match self.rule {
                    WritableRule::Hazard if hazard.is_none() => Some((
                        format!("The register `{name}` can be written and declares no hazard."),
                        "Add `hazard:` with what a write to it could harm: `none`, `device_function`, or `physical`.",
                    )),
                    WritableRule::Range
                        if register.get("range").is_none()
                            && !matches!(register.text("data"), Some("bit" | "text")) =>
                    {
                        Some((
                            format!(
                                "The register `{name}` can be written, holds a number, and declares no safe range."
                            ),
                            "Add `range:` with the least and the greatest safe value, in the register's unit.",
                        ))
                    }
                    WritableRule::CommandHazard if access == "write" && hazard == Some("none") => {
                        Some((
                            format!(
                                "The register `{name}` is a command, and its hazard is `none`."
                            ),
                            "Declare what the command could harm: `device_function` or `physical`.",
                        ))
                    }
                    _ => None,
                };
                if let Some((message, fix)) = problem {
                    found.push(Diagnostic::error(
                        self.code.clone(),
                        Stage::Lint,
                        &entry.key.at,
                        message,
                        fix,
                    ));
                }
            }
        }
        found
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        reason = "a test: a helper that cannot do its work stops the test"
    )]

    use std::sync::Arc;

    use a2m_core::{
        Decimal, Failure, FileKindName, Finding, FindingKind, ProducedBy, Source, Subject,
        Timestamp,
    };
    use a2m_files::{
        CompilerSet, DraftContext, DraftDoc, DraftValue, Drafter, DraftingRule, RelPath, TargetRoot,
    };

    use crate::names::{Names, PACK};

    /// A rule that drafts what the pack's own rule never does: a map with
    /// a register that can be written.
    struct DraftsAWrite(Arc<Names>);

    impl DraftingRule for DraftsAWrite {
        fn finding_kind(&self) -> FindingKind {
            self.0.identity_finding.clone()
        }

        fn file_kind(&self) -> FileKindName {
            self.0.register_kind.clone()
        }

        fn draft(
            &self,
            _finding: &Finding,
            _cx: &DraftContext<'_>,
        ) -> Result<Vec<DraftDoc>, Failure> {
            let map = |file: &str, access: &str| -> Result<DraftDoc, Failure> {
                let register = DraftValue::map(vec![
                    ("table", DraftValue::text("holding")),
                    ("address", DraftValue::text("0")),
                    ("access", DraftValue::text(access)),
                ]);
                Ok(DraftDoc {
                    path: RelPath::new(&format!("registers/{file}.yaml"))?,
                    body: DraftValue::map(vec![
                        ("device_map", DraftValue::text(file)),
                        ("surface", DraftValue::text("controller")),
                        (
                            "registers",
                            DraftValue::map(vec![("dosing_rate", register)]),
                        ),
                    ]),
                })
            };
            Ok(vec![
                map("tries_a_write", "read_write")?,
                map("only_reads", "read")?,
            ])
        }
    }

    #[test]
    fn a_drafting_rule_that_drafts_a_writable_register_is_refused() {
        let names = Arc::new(Names::new().unwrap());
        let mut builder = CompilerSet::builder();
        builder
            .file_kind(PACK, Arc::new(super::RegisterKind::new(Arc::clone(&names))))
            .unwrap()
            .drafting_rule(PACK, Arc::new(DraftsAWrite(Arc::clone(&names))))
            .unwrap();
        let set = builder.build();
        let finding = Finding::about(
            Subject::Device {
                reported_id: String::new(),
                model: "example-controller".to_owned(),
                firmware: "2.4.0".to_owned(),
            },
            names.identity_finding.clone(),
            "{}",
            Source::LiveSystem,
            ProducedBy {
                provider: PACK.to_owned(),
                version: "0.0.1".to_owned(),
                component: None,
            },
            Timestamp::from_unix_millis(0),
        )
        .build()
        .unwrap();
        let root = std::env::temp_dir().join(format!(
            "a2m-pack-device-draft-write-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        // The target has the surface the drafted maps name, of the kind the
        // pack registered register files for, so that a draft is not refused
        // for living on none (decision 0203).
        std::fs::write(
            root.join("target.yaml"),
            "schema_version: 1\ntarget: example\nsurfaces:\n  controller:\n    kind: device_link\n    address: \"127.0.0.1:502\"\n    protocol: modbus_tcp\n    unit: 1\n",
        )
        .unwrap();
        let report = Drafter::new(&set, &TargetRoot::new(&root))
            .draft(std::slice::from_ref(&finding))
            .unwrap();
        let refused: Vec<&str> = report
            .refused
            .iter()
            .map(|(path, _)| path.as_str())
            .collect();
        // Refused for the write it would allow, not for another reason.
        for (path, why) in &report.refused {
            assert!(
                why.contains("drafts nothing that writes"),
                "{path} was refused for another reason: {why}"
            );
        }
        let written: Vec<&str> = report.written.iter().map(RelPath::as_str).collect();
        let exists = root.join("registers").join("tries_a_write.yaml").exists();
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(refused, ["registers/tries_a_write.yaml"]);
        assert_eq!(written, ["registers/only_reads.yaml"]);
        assert!(!exists);
    }

    use super::{duration_ms, per_second, raw_of};

    #[test]
    fn times_and_rates_are_whole_numbers_with_a_unit() {
        assert_eq!(duration_ms("20ms"), Some(20));
        assert_eq!(duration_ms("5s"), Some(5000));
        assert_eq!(duration_ms("5"), None);
        assert_eq!(duration_ms("-5s"), None);
        assert_eq!(duration_ms("1.5s"), None);
        assert_eq!(per_second("20/s"), Some(20));
        assert_eq!(per_second("0/s"), None);
        assert_eq!(per_second("20"), None);
    }

    #[test]
    fn a_value_is_a_whole_number_of_steps_or_it_has_no_raw_number() {
        let scale = Decimal::parse("0.1").unwrap();
        let value = |text: &str| Decimal::parse(text).unwrap();
        assert_eq!(raw_of(&value("145.0"), Some(&scale)), Some(1450));
        assert_eq!(raw_of(&value("-10"), Some(&scale)), Some(-100));
        assert_eq!(raw_of(&value("0.25"), Some(&scale)), None);
        assert_eq!(raw_of(&value("7"), None), Some(7));
        assert_eq!(raw_of(&value("7.5"), None), None);
    }
}
