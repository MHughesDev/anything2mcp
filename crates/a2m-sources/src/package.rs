//! The package a workbook is kept in: a ZIP archive, read from its central
//! directory. Parts are stored or deflated; nothing else is read. A part is
//! unpacked only when asked for, under the caller's limit, and its checksum
//! is checked.

use a2m_core::Failure;

use crate::error::{malformed, protected, too_large};

const END_RECORD: &[u8] = b"PK\x05\x06";
const DIRECTORY_ENTRY: &[u8] = b"PK\x01\x02";
const LOCAL_HEADER: &[u8] = b"PK\x03\x04";
/// The most parts a package may list.
const MOST_PARTS: usize = 4096;

/// One part as the directory lists it.
#[derive(Debug, Clone)]
struct Entry {
    name: String,
    deflated: bool,
    checksum: u32,
    packed: usize,
    unpacked: u64,
    header: usize,
}

/// A package: its bytes and its directory.
#[derive(Debug)]
pub(crate) struct Package<'a> {
    bytes: &'a [u8],
    entries: Vec<Entry>,
    /// How many unpacked bytes may still be produced.
    budget: u64,
}

fn u16_at(bytes: &[u8], at: usize) -> Option<usize> {
    let pair = bytes.get(at..at + 2)?;
    Some(usize::from(u16::from_le_bytes([
        *pair.first()?,
        *pair.get(1)?,
    ])))
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    let four = bytes.get(at..at + 4)?;
    Some(u32::from_le_bytes([
        *four.first()?,
        *four.get(1)?,
        *four.get(2)?,
        *four.get(3)?,
    ]))
}

fn cut() -> Failure {
    malformed("the package's directory is cut short or points outside the file")
}

impl<'a> Package<'a> {
    /// Reads the directory of a package.
    pub(crate) fn open(bytes: &'a [u8], unpacked_limit: u64) -> Result<Self, Failure> {
        // The end record is the last thing in the file, before a comment of
        // at most 65,535 bytes.
        let earliest = bytes.len().saturating_sub(22 + 65_535);
        let end = (earliest..=bytes.len().saturating_sub(22))
            .rev()
            .find(|&at| bytes.get(at..at + 4) == Some(END_RECORD))
            .ok_or_else(|| malformed("the package has no directory"))?;
        let count = u16_at(bytes, end + 10).ok_or_else(cut)?;
        let size = u32_at(bytes, end + 12).ok_or_else(cut)?;
        let start = u32_at(bytes, end + 16).ok_or_else(cut)?;
        if count == 0xFFFF || size == u32::MAX || start == u32::MAX {
            return Err(malformed(
                "the package uses the large-file form, which is not read",
            ));
        }
        if count > MOST_PARTS {
            return Err(too_large(format!(
                "more than {MOST_PARTS} parts in the package"
            )));
        }
        let mut at = usize::try_from(start).map_err(|_| cut())?;
        let mut entries: Vec<Entry> = Vec::with_capacity(count);
        for _ in 0..count {
            if bytes.get(at..at + 4) != Some(DIRECTORY_ENTRY) {
                return Err(cut());
            }
            let flags = u16_at(bytes, at + 8).ok_or_else(cut)?;
            let method = u16_at(bytes, at + 10).ok_or_else(cut)?;
            let checksum = u32_at(bytes, at + 16).ok_or_else(cut)?;
            let packed = u32_at(bytes, at + 20).ok_or_else(cut)?;
            let unpacked = u32_at(bytes, at + 24).ok_or_else(cut)?;
            let name_length = u16_at(bytes, at + 28).ok_or_else(cut)?;
            let extra_length = u16_at(bytes, at + 30).ok_or_else(cut)?;
            let comment_length = u16_at(bytes, at + 32).ok_or_else(cut)?;
            let header = u32_at(bytes, at + 42).ok_or_else(cut)?;
            let name = bytes.get(at + 46..at + 46 + name_length).ok_or_else(cut)?;
            let name = std::str::from_utf8(name)
                .map_err(|_| malformed("a part's name is not UTF-8"))?
                .to_owned();
            if flags & 1 == 1 {
                return Err(protected());
            }
            if packed == u32::MAX || unpacked == u32::MAX || header == u32::MAX {
                return Err(malformed(
                    "the package uses the large-file form, which is not read",
                ));
            }
            let deflated = match method {
                0 => false,
                8 => true,
                _ => {
                    return Err(malformed(
                        "a part is packed by a method other than store or deflate",
                    ));
                }
            };
            if entries.iter().any(|known| known.name == name) {
                return Err(malformed("the package lists one part twice"));
            }
            entries.push(Entry {
                name,
                deflated,
                checksum,
                packed: usize::try_from(packed).map_err(|_| cut())?,
                unpacked: u64::from(unpacked),
                header: usize::try_from(header).map_err(|_| cut())?,
            });
            at += 46 + name_length + extra_length + comment_length;
        }
        Ok(Self {
            bytes,
            entries,
            budget: unpacked_limit,
        })
    }

    /// The names of the parts, in the directory's order.
    pub(crate) fn names(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|entry| entry.name.as_str())
    }

    /// Whether the package lists a part with that name.
    pub(crate) fn has(&self, name: &str) -> bool {
        self.entries.iter().any(|entry| entry.name == name)
    }

    /// Unpacks one part. Nothing when the package does not list it.
    pub(crate) fn part(&mut self, name: &str) -> Result<Option<Vec<u8>>, Failure> {
        let Some(entry) = self.entries.iter().find(|entry| entry.name == name) else {
            return Ok(None);
        };
        if entry.unpacked > self.budget {
            return Err(too_large(
                "the package's parts unpack to more bytes than the limit",
            ));
        }
        let header = entry.header;
        if self.bytes.get(header..header + 4) != Some(LOCAL_HEADER) {
            return Err(cut());
        }
        let name_length = u16_at(self.bytes, header + 26).ok_or_else(cut)?;
        let extra_length = u16_at(self.bytes, header + 28).ok_or_else(cut)?;
        let data = header + 30 + name_length + extra_length;
        let packed = self
            .bytes
            .get(data..data.checked_add(entry.packed).ok_or_else(cut)?)
            .ok_or_else(cut)?;
        let limit = usize::try_from(entry.unpacked).map_err(|_| cut())?;
        let unpacked = if entry.deflated {
            miniz_oxide::inflate::decompress_to_vec_with_limit(packed, limit).map_err(|_| {
                malformed("a part does not unpack, or unpacks to more than the package states")
            })?
        } else {
            packed.to_vec()
        };
        if unpacked.len() != limit {
            return Err(malformed(
                "a part does not unpack to the size the package states",
            ));
        }
        if checksum(&unpacked) != entry.checksum {
            return Err(malformed("a part's checksum does not match its bytes"));
        }
        self.budget -= entry.unpacked;
        Ok(Some(unpacked))
    }
}

/// The table of the CRC-32 checksum, one entry for each value of a byte.
const CHECKSUM_TABLE: [u32; 256] = {
    let mut table = [0_u32; 256];
    let mut index = 0;
    while index < 256 {
        let mut crc = index as u32;
        let mut bit = 0;
        while bit < 8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
            bit += 1;
        }
        table[index] = crc;
        index += 1;
    }
    table
};

/// The CRC-32 checksum a package keeps for each part.
pub(crate) fn checksum(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for &byte in bytes {
        let index = usize::from((crc as u8) ^ byte);
        crc = (crc >> 8) ^ CHECKSUM_TABLE.get(index).copied().unwrap_or(0);
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::{SOURCE_MALFORMED, SOURCE_PROTECTED, SOURCE_TOO_LARGE};
    use crate::testing::{Packed, package};

    #[test]
    fn the_checksum_is_the_one_a_package_keeps() {
        assert_eq!(checksum(b"123456789"), 0xCBF4_3926);
        assert_eq!(checksum(b""), 0);
    }

    #[test]
    fn stored_and_deflated_parts_unpack_to_their_bytes() {
        let long = "abc".repeat(500);
        let bytes = package(&[
            ("a.txt", b"stored".as_slice(), Packed::Stored),
            ("dir/b.txt", long.as_bytes(), Packed::Deflated),
        ]);
        let mut package = Package::open(&bytes, 10_000).unwrap();
        assert_eq!(package.names().collect::<Vec<_>>(), ["a.txt", "dir/b.txt"]);
        assert_eq!(package.part("a.txt").unwrap().unwrap(), b"stored");
        assert_eq!(package.part("dir/b.txt").unwrap().unwrap(), long.as_bytes());
        assert!(package.part("absent").unwrap().is_none());
    }

    #[test]
    fn a_package_that_lies_or_is_too_large_is_refused() {
        let long = "abc".repeat(500);
        let good = package(&[("a.txt", long.as_bytes(), Packed::Deflated)]);

        // More unpacked bytes than the caller allows.
        let mut small = Package::open(&good, 100).unwrap();
        assert_eq!(small.part("a.txt").unwrap_err().code(), &SOURCE_TOO_LARGE);

        // A part whose bytes were changed after its checksum was taken.
        let stored = package(&[("a.txt", b"stored".as_slice(), Packed::Stored)]);
        let mut changed = stored.clone();
        let at = changed.windows(6).position(|w| w == b"stored").unwrap();
        changed[at] = b'S';
        let mut package_changed = Package::open(&changed, 100).unwrap();
        assert_eq!(
            package_changed.part("a.txt").unwrap_err().code(),
            &SOURCE_MALFORMED
        );

        // A part marked as encrypted.
        let mut encrypted = stored.clone();
        let entry = encrypted
            .windows(4)
            .position(|w| w == DIRECTORY_ENTRY)
            .unwrap();
        encrypted[entry + 8] |= 1;
        assert_eq!(
            Package::open(&encrypted, 100).unwrap_err().code(),
            &SOURCE_PROTECTED
        );

        // Cut short, and not a package at all.
        assert_eq!(
            Package::open(&stored[..stored.len() - 30], 100)
                .unwrap_err()
                .code(),
            &SOURCE_MALFORMED
        );
        assert_eq!(
            Package::open(b"not a package", 100).unwrap_err().code(),
            &SOURCE_MALFORMED
        );
    }
}
