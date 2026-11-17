//! Helpers for this crate's tests: limits, and a package built from code.

use crate::model::Limits;
use crate::package::checksum;

/// Limits wide enough for every test input.
pub(crate) const LIMITS: Limits = Limits {
    bytes: 8 * 1024 * 1024,
    unpacked_bytes: 16 * 1024 * 1024,
    rows: 100_000,
    cells: 1_000_000,
    cell_bytes: 32 * 1024,
};

/// How a test package keeps a part.
#[derive(Clone, Copy)]
pub(crate) enum Packed {
    Stored,
    Deflated,
}

/// Builds a package from parts, each stored or deflated, with a fixed date,
/// so the same parts give the same bytes.
pub(crate) fn package(parts: &[(&str, &[u8], Packed)]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut directory = Vec::new();
    for (name, bytes, packed) in parts {
        let (method, data) = match packed {
            Packed::Stored => (0_u16, bytes.to_vec()),
            Packed::Deflated => (8_u16, miniz_oxide::deflate::compress_to_vec(bytes, 6)),
        };
        let offset = u32::try_from(out.len()).unwrap();
        let mut fields = Vec::new();
        fields.extend_from_slice(&20_u16.to_le_bytes()); // version needed
        fields.extend_from_slice(&0_u16.to_le_bytes()); // flags
        fields.extend_from_slice(&method.to_le_bytes());
        fields.extend_from_slice(&0_u16.to_le_bytes()); // time
        fields.extend_from_slice(&0x0021_u16.to_le_bytes()); // date: 1980-01-01
        fields.extend_from_slice(&checksum(bytes).to_le_bytes());
        fields.extend_from_slice(&u32::try_from(data.len()).unwrap().to_le_bytes());
        fields.extend_from_slice(&u32::try_from(bytes.len()).unwrap().to_le_bytes());
        fields.extend_from_slice(&u16::try_from(name.len()).unwrap().to_le_bytes());
        fields.extend_from_slice(&0_u16.to_le_bytes()); // extra length
        out.extend_from_slice(b"PK\x03\x04");
        out.extend_from_slice(&fields);
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(&data);
        directory.extend_from_slice(b"PK\x01\x02");
        directory.extend_from_slice(&20_u16.to_le_bytes()); // version made by
        directory.extend_from_slice(&fields);
        directory.extend_from_slice(&0_u16.to_le_bytes()); // comment length
        directory.extend_from_slice(&0_u16.to_le_bytes()); // disk
        directory.extend_from_slice(&0_u16.to_le_bytes()); // internal attributes
        directory.extend_from_slice(&0_u32.to_le_bytes()); // external attributes
        directory.extend_from_slice(&offset.to_le_bytes());
        directory.extend_from_slice(name.as_bytes());
    }
    let start = u32::try_from(out.len()).unwrap();
    let count = u16::try_from(parts.len()).unwrap();
    out.extend_from_slice(&directory);
    out.extend_from_slice(b"PK\x05\x06");
    out.extend_from_slice(&0_u16.to_le_bytes());
    out.extend_from_slice(&0_u16.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&u32::try_from(directory.len()).unwrap().to_le_bytes());
    out.extend_from_slice(&start.to_le_bytes());
    out.extend_from_slice(&0_u16.to_le_bytes());
    out
}
