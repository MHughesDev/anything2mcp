//! Small helpers for reading what the store holds and for digests over
//! several fields.

use std::str::FromStr;

use a2m_core::{Digest, Failure, Timestamp};

use crate::failure;

/// Reads a stored name or identifier back. Text the store wrote and can no
/// longer read is a damaged store, not bad input.
pub(crate) fn parse<T: FromStr>(text: &str) -> Result<T, Failure> {
    text.parse()
        .map_err(|_| failure::corrupt("a stored name or identifier that no longer reads"))
}

pub(crate) fn parse_opt<T: FromStr>(text: Option<&str>) -> Result<Option<T>, Failure> {
    text.map(parse).transpose()
}

pub(crate) fn moment(millis: i64) -> Timestamp {
    Timestamp::from_unix_millis(millis)
}

pub(crate) fn count(number: i64) -> Result<u64, Failure> {
    u64::try_from(number).map_err(|_| failure::corrupt("a stored count below zero"))
}

pub(crate) fn small(number: i64) -> Result<u32, Failure> {
    u32::try_from(number).map_err(|_| failure::corrupt("a stored number out of range"))
}

/// A number as the store keeps it. Lease numbers and sequence numbers are far
/// below the limit; one above it is refused, not wrapped.
pub(crate) fn stored(number: u64) -> Result<i64, Failure> {
    i64::try_from(number).map_err(|_| failure::unexpected("a number too large to store"))
}

/// The digest of several named fields. Each is written as its name's length,
/// its name, its value's length, and its value, so no two different sets of
/// fields give the same bytes.
pub(crate) fn digest_of(purpose: &str, fields: &[(&str, &[u8])]) -> Digest {
    let mut bytes = Vec::new();
    let mut field = |name: &str, value: &[u8]| {
        bytes.extend_from_slice(&(name.len() as u64).to_le_bytes());
        bytes.extend_from_slice(name.as_bytes());
        bytes.extend_from_slice(&(value.len() as u64).to_le_bytes());
        bytes.extend_from_slice(value);
    };
    field("anything2mcp", purpose.as_bytes());
    for (name, value) in fields {
        field(name, value);
    }
    Digest::of(&bytes)
}
