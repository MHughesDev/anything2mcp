//! A source definition, loaded: the form `read` works from.
//!
//! The compiler in `a2m-files` writes a [`SourceSpec`] into the payload of
//! `a2m_core::SourcePlan`; this crate reads it back. The type and the types
//! it is made of are defined once, in `a2m-files`, beside the file formats
//! (decisions 0079 and 0121); this crate uses them and has no copy.

use a2m_core::{
    Failure, FieldBase, FieldTypeIndex, FieldTypePlan, Plan, PlanExpr, SourcePlan, ValueType,
};
use a2m_expr::{Normalizer, Program};
use a2m_files::{OPERATION_READER, SourceSpec};

use crate::error::definition;
use crate::model::ReaderName;

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
