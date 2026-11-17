//! What the pack compiles `statements/` files to: its own section of the
//! plan, and the details it puts on each step. The kernel stores and hashes
//! both and never reads them; only this crate decodes them.
//!
//! Every map is ordered, and nothing here holds a float, so the same files
//! give the same bytes.

use std::collections::BTreeMap;

use a2m_core::ValueType;
use serde::{Deserialize, Serialize};

/// The version of the section's shape.
pub(crate) const SECTION_FORMAT: u16 = 1;

/// Whether a transaction may change data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Access {
    /// The store holds the connection read-only.
    ReadOnly,
    /// The transaction may write.
    ReadWrite,
}

/// A standard isolation level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Isolation {
    /// A statement sees only committed data.
    ReadCommitted,
    /// A transaction sees one snapshot.
    RepeatableRead,
    /// Transactions behave as if run one after another.
    Serializable,
}

impl Isolation {
    pub(crate) const NAMES: &'static [&'static str] =
        &["read_committed", "repeatable_read", "serializable"];

    pub(crate) fn from_name(name: &str) -> Option<Self> {
        match name {
            "read_committed" => Some(Self::ReadCommitted),
            "repeatable_read" => Some(Self::RepeatableRead),
            "serializable" => Some(Self::Serializable),
            _ => None,
        }
    }
}

/// How a stored value must be held for a field of a given type. The driver
/// refuses a value stored another way as drift.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Stored {
    /// Text: identifiers, text, choices, exact decimals, dates, and times.
    Text,
    /// A whole number.
    Whole,
    /// A whole number or text: a truth value, however the store writes it.
    Flag,
}

impl Stored {
    /// How a field of the type `ty` is stored.
    pub(crate) fn of(ty: &ValueType) -> Self {
        match ty {
            ValueType::Int | ValueType::Duration => Self::Whole,
            ValueType::Bool => Self::Flag,
            _ => Self::Text,
        }
    }
}

/// How many rows a statement gives back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Returns {
    /// One row, or none: none is "absent", two is a failure.
    OneOrNone,
    /// Up to `max_rows` rows.
    Many,
    /// No rows: a write the pack wrote.
    Nothing,
    /// The key the store assigned to the one row an insert made.
    AssignedKey,
}

/// One column of a result, and the field it fills.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Column {
    /// The column's name, as the store reports it.
    pub column: String,
    /// The name its value is returned under.
    pub field: String,
    /// How its value must be stored.
    pub stored: Stored,
    /// Whether it is part of the record's key, and its place in it from 1.
    pub key_position: u32,
}

/// One declared statement, compiled.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Statement {
    /// The surface it runs on.
    pub surface: String,
    /// Whether it may change data.
    pub access: Access,
    /// The statement, a constant of the plan. Nothing is ever placed in it.
    pub text: String,
    /// The parameters, in placeholder order, by the name of the step
    /// argument that fills each.
    pub params: Vec<String>,
    /// The columns its result may hold, in any order.
    pub columns: Vec<Column>,
    /// How many rows it gives back.
    pub returns: Returns,
    /// The most rows returned, for `many`.
    pub max_rows: u32,
    /// The table a write changes, for the check of triggers and cascading
    /// keys before it runs. Nothing for a read.
    pub table: Option<String>,
    /// Whether the target declared that the write reaches other data.
    pub hazard_declared: bool,
    /// The isolation level the target asked for.
    pub isolation: Isolation,
}

/// The pack's section of the plan: every statement, by name.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Section {
    /// The statements, by the name bindings use.
    pub statements: BTreeMap<String, Statement>,
}

/// What a surface's payload holds beside its address.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SurfaceFields {
    /// The kind of store.
    pub store: String,
    /// The named root the store's file is in.
    #[serde(default)]
    pub root: Option<String>,
}

/// The pack's details of one step. Which fields are set depends on the
/// step's kind.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StepDetail {
    /// `begin`: the access mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access: Option<Access>,
    /// `begin`: the isolation level.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub isolation: Option<Isolation>,
    /// `expect_rows`: the count the write must have changed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rows: Option<u64>,
}
