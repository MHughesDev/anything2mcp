//! The stand's own writer and reader for the container a workbook is kept in.
//!
//! The writer stores every part without compression, with a fixed date, so the
//! same parts always give the same bytes. The reader lists the parts of any such
//! container, compressed or not, and digests each part's bytes exactly as they
//! sit in the file, without ever decompressing them. That is enough to show that
//! a save left a part untouched byte for byte. It shares no code with any reader
//! the product uses.

use crate::control::sha256_hex;

const LOCAL_HEADER: u32 = 0x0403_4b50;
const DIRECTORY_HEADER: u32 = 0x0201_4b50;
const END_RECORD: u32 = 0x0605_4b50;
const LOCAL_HEADER_LENGTH: usize = 30;
const DIRECTORY_HEADER_LENGTH: usize = 46;
const END_RECORD_LENGTH: usize = 22;
/// 1 January 1980, the earliest date the container can hold.
const FIXED_DATE: u16 = 0x0021;

/// One part of a package, as it sits in the file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackagePart {
    /// The part's name inside the package, such as `xl/workbook.xml`.
    pub name: String,
    /// How the part is kept: 0 is stored as it is, 8 is deflated.
    pub method: u16,
    /// The checksum the package records for the part's content.
    pub crc32: u32,
    /// How many bytes the part takes in the file.
    pub stored_size: u64,
    /// How many bytes the part's content has.
    pub size: u64,
    /// The SHA-256 digest of the part's bytes as they sit in the file.
    pub stored_sha256: String,
}

/// The checksum the container uses, computed bit by bit.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

fn put16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

/// A length as the container writes it. The stand's parts are far below the
/// limit; a part that was not would be cut short here and fail every reader.
fn length32(length: usize) -> u32 {
    u32::try_from(length).unwrap_or(u32::MAX)
}

fn length16(length: usize) -> u16 {
    u16::try_from(length).unwrap_or(u16::MAX)
}

/// Writes a package holding these parts, in this order, each stored as it is.
pub(crate) fn write(parts: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut directory = Vec::new();
    for (name, content) in parts {
        let offset = length32(out.len());
        let checksum = crc32(content);
        let size = length32(content.len());
        let name_length = length16(name.len());

        put32(&mut out, LOCAL_HEADER);
        put16(&mut out, 20); // version needed
        put16(&mut out, 0); // flags
        put16(&mut out, 0); // method: stored
        put16(&mut out, 0); // time
        put16(&mut out, FIXED_DATE);
        put32(&mut out, checksum);
        put32(&mut out, size);
        put32(&mut out, size);
        put16(&mut out, name_length);
        put16(&mut out, 0); // extra field
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(content);

        put32(&mut directory, DIRECTORY_HEADER);
        put16(&mut directory, 20); // version made by
        put16(&mut directory, 20); // version needed
        put16(&mut directory, 0); // flags
        put16(&mut directory, 0); // method: stored
        put16(&mut directory, 0); // time
        put16(&mut directory, FIXED_DATE);
        put32(&mut directory, checksum);
        put32(&mut directory, size);
        put32(&mut directory, size);
        put16(&mut directory, name_length);
        put16(&mut directory, 0); // extra field
        put16(&mut directory, 0); // comment
        put16(&mut directory, 0); // disk
        put16(&mut directory, 0); // internal attributes
        put32(&mut directory, 0); // external attributes
        put32(&mut directory, offset);
        directory.extend_from_slice(name.as_bytes());
    }
    let directory_offset = length32(out.len());
    let directory_size = length32(directory.len());
    let count = length16(parts.len());
    out.extend_from_slice(&directory);
    put32(&mut out, END_RECORD);
    put16(&mut out, 0); // this disk
    put16(&mut out, 0); // disk with the directory
    put16(&mut out, count);
    put16(&mut out, count);
    put32(&mut out, directory_size);
    put32(&mut out, directory_offset);
    put16(&mut out, 0); // comment
    out
}

fn read16(bytes: &[u8], at: usize) -> Option<u16> {
    let slice = bytes.get(at..at.checked_add(2)?)?;
    Some(u16::from_le_bytes([slice[0], slice[1]]))
}

fn read32(bytes: &[u8], at: usize) -> Option<u32> {
    let slice = bytes.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn index(value: u32) -> Option<usize> {
    usize::try_from(value).ok()
}

/// Lists the parts of a package, in the order of its directory. `None` when the
/// bytes are not a package this reader can list.
pub(crate) fn parts(bytes: &[u8]) -> Option<Vec<PackagePart>> {
    // The end record is the last thing in the file, before an optional comment.
    let last = bytes.len().checked_sub(END_RECORD_LENGTH)?;
    let end = (0..=last)
        .rev()
        .find(|at| read32(bytes, *at) == Some(END_RECORD))?;
    let count = read16(bytes, end + 10)?;
    let mut at = index(read32(bytes, end + 16)?)?;

    let mut found = Vec::new();
    for _ in 0..count {
        if read32(bytes, at)? != DIRECTORY_HEADER {
            return None;
        }
        let method = read16(bytes, at + 10)?;
        let crc32 = read32(bytes, at + 16)?;
        let stored_size = index(read32(bytes, at + 20)?)?;
        let size = read32(bytes, at + 24)?;
        let name_length = usize::from(read16(bytes, at + 28)?);
        let extra_length = usize::from(read16(bytes, at + 30)?);
        let comment_length = usize::from(read16(bytes, at + 32)?);
        let local = index(read32(bytes, at + 42)?)?;
        let name_start = at + DIRECTORY_HEADER_LENGTH;
        let name = bytes.get(name_start..name_start.checked_add(name_length)?)?;

        // The part's bytes follow its own header, whose name and extra field may
        // differ in length from the directory's.
        if read32(bytes, local)? != LOCAL_HEADER {
            return None;
        }
        let local_name = usize::from(read16(bytes, local + 26)?);
        let local_extra = usize::from(read16(bytes, local + 28)?);
        let start = local + LOCAL_HEADER_LENGTH + local_name + local_extra;
        let stored = bytes.get(start..start.checked_add(stored_size)?)?;

        found.push(PackagePart {
            name: String::from_utf8_lossy(name).into_owned(),
            method,
            crc32,
            stored_size: u64::try_from(stored_size).ok()?,
            size: u64::from(size),
            stored_sha256: sha256_hex(stored),
        });
        at = name_start + name_length + extra_length + comment_length;
    }
    Some(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_checksum_matches_the_published_check_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn what_is_written_is_listed_again() {
        let bytes = write(&[("a.xml", b"<a/>".to_vec()), ("dir/b.bin", vec![0, 1, 2])]);
        let parts = parts(&bytes).unwrap();
        let names: Vec<&str> = parts.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["a.xml", "dir/b.bin"]);
        assert_eq!(parts[0].method, 0);
        assert_eq!(parts[0].size, 4);
        assert_eq!(parts[0].stored_sha256, sha256_hex(b"<a/>"));
        assert_eq!(parts[1].crc32, crc32(&[0, 1, 2]));
    }

    #[test]
    fn bytes_that_are_not_a_package_are_not_listed() {
        assert_eq!(parts(b"not a package"), None);
        assert_eq!(parts(&[]), None);
    }
}
