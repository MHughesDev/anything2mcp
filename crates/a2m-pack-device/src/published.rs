//! A register map a vendor published, given as a file in the project: read
//! through the run's checked paths, turned into one finding at `inferred`
//! with the source `published_description`, and drafted into a map of
//! readable registers with one read each (decision 0188).
//!
//! Nothing here sends a frame. Nothing was observed of the device: the
//! finding states what the file says, and a device whose model or firmware
//! is outside the drafted map still stops every run, reads included.
//!
//! The layout the reader takes, as the pack's design states it: UTF-8
//! text, one record a line, cells divided by commas and never quoted.
//! Before the header, a line that starts with `#` is a note; two notes are
//! read, `# model: <model>` and `# firmware: <version>, <version>`. The
//! header row names the columns, in any order: `name`, `table`, `offset`,
//! and `data` are required; `words`, `word_order`, `scale`, `unit`, and
//! `access` may be present. A file with any other column, a quote, or a row
//! of another width is not a map, and no finding is made of it. An empty
//! cell states nothing; an empty `scale` is the layout's own word for a
//! register that holds the raw number.
//!
//! A map file is hostile input (decision 0197). A file is read whole up to
//! [`MOST_MAP_BYTES`], at most [`MOST_ROWS`] rows, and a cell or a note
//! longer than [`LONGEST_CELL`] characters makes the file no map. What the
//! file says reaches a drafted file only through the YAML writer, and only
//! from a closed set of characters: a register's name as `[a-z0-9_]`, a
//! unit, the model, and the firmware versions from the sets below. A unit,
//! a model, or a version outside its set is not copied: the question is
//! asked instead, with the file's words quoted apart from the pack's
//! sentence (`open_question_quoting`). A description repeats a name or a
//! unit only in backticks, as the map's words.

use std::collections::BTreeSet;
use std::sync::Arc;

use a2m_core::{
    Coverage, Decimal, EvidenceLevel, Failure, FailureClass, FailureCode, FileKindName, Finding,
    FindingKind, OperationName, ProducedBy, Source, Subject, Values,
};
use a2m_files::{DraftContext, DraftDoc, DraftValue, DraftingRule, RelPath, UNTRUSTED_TEXT};
use a2m_provider::{
    BoxFuture, CheckedPath, DescribeForm, Describer, Description, Discoverer, DiscovererName,
    DiscoveryReport, DiscoveryScope, FindingSink, SandboxPort, SurfaceDescription,
};
use a2m_telemetry::Clock;
use serde::{Deserialize, Serialize};

use crate::codes;
use crate::names::{Names, PACK};

/// The discoverer's name, and the kind of finding it makes.
pub(crate) const PUBLISHED_MAP: &str = "device_register_map";

/// The name, among a discovery's checked paths, of a published map given
/// as a file: the one name the manifest's `device_register_map` declares
/// under `files`, so the engine hands it a file of the project's `sources`
/// folder under it (decision 0263). No other path is read.
pub const REGISTER_MAP_FILE: &str = "register_map";

/// The columns the layout knows; the first four are required.
const COLUMNS: &[&str] = &[
    "name",
    "table",
    "offset",
    "data",
    "words",
    "word_order",
    "scale",
    "unit",
    "access",
];
const REQUIRED: usize = 4;
/// The most rows of one map that are read.
const MOST_ROWS: usize = 4096;
/// The longest unit the drafter repeats in a read's description.
const LONGEST_UNIT: usize = 16;
/// The largest map file that is read, whatever the job's bound.
pub(crate) const MOST_MAP_BYTES: u64 = 1 << 20;
/// The longest cell or note of a map, in characters.
const LONGEST_CELL: usize = 64;
/// The longest model or firmware version the drafter copies.
const LONGEST_COPIED: usize = 40;

/// One row of a published map, as the file states it. Every cell is the
/// file's text, and an empty cell is absent.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MapRow {
    /// Its line in the file, counted from one.
    pub line: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub words: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub word_order: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access: Option<String>,
}

/// What a `device_register_map` finding holds: a published map, as its file
/// states it. One finding a file, because one map is one document: the
/// drafter hands a rule one finding at a time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PublishedMap {
    /// The surface the map was given for.
    pub surface: String,
    /// The file's name inside the project folder.
    pub file: String,
    /// The model the file says it describes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// The firmware versions it says it describes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub firmware: Vec<String>,
    /// Its rows, in order.
    pub rows: Vec<MapRow>,
}

fn invalid(what: &str) -> Failure {
    codes::failure(
        FailureClass::Input,
        FailureCode::FINDING_INVALID,
        format!("A `{PUBLISHED_MAP}` finding cannot be made or read: {what}."),
        "Run discovery on the device link again with the published map.",
    )
}

/// Reads a published map's text in the layout above. `None`: the file is
/// not a map in that layout.
pub(crate) fn parse(file: &str, surface: &str, bytes: &[u8]) -> Option<PublishedMap> {
    let text = std::str::from_utf8(bytes).ok()?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut map = PublishedMap {
        surface: surface.to_owned(),
        file: file.to_owned(),
        model: None,
        firmware: Vec::new(),
        rows: Vec::new(),
    };
    let mut header: Option<Vec<usize>> = None;
    for (number, line) in text.lines().enumerate() {
        let line = line.strip_suffix('\r').unwrap_or(line);
        let number = u32::try_from(number + 1).ok()?;
        if line.contains('"') || line.chars().any(|c| c.is_control() && c != '\t') {
            return None;
        }
        // A note or a cell longer than any a map needs: not a map.
        let long = |text: &str| text.trim().chars().count() > LONGEST_CELL;
        if line.strip_prefix('#').map_or_else(
            || line.split(',').any(long),
            |note| note.split(',').any(long) || note.trim().chars().count() > 2 * LONGEST_CELL,
        ) {
            return None;
        }
        if header.is_none() {
            if let Some(note) = line.strip_prefix('#') {
                let note = note.trim();
                if let Some(model) = note.strip_prefix("model:") {
                    map.model = Some(model.trim().to_owned()).filter(|model| !model.is_empty());
                } else if let Some(versions) = note.strip_prefix("firmware:") {
                    map.firmware = versions
                        .split(',')
                        .map(str::trim)
                        .filter(|version| !version.is_empty())
                        .map(str::to_owned)
                        .collect();
                }
                continue;
            }
            if line.trim().is_empty() {
                continue;
            }
            let mut columns = Vec::new();
            for cell in line.split(',') {
                let position = COLUMNS.iter().position(|known| *known == cell.trim())?;
                if columns.contains(&position) {
                    return None;
                }
                columns.push(position);
            }
            if (0..REQUIRED).any(|required| !columns.contains(&required)) {
                return None;
            }
            header = Some(columns);
            continue;
        }
        if line.trim().is_empty() {
            continue;
        }
        let columns = header.as_ref()?;
        let cells: Vec<&str> = line.split(',').map(str::trim).collect();
        if cells.len() != columns.len() || map.rows.len() >= MOST_ROWS {
            return None;
        }
        let mut row = MapRow {
            line: number,
            ..MapRow::default()
        };
        for (cell, column) in cells.iter().zip(columns) {
            let stated = (!cell.is_empty()).then(|| (*cell).to_owned());
            let slot = match COLUMNS[*column] {
                "name" => &mut row.name,
                "table" => &mut row.table,
                "offset" => &mut row.offset,
                "data" => &mut row.data,
                "words" => &mut row.words,
                "word_order" => &mut row.word_order,
                "scale" => &mut row.scale,
                "unit" => &mut row.unit,
                _ => &mut row.access,
            };
            *slot = stated;
        }
        map.rows.push(row);
    }
    header.map(|_| map)
}

/// Why a row gives no readable register, in the pack's words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LeftOut {
    /// No name the drafter can spell a register by.
    NoName,
    /// No data type, or one the pack does not know.
    NoType,
    /// An address the protocol cannot hold: an unknown table, an offset
    /// that is not a whole number, or a register that ends past the table.
    Address,
    /// A coil or a discrete input: a bit, which this rule drafts no read of.
    Bit,
    /// Can only be written.
    WriteOnly,
    /// A scale, a word count, a word order, or an access the pack cannot
    /// read.
    Unreadable,
}

impl LeftOut {
    pub(crate) const fn why(self) -> &'static str {
        match self {
            Self::NoName => "it states no name a register can be spelled by",
            Self::NoType => "it states no data type the pack knows",
            Self::Address => "it states an address the protocol cannot hold",
            Self::Bit => "it is a coil or a discrete input, of which no read is drafted",
            Self::WriteOnly => "it can only be written",
            Self::Unreadable => {
                "it states a scale, a word count, a word order, or an access the pack cannot read"
            }
        }
    }
}

/// One register the drafter writes, and the read of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Drafted {
    pub name: String,
    pub table: &'static str,
    pub offset: u16,
    pub data: &'static str,
    pub words: Option<u16>,
    pub low_first: bool,
    pub scale: Option<String>,
    /// The unit the map states, when the pack can repeat it safely.
    pub unit: Option<String>,
    /// A unit the map states in characters the pack does not copy: only
    /// ever quoted apart in a question.
    pub unit_not_copied: Option<String>,
}

impl Drafted {
    /// The output type of its read: text is marked untrusted, a scaled
    /// number is an exact decimal, and a raw number an integer.
    fn output_type(&self) -> &'static str {
        if self.data == "text" {
            UNTRUSTED_TEXT
        } else if self.scale.is_some() {
            "decimal"
        } else {
            "integer"
        }
    }
}

/// A name made from the file's own word: lowercase letters, digits, and
/// underscores, starting with a letter, at most 40 characters.
fn name_of(text: &str) -> Option<String> {
    let mut name = String::new();
    for character in text.chars() {
        let character = character.to_ascii_lowercase();
        if character.is_ascii_lowercase() || character.is_ascii_digit() {
            name.push(character);
        } else if !name.ends_with('_') && !name.is_empty() {
            name.push('_');
        }
    }
    let name = name.trim_end_matches('_').to_owned();
    (name.starts_with(|first: char| first.is_ascii_lowercase()) && name.len() <= 40).then_some(name)
}

/// A unit the drafter may repeat in its own sentence: short, and of a
/// closed set of characters: ASCII letters and digits, and the few signs
/// units are written with. No other letter, so no letter of another script
/// that looks like a Latin one. Anything else is not copied, and asked.
fn safe_unit(unit: &str) -> Option<String> {
    let fits = !unit.is_empty()
        && unit.chars().count() <= LONGEST_UNIT
        && unit.chars().all(|c| {
            c.is_ascii_alphanumeric()
                || matches!(c, '%' | '/' | '.' | '-' | '°' | '²' | '³' | '·' | 'µ' | 'Ω')
        });
    fits.then(|| unit.to_owned())
}

/// A model or firmware version the drafter copies into the map: ASCII
/// letters, digits, spaces, and `.`, `-`, `_`, `+`, at most 40 characters.
fn copied(text: &str) -> Option<&str> {
    (text.chars().count() <= LONGEST_COPIED
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '.' | '-' | '_' | '+')))
    .then_some(text)
}

/// The register a row gives, or why it gives none. Rule 7: nothing the
/// row does not state is made up; an unknown word leaves the row out.
pub(crate) fn drafted(row: &MapRow, taken: &mut BTreeSet<String>) -> Result<Drafted, LeftOut> {
    let table = match row.table.as_deref() {
        Some("input") => "input",
        Some("holding") => "holding",
        Some("coil" | "discrete") => return Err(LeftOut::Bit),
        _ => return Err(LeftOut::Address),
    };
    let data = match row.data.as_deref() {
        Some("u16") => "u16",
        Some("i16") => "i16",
        Some("u32") => "u32",
        Some("i32") => "i32",
        Some("text") => "text",
        Some("bit") => return Err(LeftOut::Bit),
        _ => return Err(LeftOut::NoType),
    };
    let offset: u32 = row
        .offset
        .as_deref()
        .filter(|digits| !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit()))
        .and_then(|digits| digits.parse().ok())
        .ok_or(LeftOut::Address)?;
    let words = match (data, row.words.as_deref()) {
        ("text", Some(words)) => Some(
            words
                .parse::<u16>()
                .ok()
                .filter(|words| (1..=125).contains(words))
                .ok_or(LeftOut::Unreadable)?,
        ),
        ("text", None) => return Err(LeftOut::NoType),
        (_, Some(_)) => return Err(LeftOut::Unreadable),
        (_, None) => None,
    };
    let width = match data {
        "u32" | "i32" => 2,
        "text" => u32::from(words.unwrap_or(1)),
        _ => 1,
    };
    // Checked: an offset near the top of `u32` is an address the protocol
    // cannot hold, never an overflow.
    if offset.checked_add(width - 1).is_none_or(|end| end > 0xFFFF) {
        return Err(LeftOut::Address);
    }
    let offset = u16::try_from(offset).map_err(|_| LeftOut::Address)?;
    let low_first = match row.word_order.as_deref() {
        None | Some("high_first") => false,
        Some("low_first") if width == 2 && data != "text" => true,
        Some(_) => return Err(LeftOut::Unreadable),
    };
    let scale = match row.scale.as_deref() {
        None => None,
        Some(_) if data == "text" => return Err(LeftOut::Unreadable),
        Some(text) => {
            let scale = Decimal::parse(text).map_err(|_| LeftOut::Unreadable)?;
            if scale.is_zero() || scale.is_negative() {
                return Err(LeftOut::Unreadable);
            }
            Some(text.to_owned())
        }
    };
    match row.access.as_deref() {
        None | Some("read" | "read_write") => {}
        Some("write") => return Err(LeftOut::WriteOnly),
        Some(_) => return Err(LeftOut::Unreadable),
    }
    let name = row
        .name
        .as_deref()
        .and_then(name_of)
        .ok_or(LeftOut::NoName)?;
    let mut unique = name.clone();
    let mut number = 2;
    while !taken.insert(unique.clone()) {
        unique = format!("{name}_{number}");
        number += 1;
    }
    Ok(Drafted {
        name: unique,
        table,
        offset,
        data,
        words,
        low_first,
        scale,
        unit: row.unit.as_deref().and_then(safe_unit),
        unit_not_copied: row
            .unit
            .as_deref()
            .filter(|unit| safe_unit(unit).is_none())
            .map(str::to_owned),
    })
}

/// Every row of a map, as the drafter takes it.
pub(crate) fn judged(map: &PublishedMap) -> Vec<(u32, Result<Drafted, LeftOut>)> {
    let mut taken = BTreeSet::new();
    map.rows
        .iter()
        .map(|row| (row.line, drafted(row, &mut taken)))
        .collect()
}

fn map_of(finding: &Finding) -> Result<PublishedMap, Failure> {
    serde_json::from_str(finding.found()).map_err(|_| invalid("it does not hold a published map"))
}

fn read_handed(path: &CheckedPath, most: u64) -> Option<Vec<u8>> {
    (path.entry_kind().ok()? == a2m_provider::EntryKind::File)
        .then(|| path.read(most).ok())
        .flatten()
}

/// Reads a published map from the file a discovery job is handed under
/// [`REGISTER_MAP_FILE`], when its name ends `.csv`. It never touches the
/// link.
pub(crate) struct PublishedMapDiscoverer {
    names: Arc<Names>,
    name: DiscovererName,
    kind: FindingKind,
    clock: Arc<dyn Clock>,
    version: String,
}

impl PublishedMapDiscoverer {
    pub fn new(names: Arc<Names>, clock: Arc<dyn Clock>, version: String) -> Result<Self, Failure> {
        Ok(Self {
            names,
            name: DiscovererName::new(PUBLISHED_MAP)?,
            kind: FindingKind::new(PUBLISHED_MAP)?,
            clock,
            version,
        })
    }
}

impl Discoverer for PublishedMapDiscoverer {
    fn name(&self) -> DiscovererName {
        self.name.clone()
    }

    fn suits(&self, surface: &SurfaceDescription) -> bool {
        surface.kind == self.names.surface_kind
    }

    fn discover<'a>(
        &'a self,
        _port: &'a dyn SandboxPort,
        scope: &'a DiscoveryScope<'a>,
        out: &'a mut dyn FindingSink,
    ) -> BoxFuture<'a, Result<DiscoveryReport, Failure>> {
        Box::pin(async move {
            let mut report = DiscoveryReport {
                coverage: Coverage::Complete,
                ..DiscoveryReport::default()
            };
            let mut left = scope.bounds.bytes;
            // At most one file: the one handed under the declared name.
            for path in scope.paths.get(REGISTER_MAP_FILE).into_iter() {
                let file = path.relative().rsplit('/').next().unwrap_or_default();
                if !file.to_ascii_lowercase().ends_with(".csv") {
                    report.dropped += 1;
                    continue;
                }
                if scope.deadline.passed() {
                    report.coverage = Coverage::Partial;
                    report.bounds_hit.push("seconds".to_owned());
                    break;
                }
                let reading = path.clone();
                let most = left.min(MOST_MAP_BYTES);
                let bytes = tokio::task::spawn_blocking(move || read_handed(&reading, most))
                    .await
                    .ok()
                    .flatten();
                let Some(bytes) = bytes else {
                    report.dropped += 1;
                    continue;
                };
                left = left.saturating_sub(bytes.len() as u64);
                let Some(map) = parse(file, scope.surface.surface.name.as_str(), &bytes) else {
                    report.dropped += 1;
                    continue;
                };
                let found = serde_json::to_string(&map)
                    .map_err(|_| invalid("the map cannot be written"))?;
                let finding = Finding::about(
                    Subject::File {
                        digest: a2m_core::Digest::of(&bytes),
                        format: "csv".to_owned(),
                        part: None,
                    },
                    self.kind.clone(),
                    found,
                    Source::PublishedDescription,
                    ProducedBy {
                        provider: PACK.to_owned(),
                        version: self.version.clone(),
                        component: None,
                    },
                    self.clock.now(),
                )
                .level(EvidenceLevel::Inferred)
                .coverage(Coverage::Complete)
                .build()?;
                out.found(finding)?;
            }
            Ok(report)
        })
    }
}

/// Puts published maps so that an agent can read them: what the drafter
/// takes, and each row it leaves out by its line and why. The names the
/// file gives are the file's words, kept apart.
pub(crate) struct PublishedMapDescriber;

impl Describer for PublishedMapDescriber {
    fn describe(&self, findings: &[Finding], form: DescribeForm) -> Description {
        let maps: Vec<PublishedMap> = findings
            .iter()
            .filter(|finding| finding.kind().as_str() == PUBLISHED_MAP)
            .filter_map(|finding| map_of(finding).ok())
            .collect();
        let mut text = format!(
            "{} published map(s) read; no frame was sent to read them.",
            maps.len()
        );
        let mut untrusted = Vec::new();
        let mut kept = 0_i64;
        for (number, map) in maps.iter().enumerate() {
            for (line, judged) in judged(map) {
                match judged {
                    Ok(_) => kept += 1,
                    Err(why) => {
                        text.push_str(&format!(
                            " Map {}, line {line}: left out, because {}.",
                            number + 1,
                            why.why()
                        ));
                    }
                }
            }
            if form == DescribeForm::Full {
                untrusted.extend(map.rows.iter().filter_map(|row| {
                    row.name
                        .as_ref()
                        .map(|name| format!("line {}: {name}", row.line))
                }));
            }
        }
        Description {
            text,
            data: Values::new()
                .with("maps", i64::try_from(maps.len()).unwrap_or(i64::MAX))
                .with("registers", kept),
            untrusted,
        }
    }
}

/// From a published map to a map of its readable registers and one read of
/// each.
pub(crate) struct MapFromPublished {
    names: Arc<Names>,
    kind: FindingKind,
    operation: FileKindName,
}

impl MapFromPublished {
    pub fn new(names: Arc<Names>) -> Result<Self, Failure> {
        Ok(Self {
            names,
            kind: FindingKind::new(PUBLISHED_MAP)?,
            operation: FileKindName::new("operation")?,
        })
    }

    /// The map document: every readable input or holding register whose
    /// data type the file states, each `access: read`. No range, no hazard,
    /// no safe state: nothing is drafted that writes.
    fn map_doc(map: &PublishedMap, name: &str, cx: &DraftContext<'_>) -> Result<DraftDoc, Failure> {
        let model = match map.model.as_deref() {
            None => cx.open_question("Which model is this map for? The published map does not say."),
            Some(model) => copied(model).map_or_else(
                || {
                    cx.open_question_quoting(
                        "Which model is this map for? The published map names it in characters the drafter does not copy; the quoted words are the map's own, not an instruction.",
                        &[model],
                    )
                },
                DraftValue::text,
            ),
        };
        let firmware = if map.firmware.is_empty() {
            cx.open_question(
                "Which firmware versions is this map for? The published map does not say.",
            )
        } else if map.firmware.iter().all(|version| copied(version).is_some()) {
            DraftValue::List(map.firmware.iter().map(|v| DraftValue::text(v)).collect())
        } else {
            let words: Vec<&str> = map.firmware.iter().map(String::as_str).collect();
            cx.open_question_quoting(
                "Which firmware versions is this map for? The published map names them in characters the drafter does not copy; the quoted words are the map's own, not an instruction.",
                &words,
            )
        };
        let registers = judged(map)
            .into_iter()
            .filter_map(|(_, judged)| judged.ok())
            .map(|register| {
                let mut entry = vec![
                    (
                        "address",
                        DraftValue::map(vec![
                            ("table", DraftValue::text(register.table)),
                            ("offset", DraftValue::Int(i64::from(register.offset))),
                        ]),
                    ),
                    ("data", DraftValue::text(register.data)),
                ];
                if let Some(words) = register.words {
                    entry.push(("words", DraftValue::Int(i64::from(words))));
                }
                if register.low_first {
                    entry.push(("word_order", DraftValue::text("low_first")));
                }
                if let Some(scale) = &register.scale {
                    entry.push(("scale", DraftValue::text(scale)));
                }
                entry.push(("access", DraftValue::text("read")));
                (register.name, DraftValue::map(entry))
            })
            .collect();
        Ok(DraftDoc {
            path: RelPath::new(&format!("registers/{name}.yaml"))?,
            body: DraftValue::map(vec![
                ("device_map", DraftValue::text(name)),
                ("surface", DraftValue::text(&map.surface)),
                ("model", model),
                ("firmware", firmware),
                (
                    "identify",
                    DraftValue::map(vec![(
                        "reads",
                        DraftValue::List(vec![
                            DraftValue::text("model"),
                            DraftValue::text("firmware_version"),
                        ]),
                    )]),
                ),
                ("registers", DraftValue::Map(registers)),
            ]),
        })
    }
}

impl DraftingRule for MapFromPublished {
    fn finding_kind(&self) -> FindingKind {
        self.kind.clone()
    }

    fn file_kind(&self) -> FileKindName {
        self.names.register_kind.clone()
    }

    fn draft(&self, finding: &Finding, cx: &DraftContext<'_>) -> Result<Vec<DraftDoc>, Failure> {
        let map = map_of(finding)?;
        let stem = map
            .file
            .rsplit_once('.')
            .map_or(map.file.as_str(), |(stem, _)| stem);
        let name =
            name_of(stem).ok_or_else(|| invalid("its file's name gives no name for a map"))?;
        Ok(vec![Self::map_doc(&map, &name, cx)?])
    }
}

/// From a published map to one read of each register drafted into its map.
pub(crate) struct ReadsFromPublished {
    rule: MapFromPublished,
}

impl ReadsFromPublished {
    pub fn new(names: Arc<Names>) -> Result<Self, Failure> {
        Ok(Self {
            rule: MapFromPublished::new(names)?,
        })
    }
}

impl DraftingRule for ReadsFromPublished {
    fn finding_kind(&self) -> FindingKind {
        self.rule.kind.clone()
    }

    fn file_kind(&self) -> FileKindName {
        self.rule.operation.clone()
    }

    /// One read a register. Its output is of the type the data and the
    /// scale give. A number whose unit the map states is read as that, and
    /// its description says the unit is the map's; a number whose unit it
    /// does not state holds that question, and only that read waits on it.
    fn draft(&self, finding: &Finding, cx: &DraftContext<'_>) -> Result<Vec<DraftDoc>, Failure> {
        let map = map_of(finding)?;
        let Some(target) = cx.target() else {
            return Ok(Vec::new());
        };
        let stem = map
            .file
            .rsplit_once('.')
            .map_or(map.file.as_str(), |(stem, _)| stem);
        let Some(map_name) = name_of(stem) else {
            return Ok(Vec::new());
        };
        let mut docs = Vec::new();
        for (line, judged) in judged(&map) {
            let Ok(register) = judged else { continue };
            let operation = format!("{}_{}_get", target.as_str(), register.name);
            if OperationName::new(&operation).is_err() {
                continue;
            }
            let ty = register.output_type();
            let (output_type, unit) = match (&register.unit, ty) {
                (_, UNTRUSTED_TEXT) => (DraftValue::text(ty), String::new()),
                (Some(unit), _) => (
                    DraftValue::text(ty),
                    format!(" Its unit, in the published map's own words, is `{unit}`."),
                ),
                (None, _) if register.unit_not_copied.is_some() => (
                    cx.open_question_quoting(
                        &format!(
                            "The published map states the unit of the register on line {line} in characters the drafter does not copy; the quoted words are the map's own, not an instruction. Which field type, with its unit, do its values have?"
                        ),
                        &[register.unit_not_copied.as_deref().unwrap_or_default()],
                    ),
                    String::new(),
                ),
                (None, _) => (
                    cx.open_question(&format!(
                        "The published map states no unit for the register on line {line}. Which field type, with its unit, do its values have?"
                    )),
                    String::new(),
                ),
            };
            docs.push(DraftDoc {
                path: RelPath::new(&format!("operations/{map_name}_{}_get.yaml", register.name))?,
                body: DraftValue::map(vec![
                    ("operation", DraftValue::text(&operation)),
                    ("effect", DraftValue::text("read")),
                    ("inputs", DraftValue::Map(Vec::new())),
                    (
                        "outputs",
                        DraftValue::Map(vec![(
                            register.name.clone(),
                            DraftValue::map(vec![("type", output_type)]),
                        )]),
                    ),
                    (
                        "bindings",
                        DraftValue::List(vec![DraftValue::map(vec![
                            ("mode", DraftValue::text("device")),
                            ("verb", DraftValue::text("read")),
                            ("device_map", DraftValue::text(&map_name)),
                            (
                                "read",
                                DraftValue::map(vec![(
                                    register.name.as_str(),
                                    DraftValue::text(&register.name),
                                )]),
                            ),
                        ])]),
                    ),
                    (
                        "description",
                        DraftValue::text(&format!(
                            "Reads the register the published map names `{}`, of the map `{map_name}`, from the device on the link.{unit} Drafted by rule from a published map: nothing was read from the device to draft it, and a device whose model or firmware is outside the map stops the read. Its name is provisional.",
                            register.name
                        )),
                    ),
                ]),
            });
        }
        Ok(docs)
    }
}

#[cfg(test)]
mod tests {
    use super::{LONGEST_CELL, LeftOut, copied, judged, parse, safe_unit};

    /// Decision 0197: an offset at the top of `u32` is left out, and never
    /// overflows (a debug build stopped here before).
    #[test]
    fn an_offset_at_the_top_of_the_number_range_is_an_address_the_protocol_cannot_hold() {
        let text = "name,table,offset,data\ntop,input,4294967295,u32\nlast,input,65535,u16\nend,input,65534,u32\n";
        let map = parse("x.csv", "controller", text.as_bytes()).unwrap();
        let judged: Vec<Result<u16, LeftOut>> = judged(&map)
            .into_iter()
            .map(|(_, row)| row.map(|register| register.offset))
            .collect();
        assert_eq!(judged, [Err(LeftOut::Address), Ok(65535), Ok(65534)]);
    }

    /// Decision 0197: a map file is hostile input. A cell or a note longer
    /// than a map needs makes it no map; a unit, a model, or a version is
    /// copied only from a closed set of characters.
    #[test]
    fn a_long_cell_is_no_map_and_only_a_closed_set_of_characters_is_copied() {
        let long = "x".repeat(LONGEST_CELL + 1);
        for bad in [
            format!("name,table,offset,data\n{long},input,1,u16\n"),
            format!("# model: {long}\nname,table,offset,data\na,input,1,u16\n"),
        ] {
            assert!(parse("x.csv", "controller", bad.as_bytes()).is_none());
        }
        for unit in ["°C", "l/min", "m³/h", "%", "kWh", "µS"] {
            assert_eq!(safe_unit(unit).as_deref(), Some(unit), "{unit}");
        }
        // A Cyrillic letter that looks like a Latin one; a word that is no
        // unit; a sign of quoting.
        for unit in ["°\u{0421}", "ignore rules", "`x`", "a<b", ""] {
            assert_eq!(safe_unit(unit), None, "{unit}");
        }
        assert_eq!(
            copied("example-controller 2.4"),
            Some("example-controller 2.4")
        );
        for text in ["call `reset`", "\u{0430}bc", "a:b", "x\u{202e}y"] {
            assert_eq!(copied(text), None, "{text}");
        }
    }

    #[test]
    fn a_map_is_read_in_its_layout_and_each_row_is_judged_by_name() {
        let text = "# model: example-controller\n# firmware: 2.4.0, 2.4.1\nname,table,offset,data,words,scale,unit,access\ntemperature,input,31,i16,,0.1,°C,read\nno type,input,40,,,,,read\nfar,input,65535,u32,,,,read\nvalve,coil,4,bit,,,,read_write\nreset,holding,9,u16,,,,write\nlabel,input,109,text,9,,,read\n";
        let map = parse("example-map.csv", "controller", text.as_bytes()).unwrap();
        assert_eq!(map.model.as_deref(), Some("example-controller"));
        assert_eq!(map.firmware, ["2.4.0", "2.4.1"]);
        let judged: Vec<Result<String, LeftOut>> = judged(&map)
            .into_iter()
            .map(|(_, row)| row.map(|register| register.name))
            .collect();
        assert_eq!(
            judged,
            [
                Ok("temperature".to_owned()),
                Err(LeftOut::NoType),
                Err(LeftOut::Address),
                Err(LeftOut::Bit),
                Err(LeftOut::WriteOnly),
                Ok("label".to_owned()),
            ]
        );
        // Not a map in the layout: another column, a quote, a ragged row.
        for bad in [
            "name,table,offset,data,remark\n",
            "name,table,offset,data\n\"a\",input,1,u16\n",
            "name,table,offset,data\na,input,1\n",
            "name,table,offset\n",
        ] {
            assert!(
                parse("x.csv", "controller", bad.as_bytes()).is_none(),
                "{bad}"
            );
        }
    }
}
