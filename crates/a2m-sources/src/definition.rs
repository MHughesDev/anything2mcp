//! The compiled source definition: what a plan's `SourcePlan` carries in its
//! payload, and the loaded form `read` works from.
//!
//! The compiler in `a2m-files` writes a [`SourceSpec`] into the payload of
//! `a2m_core::SourcePlan`; this crate reads it back. The two share only the
//! data, so neither depends on the other.

use a2m_core::{
    Failure, FieldBase, FieldTypeIndex, FieldTypePlan, Plan, PlanExpr, SourcePlan, ValueType,
};
use a2m_expr::{Normalizer, Program};
use serde::{Deserialize, Serialize};

use crate::error::definition;
use crate::fingerprint::Fingerprint;
use crate::model::{ReaderName, ReaderOptions};

/// The reader name of a source whose rows are another operation's result:
/// no reader is registered under it, and its rows go to `from_rows`.
pub const OPERATION_READER: &str = "operation";

/// Where the data lies in an input. Always a name or header text, never a
/// coordinate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Locate {
    /// The whole document: a delimited file, or the list at the root of a
    /// JSON document. A workbook's data is never found this way.
    Document,
    /// A table or a defined name of a workbook, whose first row is the
    /// header row; or the list under that key of a JSON document.
    Named(String),
    /// The sheet with that name. The header row is the first row that holds
    /// every header the definition names.
    Sheet(String),
}

/// What is done with a hidden row or column inside the located data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HiddenRule {
    /// A hidden cell in the data is a problem: nobody saw what it holds.
    #[default]
    Reject,
    /// A hidden cell is read like any other.
    Include,
}

/// One version of a template: its name, and the fingerprint a file of that
/// version has.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateVersion {
    /// The version, as the source file states it.
    pub version: String,
    /// The fingerprint, with its parts, so that a file that matches no
    /// version can be told how it differs.
    pub fingerprint: Fingerprint,
}

/// One row field: its name, its field type, and where its value comes from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RowField {
    /// The row field's name.
    pub name: String,
    /// Its field type, by which its value is typed. Every row field has
    /// one, so every row field has a type before any file is read.
    pub field_type: FieldTypeIndex,
    /// True when a row without a value for it is a problem.
    pub required: bool,
    /// The header of the column it is read from. When absent, the value is
    /// the result of the expression of the same name in the plan's mapping.
    pub column: Option<String>,
}

/// What a source definition says beyond the plan's own fields: the payload
/// of `a2m_core::SourcePlan`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceSpec {
    /// The template versions the definition knows. Empty only for a source
    /// whose reader is `operation`.
    pub versions: Vec<TemplateVersion>,
    /// Where the data lies.
    pub locate: Locate,
    /// The header text of every column the definition reads, each once. A
    /// column in the data that is not listed here is a problem.
    pub columns: Vec<String>,
    /// The row fields, in declared order.
    pub fields: Vec<RowField>,
    /// The row fields whose values, together, identify a row. Two rows with
    /// the same values are a problem. Empty when rows have no key.
    pub key: Vec<String>,
    /// The texts that count as no value. Empty unless the source file lists
    /// some: text such as `NA` is text.
    pub missing: Vec<String>,
    /// What is done with hidden rows and columns.
    pub hidden: HiddenRule,
    /// The dialect the reader is told.
    pub options: ReaderOptions,
}

/// A field type, loaded: its base and its normalizer. This is what types a
/// cell, for `read` and for the file pack alike.
#[derive(Debug, Clone)]
pub struct FieldTyper {
    name: String,
    base: FieldBase,
    normalizer: Normalizer,
}

impl FieldTyper {
    /// Loads the field type at `index` of a plan.
    ///
    /// # Errors
    ///
    /// `source_definition_invalid` when the plan has no such field type or
    /// the field type has no normalizer; `plan_expression_unloadable` when
    /// the normalizer does not load.
    pub fn load(plan: &Plan, index: FieldTypeIndex) -> Result<Self, Failure> {
        let field_type = plan
            .field_type(index)
            .ok_or_else(|| definition("a row field names a field type the plan does not hold"))?;
        let normalizer = field_type
            .normalizer
            .and_then(|index| plan.expression(index))
            .ok_or_else(|| definition("a field type has no normalizer in the plan"))?;
        Self::from_parts(field_type, normalizer)
    }

    /// Loads a field type from its entry in a plan and its normalizer.
    ///
    /// # Errors
    ///
    /// `plan_expression_unloadable` when the normalizer does not load.
    pub fn from_parts(field_type: &FieldTypePlan, normalizer: &PlanExpr) -> Result<Self, Failure> {
        Ok(Self {
            name: field_type.name.clone(),
            base: field_type.base,
            normalizer: Normalizer::load(normalizer)?,
        })
    }

    /// The field type's name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// What the field type is built on.
    #[must_use]
    pub const fn base(&self) -> FieldBase {
        self.base
    }

    /// The type of a typed value.
    #[must_use]
    pub const fn value_type(&self) -> &ValueType {
        self.normalizer.value_type()
    }

    pub(crate) const fn normalizer(&self) -> &Normalizer {
        &self.normalizer
    }
}

/// Where a loaded row field's value comes from.
#[derive(Debug, Clone)]
pub(crate) enum Origin {
    Column(String),
    Expression(Program),
}

#[derive(Debug, Clone)]
pub(crate) struct LoadedField {
    pub(crate) name: String,
    pub(crate) typer: FieldTyper,
    pub(crate) required: bool,
    pub(crate) from: Origin,
}

/// A source definition, loaded from a plan once and used for every read:
/// its expressions parsed, its field types' normalizers ready.
#[derive(Debug, Clone)]
pub struct LoadedSource {
    template: String,
    reader: String,
    spec: SourceSpec,
    pub(crate) fields: Vec<LoadedField>,
    pub(crate) filter: Option<Program>,
}

impl LoadedSource {
    /// Loads the source of a pipeline from its plan.
    ///
    /// # Errors
    ///
    /// `payload_unreadable` when the payload is not a [`SourceSpec`];
    /// `source_definition_invalid` when the definition does not hold
    /// together: an index the plan lacks, a row field named twice, a column
    /// that is not listed, a mapping with no row field, a key that is not a
    /// row field, a fingerprint whose digest is not that of its parts, a
    /// file reader with no template version; `plan_expression_unloadable`
    /// when an expression does not load.
    pub fn load(plan: &Plan, source: &SourcePlan) -> Result<Self, Failure> {
        let spec: SourceSpec = source.payload.decode()?;
        if source.reader != OPERATION_READER {
            ReaderName::new(&source.reader)?;
            if spec.versions.is_empty() {
                return Err(definition(
                    "a source read from a file knows no template version",
                ));
            }
        }
        if spec.versions.iter().any(|v| !v.fingerprint.is_sound()) {
            return Err(definition(
                "a template version's digest is not the digest of its parts",
            ));
        }
        fn twice<'a>(names: impl Iterator<Item = &'a str>) -> bool {
            let mut seen: Vec<&str> = Vec::new();
            for name in names {
                if seen.contains(&name) {
                    return true;
                }
                seen.push(name);
            }
            false
        }
        if twice(spec.columns.iter().map(String::as_str)) {
            return Err(definition("a column is listed twice"));
        }
        if twice(spec.fields.iter().map(|field| field.name.as_str())) {
            return Err(definition("a row field is named twice"));
        }
        if twice(spec.versions.iter().map(|v| v.version.as_str())) {
            return Err(definition("a template version is listed twice"));
        }
        if twice(source.mapping.iter().map(|arg| arg.name.as_str())) {
            return Err(definition("a mapping is written twice"));
        }
        if spec.fields.is_empty() {
            return Err(definition("the source has no row field"));
        }
        let mut fields = Vec::with_capacity(spec.fields.len());
        for field in &spec.fields {
            let typer = FieldTyper::load(plan, field.field_type)?;
            let mapped = source.mapping.iter().find(|arg| arg.name == field.name);
            let from = match (&field.column, mapped) {
                (Some(column), None) => {
                    if !spec.columns.contains(column) {
                        return Err(definition("a row field reads a column that is not listed"));
                    }
                    Origin::Column(column.clone())
                }
                (None, Some(arg)) => {
                    let expression = plan.expression(arg.value).ok_or_else(|| {
                        definition("a mapping names an expression the plan does not hold")
                    })?;
                    Origin::Expression(Program::load(expression)?)
                }
                (Some(_), Some(_)) => {
                    return Err(definition(
                        "a row field has both a column and an expression",
                    ));
                }
                (None, None) => {
                    return Err(definition(
                        "a row field has neither a column nor an expression",
                    ));
                }
            };
            fields.push(LoadedField {
                name: field.name.clone(),
                typer,
                required: field.required,
                from,
            });
        }
        if source
            .mapping
            .iter()
            .any(|arg| !spec.fields.iter().any(|field| field.name == arg.name))
        {
            return Err(definition("a mapping is written for no row field"));
        }
        if spec
            .key
            .iter()
            .any(|key| !spec.fields.iter().any(|field| field.name == *key))
        {
            return Err(definition("a key names something that is not a row field"));
        }
        let filter = match source.rows {
            None => None,
            Some(index) => {
                let expression = plan.expression(index).ok_or_else(|| {
                    definition("the row filter names an expression the plan does not hold")
                })?;
                if expression.result != ValueType::Bool {
                    return Err(definition("the row filter does not give true or false"));
                }
                Some(Program::load(expression)?)
            }
        };
        Ok(Self {
            template: source.template.clone(),
            reader: source.reader.clone(),
            spec,
            fields,
            filter,
        })
    }

    /// The template's name, as the source file states it.
    #[must_use]
    pub fn template(&self) -> &str {
        &self.template
    }

    /// The reader's name.
    #[must_use]
    pub fn reader(&self) -> &str {
        &self.reader
    }

    /// What the definition says.
    #[must_use]
    pub const fn spec(&self) -> &SourceSpec {
        &self.spec
    }

    /// Whether any row field or the row filter is an expression, so that the
    /// record `row` has to be built.
    pub(crate) fn reads_rows(&self) -> bool {
        self.filter.is_some()
            || self
                .fields
                .iter()
                .any(|field| matches!(field.from, Origin::Expression(_)))
    }
}
