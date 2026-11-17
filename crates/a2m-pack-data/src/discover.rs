//! Discovery: the store's own catalog, read through the sandbox port, as
//! findings; and the describer that puts them so an agent can read them.
//!
//! Nothing is written, no row is read, and no row is counted. The catalog
//! is read by one step of the kind `catalog`, which the driver carries out
//! on the connection the store holds read-only.

use std::fmt::Write as _;
use std::sync::Arc;

use a2m_core::{
    Address, Coverage, EvidenceLevel, FailsAs, Failure, FailureClass, FailureCode, Finding,
    FindingKind, LimitationCode, PackPayload, ProducedBy, Source, Step, Subject, Value, Values,
};
use a2m_provider::{
    BoxFuture, DescribeForm, Describer, Description, Discoverer, DiscovererName, DiscoveryReport,
    DiscoveryScope, FindingSink, SandboxPort, StepInput, StepOutcome, SurfaceDescription,
};
use a2m_telemetry::Clock;
use serde::{Deserialize, Serialize};

use crate::model::SurfaceFields;
use crate::names::{CATALOG, PACK, PACK_VERSION, codes};
use crate::store::Catalog;

/// One column of a `table` finding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ColumnFound {
    /// The column's name.
    pub name: String,
    /// The type it was declared with, as the store writes it.
    pub declared_type: String,
    /// Whether it refuses a null.
    pub not_null: bool,
    /// Its place in the primary key, from 1; 0 when it is not part of it.
    pub key_position: u32,
}

/// One foreign key of a `table` finding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ForeignKeyFound {
    /// The columns of this table.
    pub columns: Vec<String>,
    /// The table they refer to.
    pub references: String,
    /// What a change of the referred key does here.
    pub on_update: String,
    /// What a deletion of the referred row does here.
    pub on_delete: String,
}

/// The `found` of a `table` finding: what the store's catalog says about
/// one table or view. This is the finding kind's schema, version 1.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TableFound {
    /// The surface the store is.
    pub surface: String,
    /// The kind of store, whose dialect quotes the names.
    pub store: String,
    /// The table's or view's name.
    pub table: String,
    /// Whether it is a view.
    pub view: bool,
    /// Its columns, in declared order.
    pub columns: Vec<ColumnFound>,
    /// Its unique constraints beside the primary key.
    pub unique: Vec<Vec<String>>,
    /// Its foreign keys.
    pub foreign_keys: Vec<ForeignKeyFound>,
    /// The triggers on it.
    pub triggers: Vec<String>,
    /// The rules that rewrite a write to it. Left out where there are none,
    /// so that a finding of the embedded store, which has none, is as before.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<String>,
    /// The columns the store computes from the row's others. Left out where
    /// there are none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub generated: Vec<String>,
    /// The tables whose rows change when a row of this one is changed or
    /// deleted, through a cascading key.
    pub cascades_to: Vec<String>,
    /// Whether check constraints were looked at. A store that does not
    /// report them leaves this false, and their absence means nothing.
    pub checks_seen: bool,
}

impl TableFound {
    /// Whether a write to the table reaches data beyond the rows it names.
    pub(crate) fn reaches_elsewhere(&self) -> bool {
        !self.triggers.is_empty()
            || !self.rules.is_empty()
            || !self.generated.is_empty()
            || !self.cascades_to.is_empty()
    }
}

fn texts(items: &[String]) -> Value {
    Value::List(
        items
            .iter()
            .map(|item| Value::from(item.as_str()))
            .collect(),
    )
}

/// The catalog as the values the `catalog` step returns.
pub(crate) fn catalog_values(catalog: &Catalog) -> Values {
    let tables = catalog
        .tables
        .iter()
        .map(|table| {
            let columns = table
                .columns
                .iter()
                .map(|column| {
                    Value::Record(
                        Values::new()
                            .with("name", column.name.as_str())
                            .with("declared_type", column.declared_type.as_str())
                            .with("not_null", column.not_null)
                            .with("key_position", i64::from(column.key_position)),
                    )
                })
                .collect();
            let foreign_keys = table
                .foreign_keys
                .iter()
                .map(|key| {
                    Value::Record(
                        Values::new()
                            .with("columns", texts(&key.columns))
                            .with("references", key.references.as_str())
                            .with("on_update", key.on_update.as_str())
                            .with("on_delete", key.on_delete.as_str())
                            .with("cascades", key.cascades()),
                    )
                })
                .collect();
            let mut record = Values::new();
            // Only where there are some: the embedded store has none.
            if !table.rules.is_empty() {
                record = record.with("rules", texts(&table.rules));
            }
            if !table.generated.is_empty() {
                record = record.with("generated", texts(&table.generated));
            }
            Value::Record(
                record
                    .with("name", table.name.as_str())
                    .with("view", table.view)
                    .with("columns", Value::List(columns))
                    .with(
                        "unique",
                        Value::List(table.unique.iter().map(|columns| texts(columns)).collect()),
                    )
                    .with("foreign_keys", Value::List(foreign_keys))
                    .with("triggers", texts(&table.triggers)),
            )
        })
        .collect();
    Values::new()
        .with("tables", Value::List(tables))
        .with("truncated", catalog.truncated)
}

fn text_of(record: &Values, name: &str) -> String {
    record
        .get(name)
        .and_then(Value::as_text)
        .unwrap_or_default()
        .to_owned()
}

fn texts_of(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_list)
        .unwrap_or_default()
        .iter()
        .filter_map(|item| item.as_text().map(str::to_owned))
        .collect()
}

fn records_of<'a>(record: &'a Values, name: &str) -> Vec<&'a Values> {
    record
        .get(name)
        .and_then(Value::as_list)
        .unwrap_or_default()
        .iter()
        .filter_map(Value::as_record)
        .collect()
}

/// The tables of a `catalog` step's values, as the content of findings.
fn tables_found(values: &Values, surface: &str, store: &str) -> Vec<TableFound> {
    let tables = records_of(values, "tables");
    let mut found: Vec<TableFound> = tables
        .iter()
        .map(|table| TableFound {
            surface: surface.to_owned(),
            store: store.to_owned(),
            table: text_of(table, "name"),
            view: table.get("view").and_then(Value::as_bool).unwrap_or(false),
            columns: records_of(table, "columns")
                .into_iter()
                .map(|column| ColumnFound {
                    name: text_of(column, "name"),
                    declared_type: text_of(column, "declared_type"),
                    not_null: column
                        .get("not_null")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    key_position: column
                        .get("key_position")
                        .and_then(Value::as_int)
                        .and_then(|position| u32::try_from(position).ok())
                        .unwrap_or(0),
                })
                .collect(),
            unique: table
                .get("unique")
                .and_then(Value::as_list)
                .unwrap_or_default()
                .iter()
                .map(|columns| texts_of(Some(columns)))
                .collect(),
            foreign_keys: records_of(table, "foreign_keys")
                .into_iter()
                .map(|key| ForeignKeyFound {
                    columns: texts_of(key.get("columns")),
                    references: text_of(key, "references"),
                    on_update: text_of(key, "on_update"),
                    on_delete: text_of(key, "on_delete"),
                })
                .collect(),
            triggers: texts_of(table.get("triggers")),
            rules: texts_of(table.get("rules")),
            generated: texts_of(table.get("generated")),
            cascades_to: Vec::new(),
            checks_seen: false,
        })
        .collect();
    // Which tables change when a row of each is changed or deleted.
    let cascades: Vec<(String, String)> = tables
        .iter()
        .flat_map(|table| {
            let name = text_of(table, "name");
            records_of(table, "foreign_keys")
                .into_iter()
                .filter(|key| key.get("cascades").and_then(Value::as_bool) == Some(true))
                .map(move |key| (text_of(key, "references"), name.clone()))
        })
        .collect();
    for table in &mut found {
        table.cascades_to = cascades
            .iter()
            .filter(|(referred, _)| crate::store::same_name(referred, &table.table))
            .map(|(_, dependent)| dependent.clone())
            .collect();
        table.cascades_to.sort();
        table.cascades_to.dedup();
    }
    found
}

/// A name as one segment of an address path: every byte that is not a
/// letter, a digit, `_`, `-`, or `.` is written as `%` and two hexadecimal
/// digits.
fn segment(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for byte in name.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.') {
            out.push(char::from(byte));
        } else {
            // Writing to a `String` cannot fail.
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}

fn invalid(message: &str) -> Failure {
    Failure::new(
        FailureClass::Input,
        FailureCode::FINDING_INVALID,
        message,
        "Report this to the pack's author.",
    )
}

/// The discoverer `catalog`.
pub(crate) struct CatalogDiscoverer {
    name: DiscovererName,
    kind: FindingKind,
    clock: Arc<dyn Clock>,
}

impl CatalogDiscoverer {
    pub(crate) fn new(kind: FindingKind, clock: Arc<dyn Clock>) -> Result<Self, Failure> {
        Ok(Self {
            name: DiscovererName::new("catalog")?,
            kind,
            clock,
        })
    }
}

impl Discoverer for CatalogDiscoverer {
    fn name(&self) -> DiscovererName {
        self.name.clone()
    }

    fn suits(&self, surface: &SurfaceDescription) -> bool {
        crate::names::is_database(&surface.kind)
    }

    fn discover<'a>(
        &'a self,
        port: &'a dyn SandboxPort,
        scope: &'a DiscoveryScope<'a>,
        out: &'a mut dyn FindingSink,
    ) -> BoxFuture<'a, Result<DiscoveryReport, Failure>> {
        Box::pin(async move {
            let surface = &scope.surface.surface;
            let fields: SurfaceFields = surface.payload.decode()?;
            let step = Step {
                kind: CATALOG,
                refers_to: None,
                args: Vec::new(),
                timeout_ms: scope.bounds.seconds.saturating_mul(1000),
                fails_as: FailsAs {
                    class: FailureClass::UnknownVariant,
                    code: codes::UNEXPECTED_STATUS,
                },
                commits: false,
                lookup: false,
                idempotent: false,
                when: None,
                payload: PackPayload::none(),
            };
            let args = Values::new();
            let input = StepInput::new(&args, scope.deadline);
            let observed = match port.act(&step, &input).await {
                Ok(StepOutcome::Done(observed) | StepOutcome::Absent(observed)) => observed,
                Err(failed) => return Err(failed.failure),
            };
            let truncated = observed
                .values
                .get("truncated")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let coverage = if truncated {
                Coverage::Partial
            } else {
                Coverage::Complete
            };
            let wanted = texts_of(scope.focus.get("tables"));
            let mut report = DiscoveryReport {
                coverage,
                bounds_hit: Vec::new(),
                dropped: 0,
            };
            let mut bytes: u64 = 0;
            for table in tables_found(&observed.values, surface.name.as_str(), &fields.store) {
                if !wanted.is_empty() && !wanted.contains(&table.table) {
                    report.dropped += 1;
                    continue;
                }
                let found = serde_json::to_string(&table).map_err(|_| {
                    invalid("A table of the catalog cannot be written as a finding.")
                })?;
                bytes += found.len() as u64;
                if bytes > scope.bounds.bytes {
                    report.coverage = Coverage::Partial;
                    report.bounds_hit = vec!["bytes".to_owned()];
                    report.dropped += 1;
                    continue;
                }
                let address = Address::new(
                    &format!("database://{}", surface.name),
                    &format!("/tables/{}", segment(&table.table)),
                )
                .map_err(|_| invalid("A table's name cannot be written as an address."))?;
                let mut finding = Finding::about(
                    Subject::Address(address),
                    self.kind.clone(),
                    scope.redact.redact(&found),
                    Source::LiveSystem,
                    ProducedBy {
                        provider: PACK.to_owned(),
                        version: PACK_VERSION.to_owned(),
                        component: None,
                    },
                    self.clock.now(),
                )
                .level(EvidenceLevel::Discovered)
                .coverage(coverage);
                if truncated {
                    finding = finding.limitation(
                        LimitationCode::Truncated,
                        "The catalog holds more tables than one read returns.",
                    );
                }
                let finding = finding
                    .build()
                    .map_err(|_| invalid("A finding about a table claims more than was seen."))?;
                out.found(finding)?;
            }
            Ok(report)
        })
    }
}

/// Puts `table` findings so that an agent can read them.
pub(crate) struct CatalogDescriber {
    pub(crate) kind: FindingKind,
}

impl Describer for CatalogDescriber {
    fn describe(&self, findings: &[Finding], form: DescribeForm) -> Description {
        let tables: Vec<TableFound> = findings
            .iter()
            .filter(|finding| finding.kind() == &self.kind)
            .filter_map(|finding| serde_json::from_str(finding.found()).ok())
            .collect();
        let views = tables.iter().filter(|table| table.view).count();
        let keyless = tables
            .iter()
            .filter(|table| {
                !table.view && table.columns.iter().all(|column| column.key_position == 0)
            })
            .count();
        let reaching = tables
            .iter()
            .filter(|table| table.reaches_elsewhere())
            .count();
        let count = |number: usize| i64::try_from(number).unwrap_or(i64::MAX);
        let mut description = Description {
            text: format!(
                "{} tables and {views} views. {keyless} tables declare no key, so no read by key is drafted for them. {reaching} have a trigger or a cascading key: a write to one reaches data beyond the rows it names.",
                tables.len() - views
            ),
            data: Values::new()
                .with("tables", count(tables.len() - views))
                .with("views", count(views))
                .with("without_key", count(keyless))
                .with("reach_elsewhere", count(reaching)),
            untrusted: Vec::new(),
        };
        if form == DescribeForm::Full {
            // Names are the store's own words: kept apart, never mixed into
            // the text above.
            for table in &tables {
                description.untrusted.push(table.table.clone());
                description
                    .untrusted
                    .extend(table.columns.iter().map(|column| column.name.clone()));
                description.untrusted.extend(table.triggers.iter().cloned());
            }
        }
        description
    }
}
