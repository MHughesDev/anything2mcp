//! The driver of the `file` mode: one step at a time, as written, with no
//! retry. `open` reads the file from disk; `locate` finds a named place;
//! `set` builds the new bytes and proves nothing else changed; `save`
//! replaces the file in one step; `read` gives the values found.
//!
//! The driver resolves no path. The surface's folder comes from the roots
//! the pack was handed when it registered, through the one check that makes
//! a `CheckedPath`; a file is one name in that folder, and is the regular
//! file under that name itself, never what a link at the name points to.

use std::collections::{BTreeMap, BTreeSet};

use a2m_auth::SessionLease;
use a2m_core::{
    Digest, Failure, FailureClass, FieldTypeIndex, FieldTypePlan, Plan, Step, Value, Values,
};
use a2m_expr::Normalizer;
use a2m_provider::{
    Access, BoxFuture, Cleanup, Driver, DriverHealth, Observed, Posture, PrepareContext, Prepared,
    RootName, Roots, StepFailure, StepInput, StepOutcome, SurfaceRef,
};
use a2m_sources::{FieldTyper, Limits};
use serde::Deserialize;

use crate::codes::{self, plan_unreadable, stop, unsupported};
use crate::edit::{self, Leads, Meant, Patch};
use crate::model::{
    BEFORE_DIGEST, Change, DEFAULT_ROOT, DOCUMENT_KIND, DocumentSpec, Expect, FILE_DIGEST,
    FieldSpec, Format, INTENDED_DIGEST, PACK, ROWS, RecordSpec, SURFACE_KIND, Section, StepSpec,
    TRUNCATED, field_arg, key_arg,
};
use crate::open::{self, Content, OpenFile, RowAt, Table};
use crate::place::{Folder, Named};
use crate::save::{self, Made, SaveJob};

/// What the pack may read: the limits of `packs.file`.
#[derive(Debug, Clone, Copy)]
pub struct FileLimits {
    /// The most bytes one file may have.
    pub max_bytes: u64,
    /// What parsing one file may use up.
    pub parse: Limits,
}

impl FileLimits {
    /// The limits a pack's settings state, with the manifest's defaults
    /// where a setting is absent.
    #[must_use]
    pub fn from_settings(settings: &Values) -> Self {
        let number = |key: &str, default: u64| match settings.get(key) {
            Some(Value::Int(number)) => u64::try_from(*number).unwrap_or(default),
            _ => default,
        };
        let small = |key: &str, default: u32| {
            u32::try_from(number(key, u64::from(default))).unwrap_or(default)
        };
        let max_bytes = number("max_bytes", 16 * 1024 * 1024);
        Self {
            max_bytes,
            parse: Limits {
                bytes: max_bytes,
                unpacked_bytes: max_bytes.saturating_mul(16),
                rows: small("max_rows", 200_000),
                cells: small("max_cells", 2_000_000),
                cell_bytes: small("max_cell_bytes", 65_536),
            },
        }
    }
}

/// The driver of the `file` mode.
pub struct FileDriver {
    roots: Roots,
    limits: FileLimits,
}

impl FileDriver {
    /// A driver over the roots the pack's manifest declares.
    #[must_use]
    pub fn new(roots: Roots, limits: FileLimits) -> Self {
        Self { roots, limits }
    }
}

/// How one field's values are typed, compared, and written.
struct Typing {
    typer: FieldTyper,
    normalizer: Normalizer,
    plan: FieldTypePlan,
}

/// One document of the surface, with its field types loaded.
struct Loaded {
    spec: DocumentSpec,
    typings: BTreeMap<u32, Typing>,
}

/// The place a `locate` step found.
struct Found {
    record: String,
    table: Table,
    /// The row with the key, when one has it.
    row: Option<usize>,
    /// The key that was looked for, normalized.
    key: Vec<Value>,
}

/// The file a binding is working on.
struct Current {
    document: String,
    path: Named,
    open: OpenFile,
    found: Option<Found>,
    /// The new bytes, once `set` has built and checked them.
    pending: Option<Vec<u8>>,
    intended: Option<Digest>,
    saved: bool,
}

/// What `prepare` acquired.
struct State {
    folder: Folder,
    documents: BTreeMap<String, Loaded>,
    run: String,
    /// What the run may do: a save is refused where nothing may be written.
    posture: Posture,
    current: Option<Current>,
    /// Temporary files this driver made and may not have removed.
    temporary: Vec<Made>,
}

/// The fields a `file` surface adds beside its folder.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct SurfaceFields {
    root: Option<String>,
}

fn timed_out() -> StepFailure {
    StepFailure::not_sent(Failure::new(
        FailureClass::Transient,
        a2m_provider::codes::TIMEOUT,
        "The step's deadline passed before the file was touched.",
        "Try again.",
    ))
}

fn out_of_order(what: &str) -> StepFailure {
    plan_unreadable(&format!("{what}, which no lowering of the pack gives"))
}

fn load(plan: &Plan, spec: DocumentSpec) -> Result<Loaded, StepFailure> {
    let mut typings = BTreeMap::new();
    for field in spec.records.values().flat_map(|record| &record.fields) {
        if typings.contains_key(&field.field_type) {
            continue;
        }
        let index = FieldTypeIndex::new(field.field_type);
        let loaded = plan.field_type(index).and_then(|field_type| {
            let normalizer = plan.expression(field_type.normalizer?)?;
            Some(Typing {
                typer: FieldTyper::from_parts(field_type, normalizer).ok()?,
                normalizer: Normalizer::load(normalizer).ok()?,
                plan: field_type.clone(),
            })
        });
        let Some(typing) = loaded else {
            return Err(plan_unreadable(
                "a field of a document names a field type that does not load",
            ));
        };
        typings.insert(field.field_type, typing);
    }
    Ok(Loaded { spec, typings })
}

impl Loaded {
    fn record(&self, name: &str) -> Result<&RecordSpec, StepFailure> {
        self.spec
            .records
            .get(name)
            .ok_or_else(|| plan_unreadable("a step names a record kind its document does not have"))
    }

    fn typing(&self, field_type: u32) -> Result<&Typing, StepFailure> {
        self.typings
            .get(&field_type)
            .ok_or_else(|| plan_unreadable("a field's type was not loaded"))
    }
}

/// The value an argument holds, as its field type normalizes it.
fn normalized(typing: &Typing, value: &Value) -> Result<Value, StepFailure> {
    if value.is_null() {
        return Ok(Value::Null);
    }
    typing
        .normalizer
        .normalize(value)
        .map(|normalized| normalized.normalized)
        .map_err(StepFailure::not_sent)
}

/// The key of one row, typed, or the first failure met while typing it.
fn key_of(
    loaded: &Loaded,
    record: &RecordSpec,
    table: &Table,
    row: usize,
    open: &OpenFile,
) -> Result<Vec<Value>, StepFailure> {
    let slots = table.rows.get(row).map(|row| row.slots.as_slice());
    record
        .key
        .iter()
        .map(|name| {
            let (position, field) = record
                .fields
                .iter()
                .enumerate()
                .find(|(_, field)| &field.name == name)
                .ok_or_else(|| plan_unreadable("a key names a field its record does not have"))?;
            let slot = slots
                .and_then(|slots| slots.get(position))
                .and_then(Option::as_ref);
            open::value_at(slot, field, &loaded.typing(field.field_type)?.typer, open)
        })
        .collect()
}

/// The row that holds `key`, when exactly one does. Two rows with one key,
/// anywhere in the table, are a failure: the file no longer tells its
/// records apart.
fn row_with_key(
    loaded: &Loaded,
    record: &RecordSpec,
    table: &Table,
    key: &[Value],
    open: &OpenFile,
) -> Result<Option<usize>, StepFailure> {
    let mut seen = BTreeSet::new();
    let mut found = None;
    for row in 0..table.rows.len() {
        let this = key_of(loaded, record, table, row, open)?;
        let canonical: Vec<u8> = this.iter().flat_map(Value::canonical_bytes).collect();
        if !seen.insert(canonical) {
            return Err(stop(
                codes::KEY_NOT_UNIQUE,
                "Two records of the file share one key, so they cannot be told apart.",
                "Remove the duplicate from the file, or declare a key that is unique.",
            ));
        }
        if this == key {
            found = Some(row);
        }
    }
    Ok(found)
}

/// The values of one row, by field name.
fn values_of(
    loaded: &Loaded,
    record: &RecordSpec,
    table: &Table,
    row: usize,
    open: &OpenFile,
) -> Result<Values, StepFailure> {
    let slots = table
        .rows
        .get(row)
        .map(|row| row.slots.as_slice())
        .unwrap_or_default();
    let mut values = Values::new();
    for (position, field) in record.fields.iter().enumerate() {
        let slot = slots.get(position).and_then(Option::as_ref);
        let value = open::value_at(slot, field, &loaded.typing(field.field_type)?.typer, open)?;
        values.insert(field.name.clone(), value);
    }
    Ok(values)
}

fn round_trip(why: &str) -> StepFailure {
    stop(
        codes::ROUND_TRIP_FAILED,
        format!(
            "The change cannot be made without touching something else in the file: {why}. Nothing was written."
        ),
        "Report this with the file's layout; until the editor covers it, the file is read-only.",
    )
}

fn delimiter_of(spec: &DocumentSpec) -> u8 {
    spec.delimiter
        .as_deref()
        .and_then(|text| text.bytes().next())
        .unwrap_or(b',')
}

/// The token in a temporary file's name: sixteen characters from the
/// digest of the record's idempotency token, so that the name says which
/// record it was for and holds nothing a path could misread. The name is
/// not made of the token alone: see `save`.
fn token(stamp: &str) -> String {
    Digest::of(stamp.as_bytes())
        .to_string()
        .chars()
        .take(16)
        .collect()
}

impl FileDriver {
    fn folder(&self, surface: &SurfaceRef) -> Result<Folder, StepFailure> {
        let payload = &surface.surface.payload;
        let fields: SurfaceFields = if payload.is_none() {
            SurfaceFields::default()
        } else {
            payload
                .decode()
                .map_err(|_| plan_unreadable("the surface's fields do not decode"))?
        };
        let root = RootName::new(fields.root.as_deref().unwrap_or(DEFAULT_ROOT))
            .map_err(StepFailure::not_sent)?;
        let address = surface.surface.address.as_str();
        let under = |access| {
            if address == "." {
                self.roots.folder(&root, access)
            } else {
                self.roots.check(&root, address, access)
            }
        };
        // A root the pack may write gives a folder it may write; a root it
        // may only read gives one it may only read.
        let folder = under(Access::Write)
            .or_else(|_| under(Access::Read))
            .map_err(StepFailure::not_sent)?;
        let root = self
            .roots
            .folder(&root, Access::Read)
            .map_err(StepFailure::not_sent)?;
        Ok(Folder::new(folder, &root))
    }

    async fn open(
        &self,
        state: &mut State,
        document: &str,
        for_write: bool,
    ) -> Result<Observed, StepFailure> {
        state.current = None;
        let loaded = state
            .documents
            .get(document)
            .ok_or_else(|| plan_unreadable("a step names a document the surface does not have"))?;
        if for_write {
            // What the compiler refuses, the driver refuses too.
            may_be_written(&loaded.spec)?;
        }
        let max_bytes = self.limits.max_bytes;
        let folder = state.folder.clone();
        let name = loaded.spec.file.clone();
        let sibling = loaded.spec.lock_sibling.clone().filter(|_| for_write);
        let read = move || {
            let path = folder.file(&name)?;
            if for_write {
                folder.reached_by_no_link()?;
            }
            if let Some(sibling) = sibling
                && folder.holds(&sibling)?
            {
                return Err(StepFailure::not_sent(open::locked()));
            }
            let bytes = open::read_file(&path, max_bytes)?;
            Ok((path, bytes))
        };
        let (path, bytes) = tokio::task::spawn_blocking(read)
            .await
            .map_err(|_| StepFailure::not_sent(open::unavailable("could not be read")))??;
        let opened = open::parse(bytes, &loaded.spec, &self.limits.parse)?;
        open::check_layout(&opened, &loaded.spec.layout)?;
        let digest = opened.digest;
        state.current = Some(Current {
            document: document.to_owned(),
            path,
            open: opened,
            found: None,
            pending: None,
            intended: None,
            saved: false,
        });
        Ok(Observed {
            for_reconcile: Values::new().with(FILE_DIGEST, digest.to_string()),
            ..Observed::default()
        })
    }
}

/// Whether a write to the document's file may begin: the document says
/// whether an application may hold the file open, and, where one may, names
/// the sign that it does. A plan that lacks either was not compiled by this
/// pack's rules, and its write is refused here as it is there.
fn may_be_written(spec: &DocumentSpec) -> Result<(), StepFailure> {
    match (spec.may_be_open, &spec.lock_sibling) {
        (None, _) => Err(plan_unreadable(
            "a write to a document that does not say whether an application may hold its file open",
        )),
        (Some(true), None) => Err(plan_unreadable(
            "a write to a document whose file may be held open, with no lock signal declared",
        )),
        _ => Ok(()),
    }
}

fn current<'a>(state: &'a mut State, document: &str) -> Result<&'a mut Current, StepFailure> {
    state
        .current
        .as_mut()
        .filter(|current| current.document == document)
        .ok_or_else(|| out_of_order("a step before its file was opened"))
}

fn locate(
    loaded: &Loaded,
    current: &mut Current,
    record_name: &str,
    expect: Expect,
    step: &Step,
    input: &StepInput<'_>,
) -> Result<StepOutcome, StepFailure> {
    let record = loaded.record(record_name)?;
    let table = open::table(&current.open, record)?;
    let mut key = Vec::new();
    if expect != Expect::All {
        for name in &record.key {
            let field = record
                .field(name)
                .ok_or_else(|| plan_unreadable("a key names a field its record does not have"))?;
            let given = input
                .args
                .get(&key_arg(name))
                .ok_or_else(|| plan_unreadable("a `locate` step lacks a key field's value"))?;
            key.push(normalized(loaded.typing(field.field_type)?, given)?);
        }
    }
    let row = if record.key.is_empty() {
        (!table.rows.is_empty()).then_some(0)
    } else {
        // Typing every key also proves no key is there twice.
        row_with_key(loaded, record, &table, &key, &current.open)?
    };
    let outcome = match (expect, row) {
        (Expect::All, _) | (Expect::Present, Some(_)) | (Expect::Absent, None) => {
            StepOutcome::Done(Observed::default())
        }
        (Expect::Present, None) if step.lookup => StepOutcome::Absent(Observed::default()),
        (Expect::Present, None) => {
            return Err(stop(
                codes::PLACE_NOT_FOUND,
                "No record of the file has the key given.",
                "Check the key, or read the records the file holds.",
            ));
        }
        (Expect::Absent, Some(_)) => {
            return Err(stop(
                codes::CHANGED_SINCE_PREVIEW,
                "A record with this key is in the file already, so none was added.",
                "Read the record, look at the new preview, and approve again.",
            ));
        }
    };
    current.found = Some(Found {
        record: record_name.to_owned(),
        table,
        row: if expect == Expect::All { None } else { row },
        key,
    });
    Ok(outcome)
}

fn found<'a>(current: &'a Current, record: &str) -> Result<&'a Found, StepFailure> {
    current
        .found
        .as_ref()
        .filter(|found| found.record == record)
        .ok_or_else(|| out_of_order("a step before its place was located"))
}

fn read_one(
    loaded: &Loaded,
    current: &Current,
    record_name: &str,
) -> Result<Observed, StepFailure> {
    let record = loaded.record(record_name)?;
    let found = found(current, record_name)?;
    let row = found
        .row
        .ok_or_else(|| out_of_order("a read of a record that was not found"))?;
    Ok(Observed {
        values: values_of(loaded, record, &found.table, row, &current.open)?,
        ..Observed::default()
    })
}

fn read_list(
    loaded: &Loaded,
    current: &Current,
    record_name: &str,
    limit: u32,
) -> Result<Observed, StepFailure> {
    let record = loaded.record(record_name)?;
    let found = found(current, record_name)?;
    if !record.key.is_empty() {
        row_with_key(loaded, record, &found.table, &[], &current.open)?;
    }
    let limit = limit as usize;
    let rows = (0..found.table.rows.len().min(limit))
        .map(|row| values_of(loaded, record, &found.table, row, &current.open).map(Value::Record))
        .collect::<Result<Vec<Value>, StepFailure>>()?;
    Ok(Observed {
        values: Values::new()
            .with(ROWS, Value::List(rows))
            .with(TRUNCATED, found.table.rows.len() > limit),
        ..Observed::default()
    })
}

/// The new value of each field a step sets, by the field's position:
/// normalized, as its field type compares it.
fn intended_values(
    loaded: &Loaded,
    record: &RecordSpec,
    input: &StepInput<'_>,
    with_key: Option<&[Value]>,
) -> Result<Vec<(usize, Value)>, StepFailure> {
    let mut values = Vec::new();
    for (position, field) in record.fields.iter().enumerate() {
        let key_position = record.key.iter().position(|name| name == &field.name);
        if let (Some(key), Some(at)) = (with_key, key_position) {
            values.push((position, key.get(at).cloned().unwrap_or(Value::Null)));
        } else if let Some(given) = input.args.get(&field_arg(&field.name)) {
            if !field.writable {
                return Err(plan_unreadable(
                    "a `set` step names a field that is not writable",
                ));
            }
            values.push((
                position,
                normalized(loaded.typing(field.field_type)?, given)?,
            ));
        }
    }
    Ok(values)
}

/// Refuses text that a spreadsheet program would run as a formula when a
/// person opens the delimited file. The value is never changed to make it
/// safe: what is read back must be what was written. A field whose document
/// says `may_begin_formula: true` may begin with a sign; nothing may begin
/// with a tab or a line break. The message names the field and never the
/// value.
fn formula_guard(field: &FieldSpec, value: &Value, text: &str) -> Result<(), StepFailure> {
    // A number is spelled by the pack itself: digits, a mark, and a sign.
    if matches!(value, Value::Int(_) | Value::Decimal(_)) {
        return Ok(());
    }
    let refused = |how: &str, next: &str| {
        Err(StepFailure::not_sent(codes::failure(
            codes::VALUE_BEGINS_FORMULA,
            format!(
                "The value for the field `{}` begins with {how}, so a spreadsheet program that opens the file would not keep it as text. Nothing was written.",
                field.name
            ),
            next,
        )))
    };
    match edit::leads_as_formula(text) {
        None => Ok(()),
        Some(Leads::Sign) if field.may_begin_formula => Ok(()),
        Some(Leads::Sign) => refused(
            "`=`, `+`, `-`, or `@`",
            "Give a value that begins otherwise. If the column is free text that may begin so, and no spreadsheet program opens the file, mark the field `may_begin_formula: true` in the document.",
        ),
        Some(Leads::Control) => refused(
            "a tab or a line break",
            "Give a value that begins with neither.",
        ),
    }
}

#[allow(clippy::too_many_lines)] // one change after another, each a few lines; the order is the algorithm
fn set(
    loaded: &Loaded,
    current: &mut Current,
    record_name: &str,
    change: Change,
    input: &StepInput<'_>,
    limits: &FileLimits,
) -> Result<Observed, StepFailure> {
    let spec = &loaded.spec;
    if !spec.format.writable() {
        return Err(unsupported("writing a format that has no patch editor"));
    }
    let record = loaded.record(record_name)?;
    let place = found(current, record_name)?;
    let table = &place.table;
    let old = &current.open;
    let delimiter = delimiter_of(spec);
    let part = match &old.content {
        Content::Cells(parsed) => parsed.part(0),
        Content::Json { .. } => None,
    };
    let values = intended_values(
        loaded,
        record,
        input,
        (change == Change::Create).then_some(place.key.as_slice()),
    )?;
    let mut patches: Vec<Patch> = Vec::new();
    let meant = match change {
        Change::Update => {
            let row_index = place
                .row
                .ok_or_else(|| out_of_order("a `set` on a record that was not found"))?;
            let row = table
                .rows
                .get(row_index)
                .ok_or_else(|| out_of_order("a `set` on a row the table does not have"))?;
            let mut cells = Vec::new();
            let mut nodes = Vec::new();
            for (position, intended) in &values {
                let field = &record.fields[*position];
                let typing = loaded.typing(field.field_type)?;
                let slot = row.slots.get(*position).and_then(Option::as_ref);
                // A place that already agrees is left exactly as it is
                // spelled.
                if open::value_at(slot, field, &typing.typer, old)? == *intended {
                    continue;
                }
                let Some(span) = slot.and_then(|slot| slot.span) else {
                    return Err(stop(
                        codes::PLACE_NOT_FOUND,
                        format!(
                            "The record has no place for the field `{}` to be set at.",
                            field.name
                        ),
                        "Add the member to the record in the file, or do not set the field.",
                    ));
                };
                let existing = span.of(&old.bytes).unwrap_or_default();
                let literal = if spec.format == Format::Json {
                    nodes.push(span);
                    edit::json_literal(intended, &typing.plan, Some(existing))?
                } else {
                    if let (RowAt::Line(line), Some(column)) =
                        (row.at, table.columns.get(*position))
                    {
                        cells.push((line, *column));
                    }
                    let text = edit::spell(intended, &typing.plan)?.unwrap_or_default();
                    formula_guard(field, intended, &text)?;
                    edit::csv_field(&text, existing.first() == Some(&b'"'), delimiter)
                };
                patches.push(Patch::over(span, literal));
            }
            if spec.format == Format::Json {
                Meant::Nodes(nodes)
            } else {
                Meant::Cells(cells)
            }
        }
        Change::Delete => {
            let row_index = place
                .row
                .ok_or_else(|| out_of_order("a `set` on a record that was not found"))?;
            match (
                table.rows.get(row_index).map(|row| row.at),
                &table.list,
                part,
            ) {
                (Some(RowAt::Line(line)), _, Some(part)) => {
                    patches.push(
                        edit::csv_remove_row(&old.bytes, part, line)
                            .ok_or_else(|| round_trip("the row's bytes cannot be found"))?,
                    );
                    Meant::RowRemoved(line)
                }
                (Some(RowAt::Element(index)), Some(list), _) => {
                    patches.push(
                        edit::json_remove(list, index)
                            .ok_or_else(|| round_trip("the element's bytes cannot be found"))?,
                    );
                    Meant::ElementRemoved {
                        list: list.span,
                        index,
                    }
                }
                _ => {
                    return Err(unsupported(
                        "removing a record that is not a row or a list element",
                    ));
                }
            }
        }
        Change::Create => {
            if place.row.is_some() {
                return Err(out_of_order("a create on a record that is there"));
            }
            match (&table.list, part) {
                (Some(list), _) => {
                    let mut members = Vec::new();
                    for (position, value) in &values {
                        let field = &record.fields[*position];
                        let [name] = field.path.as_slice() else {
                            return Err(unsupported(
                                "adding a record whose field lies deeper than one key",
                            ));
                        };
                        let typing = loaded.typing(field.field_type)?;
                        members
                            .push((name.clone(), edit::json_literal(value, &typing.plan, None)?));
                    }
                    patches.push(
                        edit::json_add(&old.bytes, list, &members)
                            .ok_or_else(|| round_trip("the list's bytes cannot be found"))?,
                    );
                    Meant::ElementAdded { list: list.span }
                }
                (None, Some(part)) => {
                    let mut texts = Vec::new();
                    for (position, value) in &values {
                        let field = &record.fields[*position];
                        let typing = loaded.typing(field.field_type)?;
                        if let (Some(column), Some(text)) = (
                            table.columns.get(*position),
                            edit::spell(value, &typing.plan)?,
                        ) {
                            formula_guard(field, value, &text)?;
                            texts.push((*column, text));
                        }
                    }
                    patches.push(edit::csv_add_row(
                        &old.bytes, part, spec, table, &texts, delimiter,
                    )?);
                    Meant::RowAdded
                }
                _ => {
                    return Err(unsupported(
                        "adding a record to something that is not rows or a list",
                    ));
                }
            }
        }
    };
    let bytes = edit::apply(&old.bytes, patches)
        .ok_or_else(|| round_trip("two of its changes meet in the same bytes"))?;
    if bytes.len() as u64 > limits.max_bytes {
        return Err(stop(
            codes::FILE_TOO_LARGE,
            "The file would grow beyond the most bytes the file pack writes.",
            "Raise `packs.file.max_bytes`.",
        ));
    }
    // The untouched check: parse the new bytes, compare every other place,
    // and read the named places back from the bytes about to be saved.
    let new = open::parse(bytes, spec, &limits.parse)
        .map_err(|_| round_trip("the changed file no longer parses"))?;
    if !edit::untouched(old, &new, &meant) {
        return Err(round_trip("a place that was not named would change"));
    }
    let after =
        open::table(&new, record).map_err(|_| round_trip("the record kind is no longer found"))?;
    let row_after = if record.key.is_empty() {
        Some(0)
    } else {
        row_with_key(loaded, record, &after, &place.key, &new)
            .map_err(|_| round_trip("the keys of the changed file cannot be read"))?
    };
    match (change, row_after) {
        (Change::Delete, None) => {}
        (Change::Update | Change::Create, Some(row)) => {
            let written = values_of(loaded, record, &after, row, &new)
                .map_err(|_| round_trip("a written value cannot be read back"))?;
            for (position, intended) in &values {
                if written.get(&record.fields[*position].name) != Some(intended) {
                    return Err(round_trip(
                        "a written value would not read back as intended",
                    ));
                }
            }
        }
        _ => {
            return Err(round_trip(
                "the record would not be where it was meant to be",
            ));
        }
    }
    let before = old.digest;
    let intended = new.digest;
    current.pending = Some(new.bytes);
    current.intended = Some(intended);
    Ok(Observed {
        for_reconcile: Values::new()
            .with(BEFORE_DIGEST, before.to_string())
            .with(INTENDED_DIGEST, intended.to_string()),
        ..Observed::default()
    })
}

impl FileDriver {
    async fn save(
        &self,
        state: &mut State,
        document: &str,
        step: &Step,
        input: &StepInput<'_>,
    ) -> Result<Observed, StepFailure> {
        // The driver's own guard. Live, the step the plan marks `commits`.
        // In a sandbox with test records, that one step alone: the engine
        // cannot see what a step not marked does there. Nowhere else.
        let allowed = match state.posture {
            Posture::Live => step.commits,
            Posture::Sandbox { test_records } => test_records && step.commits,
            _ => false,
        };
        if !allowed {
            return Err(StepFailure::not_sent(Failure::new(
                FailureClass::Policy,
                a2m_provider::codes::READ_ONLY_POSTURE,
                "A save was asked for where nothing may be written, or by a step that is not marked as the commit point, so the file was not replaced.",
                "Run the operation live, or against a declared test record, with the save as the binding's commit point.",
            )));
        }
        let loaded = state
            .documents
            .get(document)
            .ok_or_else(|| plan_unreadable("a step names a document the surface does not have"))?;
        let current = state
            .current
            .as_mut()
            .filter(|current| current.document == document)
            .ok_or_else(|| out_of_order("a save before its file was opened"))?;
        let (Some(bytes), Some(intended)) = (current.pending.take(), current.intended) else {
            return Err(out_of_order("a save with nothing set"));
        };
        may_be_written(&loaded.spec)?;
        if current.path.folder().access() != Access::Write {
            return Err(StepFailure::not_sent(Failure::new(
                FailureClass::Policy,
                a2m_provider::codes::EFFECT_NOT_DECLARED,
                "The surface's folder is in a root the file pack may read and not write.",
                "Put the file in a root named `files` that the settings make writable.",
            )));
        }
        let stamp = input.stamp.unwrap_or(&state.run);
        let job = SaveJob {
            target: current.path.clone(),
            name: loaded.spec.file.clone(),
            token: token(stamp),
            lock_signal: loaded.spec.lock_sibling.clone(),
            bytes,
            before: current.open.digest,
            intended,
            max_bytes: self.limits.max_bytes,
        };
        // The replace is never left half asked: the work runs to its end on
        // its own thread, and a lost answer is a write whose fate is unknown.
        let saved = tokio::task::spawn_blocking(move || save::save(&job))
            .await
            .map_err(|_| {
                StepFailure::unknown(open::unavailable(
                    "was being replaced when the work stopped",
                ))
            })?;
        state.temporary.extend(saved.left);
        saved.outcome?;
        current.saved = true;
        Ok(Observed::default())
    }

    async fn confirm(&self, state: &mut State, document: &str) -> Result<Observed, StepFailure> {
        let current = current(state, document)?;
        let (true, Some(intended)) = (current.saved, current.intended) else {
            return Err(out_of_order("a confirm with nothing saved"));
        };
        let (path, max_bytes) = (current.path.clone(), self.limits.max_bytes);
        let now = tokio::task::spawn_blocking(move || path.digest(max_bytes))
            .await
            .ok()
            .flatten();
        if now == Some(intended) {
            Ok(Observed::default())
        } else {
            Err(StepFailure::answered(codes::failure(
                codes::CHANGED_SINCE_PREVIEW,
                "The file at the path is not the one that was saved: another writer changed it since.",
                "Read the record back to see what the file holds now.",
            )))
        }
    }
}

impl Driver for FileDriver {
    fn prepare<'a>(
        &'a self,
        cx: &'a PrepareContext<'a>,
        _lease: Option<&'a SessionLease>,
    ) -> BoxFuture<'a, Result<Prepared, StepFailure>> {
        Box::pin(async move {
            if cx.deadline.passed() {
                return Err(timed_out());
            }
            if cx.surface.surface.kind != SURFACE_KIND {
                return Err(plan_unreadable("the surface is not of the kind `file`"));
            }
            let folder = self.folder(cx.surface)?;
            let section = cx.plan.packs.iter().find(|section| {
                section.pack == PACK
                    && section.kind == DOCUMENT_KIND
                    && section.surface_kind == SURFACE_KIND
            });
            let mut documents = BTreeMap::new();
            if let Some(section) = section {
                let decoded: Section = section
                    .payload
                    .decode()
                    .map_err(|_| plan_unreadable("its section does not decode"))?;
                for (name, spec) in decoded {
                    if spec.surface == cx.surface.surface.name.as_str() {
                        documents.insert(name, load(cx.plan, spec)?);
                    }
                }
            }
            Ok(Prepared::new(State {
                folder,
                documents,
                run: cx.run.to_string(),
                posture: cx.posture,
                current: None,
                temporary: Vec::new(),
            }))
        })
    }

    fn act<'a>(
        &'a self,
        prepared: &'a mut Prepared,
        step: &'a Step,
        input: &'a StepInput<'a>,
    ) -> BoxFuture<'a, Result<StepOutcome, StepFailure>> {
        Box::pin(async move {
            if input.deadline.passed() {
                return Err(timed_out());
            }
            let state = prepared
                .state::<State>()
                .ok_or_else(|| plan_unreadable("the driver was prepared by another provider"))?;
            let spec: StepSpec = step
                .payload
                .decode()
                .map_err(|_| plan_unreadable("a step's payload does not decode"))?;
            let document = step
                .refers_to
                .as_ref()
                .map(|refers| refers.entry.as_str())
                .ok_or_else(|| plan_unreadable("a step names no document"))?;
            let done = |observed: Observed| Ok(StepOutcome::Done(observed));
            match spec {
                StepSpec::Open { for_write } => done(self.open(state, document, for_write).await?),
                StepSpec::Save => done(self.save(state, document, step, input).await?),
                StepSpec::Confirm => done(self.confirm(state, document).await?),
                other => {
                    let loaded = state.documents.get(document).ok_or_else(|| {
                        plan_unreadable("a step names a document the surface does not have")
                    })?;
                    let current = state
                        .current
                        .as_mut()
                        .filter(|current| current.document == document)
                        .ok_or_else(|| out_of_order("a step before its file was opened"))?;
                    match other {
                        StepSpec::Locate { record, expect } => {
                            locate(loaded, current, &record, expect, step, input)
                        }
                        StepSpec::One { record } => done(read_one(loaded, current, &record)?),
                        StepSpec::List { record, limit } => {
                            done(read_list(loaded, current, &record, limit)?)
                        }
                        StepSpec::Set { record, change } => {
                            done(set(loaded, current, &record, change, input, &self.limits)?)
                        }
                        StepSpec::Open { .. } | StepSpec::Save | StepSpec::Confirm => {
                            Err(out_of_order("a step of no kind"))
                        }
                    }
                }
            }
        })
    }

    fn health(&self, _surface: &SurfaceRef) -> DriverHealth {
        if self.roots.names().next().is_some() {
            DriverHealth::Ready
        } else {
            // No root the pack declares is set on this machine.
            DriverHealth::Unavailable(a2m_provider::codes::PATH_NOT_ALLOWED)
        }
    }

    fn release<'a>(&'a self, prepared: Prepared, _poisoned: bool) -> BoxFuture<'a, Cleanup> {
        Box::pin(async move {
            // Only what this driver made: a temporary file is removed when
            // what is at its name is still, by its identity, the file a
            // save of this driver made.
            if let Ok(state) = prepared.into_state::<State>() {
                let _ = tokio::task::spawn_blocking(move || {
                    for temporary in &state.temporary {
                        temporary.remove();
                    }
                })
                .await;
            }
            Cleanup::complete()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Layout;

    fn document(may_be_open: Option<bool>, lock_sibling: Option<&str>) -> DocumentSpec {
        DocumentSpec {
            surface: "price_files".to_owned(),
            file: "stock.csv".to_owned(),
            format: Format::Delimited,
            delimiter: None,
            line_ending: None,
            layout: Layout::default(),
            may_be_open,
            lock_sibling: lock_sibling.map(str::to_owned),
            records: BTreeMap::new(),
        }
    }

    #[test]
    fn a_write_to_a_document_that_does_not_say_whether_it_may_be_open_does_not_run() {
        // The compiler refuses both; a plan that holds either all the same
        // is refused by the driver before the file is read.
        for spec in [document(None, None), document(None, Some("stock.csv.lock"))] {
            let failure = may_be_written(&spec).unwrap_err();
            assert_eq!(failure.failure.code(), &codes::PAYLOAD_UNREADABLE);
            assert_eq!(failure.sent, a2m_provider::Sent::NotSent);
        }
        assert!(may_be_written(&document(Some(true), None)).is_err());
        assert!(may_be_written(&document(Some(false), None)).is_ok());
        assert!(may_be_written(&document(Some(true), Some("stock.csv.lock"))).is_ok());
    }

    #[test]
    fn a_number_the_pack_spells_is_not_taken_for_a_formula() {
        let field = |may_begin_formula| FieldSpec {
            name: "note".to_owned(),
            header: Some("note".to_owned()),
            path: Vec::new(),
            field_type: 0,
            writable: true,
            formula: false,
            may_begin_formula,
        };
        assert!(formula_guard(&field(false), &Value::Int(-5), "-5").is_ok());
        assert!(formula_guard(&field(false), &Value::from("-5"), "-5").is_err());
        assert!(formula_guard(&field(true), &Value::from("-5"), "-5").is_ok());
        // A declaration lets a sign through, and never a tab or a line break.
        let stopped = formula_guard(&field(true), &Value::from("\t=1"), "\t=1").unwrap_err();
        assert_eq!(stopped.failure.code(), &codes::VALUE_BEGINS_FORMULA);
        assert!(!stopped.failure.message().contains("=1"));
    }
}
