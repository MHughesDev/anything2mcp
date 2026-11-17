//! Sources: an input turned into validated records.
//!
//! A reader takes the bytes of one input, which its caller has already read
//! once and stored, and gives the parsed form: the input's structure and
//! every cell with the place of its value in the bytes. [`Sources`] then
//! checks the structure's fingerprint against the template versions a source
//! definition knows, finds the data by name or header text, applies the row
//! filter and the mapping, and types every row field by its field type. It
//! gives back every row, or every problem at once and no row.
//!
//! Nothing here opens a path, reaches a network, calculates a formula, runs
//! a macro, or reads a number through a float. The same bytes and the same
//! definition give the same result on every operating system.
//!
//! The built-in readers are this crate's own: delimited text, JSON, and
//! workbooks, the last over an outside crate that only inflates a package's
//! parts (decision 0086). The file pack reads files through the same parsed
//! form and the same [`typed`], so a value means one thing everywhere.
//!
//! The crates this one may depend on are set by the layering table in
//! `docs/reference/technical-reference.md`: `a2m-core`, `a2m-expr`, and
//! `a2m-files`, for the source definition's types.

mod csv;
mod definition;
mod error;
mod fingerprint;
pub mod json;
mod locate;
mod model;
pub mod number;
mod package;
mod read;
mod typing;
mod xlsx;
mod xml;

#[cfg(test)]
mod testing;
#[cfg(test)]
mod tests;

pub use csv::{CsvReader, field_text};
// The source definition's types have one home, `a2m-files`, where the
// compiler fills them. They are named here so that a caller of this crate
// needs no second import.
pub use a2m_files::{
    Fingerprint, FingerprintPart, HiddenRule, Locate, OPERATION_READER, PartKind, ReaderOptions,
    RowField, SourceSpec, TemplateVersion,
};
pub use definition::{FieldTyper, LoadedSource};
pub use error::{
    FILE_TYPE_MISMATCH, FORMULA_WITHOUT_VALUE, KEY_NOT_UNIQUE, Problem, Rejected, SCHEMA_INVALID,
    SOURCE_DEFINITION_INVALID, SOURCE_ENCODING_UNSUPPORTED, SOURCE_MALFORMED, SOURCE_PROTECTED,
    SOURCE_READER_UNKNOWN, SOURCE_TEMPLATE_UNKNOWN, SOURCE_TOO_LARGE, VALUE_OUT_OF_RANGE,
};
pub use fingerprint::{
    DefinedName, DropdownList, SheetStructure, Structure, TableStructure, differences,
};
pub use json::JsonReader;
pub use locate::{CellTable, TableColumn, locate};
pub use model::{
    ByteSpan, Cell, CellRange, CellRef, ContentKind, Limits, Parsed, ParsedPart, PartBytes,
    PlacedCell, RawCell, ReaderName, SourceInput, SourceReader,
};
pub use read::{Description, PartSize, SourceRow, Sources, ValidatedRecords};
pub use typing::{CellContext, typed};
pub use xlsx::XlsxReader;
