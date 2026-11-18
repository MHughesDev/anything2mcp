//! Normalization and comparison of a record's values.
//!
//! The fields of a record are its operation's inputs that are not part of
//! its key, in declared order. Each is normalized by its field type on both
//! sides, the intended value and the value read, and the two are compared
//! as values: there is no float anywhere in a comparison.

use a2m_core::{
    Digest, Failure, FieldTypeIndex, InputIndex, OperationPlan, Sensitivity, Value, Values,
};

use crate::codes;
use crate::loaded::Loaded;

/// One field of a record.
#[derive(Debug, Clone)]
pub(crate) struct Field {
    pub name: String,
    pub field_type: FieldTypeIndex,
    pub sensitivity: Sensitivity,
}

/// The fields of the record an operation writes.
pub(crate) fn record_fields(loaded: &Loaded, operation: &OperationPlan) -> Vec<Field> {
    operation
        .inputs
        .iter()
        .enumerate()
        .filter(|(position, _)| {
            let index = u32::try_from(*position).map(InputIndex::new);
            !index.is_ok_and(|index| operation.identity.key.contains(&index))
        })
        .map(|(_, input)| Field {
            name: input.name.clone(),
            field_type: input.field_type,
            sensitivity: loaded.sensitivity(Some(input.field_type)),
        })
        .collect()
}

/// A value as it is compared and stored: itself, or, for a field labelled
/// restricted, the hash of its normalized form.
pub(crate) fn guarded(field: &Field, normalized: Value) -> Value {
    if field.sensitivity == Sensitivity::Restricted {
        Value::Text(Digest::of(&normalized.canonical_bytes()).to_string())
    } else {
        normalized
    }
}

/// The intended values of a record: the normalized inputs that are fields,
/// each as it is compared.
pub(crate) fn intended(fields: &[Field], inputs: &Values) -> Values {
    fields
        .iter()
        .filter_map(|field| {
            inputs
                .get(&field.name)
                .map(|value| (field.name.clone(), guarded(field, value.clone())))
        })
        .collect()
}

/// The values read from the system for the fields a record intends, each
/// normalized by its field type and as it is compared.
///
/// # Errors
///
/// `unknown_variant`, `field_not_read_back`, when the read gave no value
/// for a field the record intends: such a write could never be verified.
pub(crate) fn seen(
    loaded: &Loaded,
    fields: &[Field],
    intended: &Values,
    raw: &Values,
) -> Result<Values, Failure> {
    let mut out = Values::new();
    for field in fields.iter().filter(|field| intended.contains(&field.name)) {
        let Some(value) = raw.get(&field.name) else {
            return Err(codes::fail(
                &codes::row::FIELD_NOT_READ_BACK,
                format!(
                    "Reading the record gave no value for the field `{}`, so a write to it cannot be verified.",
                    field.name
                ),
                "Make the binding's read-back return every field the operation writes.",
            ));
        };
        let normalized = loaded
            .normalize(Some(field.field_type), value)
            .map_err(|failure| {
                codes::fail(
                    &codes::row::OUTPUT_UNEXPECTED,
                    format!(
                        "The value read for the field `{}` cannot be read as its type ({}).",
                        field.name,
                        failure.code()
                    ),
                    "Compare the field's type in the target files with what the system shows.",
                )
            })?;
        out.insert(field.name.clone(), guarded(field, normalized));
    }
    Ok(out)
}

/// How two sets of a record's values compare.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Comparison {
    /// Every field is equal after normalization.
    Equal,
    /// The named fields differ. Values go to evidence, never here.
    Differs(Vec<String>),
}

/// Compares two sets of normalized values field by field, in the record's
/// declared order.
pub(crate) fn compare(fields: &[Field], left: &Values, right: &Values) -> Comparison {
    let differing: Vec<String> = fields
        .iter()
        .filter(|field| left.get(&field.name) != right.get(&field.name))
        .map(|field| field.name.clone())
        .collect();
    if differing.is_empty() {
        Comparison::Equal
    } else {
        Comparison::Differs(differing)
    }
}
