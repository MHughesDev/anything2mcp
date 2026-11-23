//! Firmware: the form of an image, the checksums, and the frames of the
//! `register_blocks` update (spec section 310, Firmware).
//!
//! The pack never builds, patches, or signs an image. It reads the form of
//! one the device's maker issued, to say what it holds (inspection) and to
//! check the version an operation names against the image's own before
//! anything is sent (update). What the device accepts is the device's
//! check, not this one.
//!
//! The form read here is the one the first device's imaginary maker
//! issues, all numbers high byte first: the four bytes `EXFW`, the model
//! code, the three numbers of the version, the payload length, the payload,
//! and the CRC-32 of everything before it.

use std::ops::Range;

/// The four bytes the form starts with.
pub(crate) const MAGIC: [u8; 4] = *b"EXFW";
/// Bytes before the payload: magic, model code, version, payload length.
pub(crate) const HEAD_BYTES: usize = 14;
/// Bytes after the payload: the checksum.
pub(crate) const TAIL_BYTES: usize = 4;

/// The commands the mechanism writes to the command address.
pub(crate) const ENTER: u16 = 1;
pub(crate) const VERIFY: u16 = 2;
pub(crate) const COMMIT: u16 = 3;
pub(crate) const ABORT: u16 = 4;

/// The values the state register reports.
pub(crate) const STATE_VERIFIED: i64 = 2;

/// The checksum of one block: CRC-16 with polynomial `0xA001` reflected and
/// a start value of `0xFFFF`.
pub(crate) fn crc16(bytes: &[u8]) -> u16 {
    let mut crc = 0xFFFF_u16;
    for byte in bytes {
        crc ^= u16::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xA001
            } else {
                crc >> 1
            };
        }
    }
    crc
}

/// The checksum of an image: the CRC-32 of Ethernet and zip files.
pub(crate) fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFF_u32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// Why bytes are not an image of the form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NotAnImage {
    /// Too short to hold the form's head and tail, or the magic is another.
    Form,
    /// The length the head states is not the length the bytes have.
    Length,
}

/// What the form's head and tail say, read from bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Image {
    pub model_code: u16,
    /// The version, as the device reports it: `2.5.0`.
    pub version: String,
    /// Where the payload lies in the bytes.
    pub payload: Range<usize>,
    /// The checksum the image carries in its tail.
    pub carried: u32,
    /// The checksum of everything before the tail.
    pub computed: u32,
}

impl Image {
    pub fn checksum_holds(&self) -> bool {
        self.carried == self.computed
    }
}

/// Reads the form of an image. Nothing is decrypted or recovered, and the
/// bytes are not changed.
pub(crate) fn read(bytes: &[u8]) -> Result<Image, NotAnImage> {
    if bytes.len() < HEAD_BYTES + TAIL_BYTES || bytes[..4] != MAGIC {
        return Err(NotAnImage::Form);
    }
    let two = |at: usize| u16::from_be_bytes([bytes[at], bytes[at + 1]]);
    let length = usize::from(two(12));
    if bytes.len() != HEAD_BYTES + length + TAIL_BYTES {
        return Err(NotAnImage::Length);
    }
    let body = bytes.len() - TAIL_BYTES;
    Ok(Image {
        model_code: two(4),
        version: format!("{}.{}.{}", two(6), two(8), two(10)),
        payload: HEAD_BYTES..body,
        carried: u32::from_be_bytes([
            bytes[body],
            bytes[body + 1],
            bytes[body + 2],
            bytes[body + 3],
        ]),
        computed: crc32(&bytes[..body]),
    })
}

/// The words of every block of an image: the block's number from zero, the
/// count of image bytes in it, the CRC-16 of those bytes, then the bytes two
/// to a word, first in the high byte, a last odd byte padded with zero.
pub(crate) fn blocks(image: &[u8], block_bytes: u16) -> Vec<Vec<u16>> {
    image
        .chunks(usize::from(block_bytes.max(1)))
        .enumerate()
        .map(|(number, chunk)| {
            let mut words = vec![
                u16::try_from(number).unwrap_or(u16::MAX),
                u16::try_from(chunk.len()).unwrap_or(u16::MAX),
                crc16(chunk),
            ];
            words.extend(
                chunk
                    .chunks(2)
                    .map(|pair| u16::from_be_bytes([pair[0], pair.get(1).copied().unwrap_or(0)])),
            );
            words
        })
        .collect()
}

/// How many frames an update of `image_bytes` may send, from the length
/// write to the commit, and `after` frames to read it back: the length,
/// enter, each block, verify, the state read, the commit, and one more in
/// case the device has to be told to leave update mode.
pub(crate) fn frames(image_bytes: usize, block_bytes: u16, after: u32) -> u32 {
    let blocks = image_bytes.div_ceil(usize::from(block_bytes.max(1)));
    let blocks = u32::try_from(blocks).unwrap_or(u32::MAX);
    blocks.saturating_add(6).saturating_add(after)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(payload: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend(MAGIC);
        bytes.extend(0x4543_u16.to_be_bytes());
        for number in [2_u16, 5, 0] {
            bytes.extend(number.to_be_bytes());
        }
        bytes.extend(u16::try_from(payload.len()).unwrap().to_be_bytes());
        bytes.extend(payload);
        bytes.extend(crc32(&bytes).to_be_bytes());
        bytes
    }

    #[test]
    fn the_checksums_match_their_published_check_values() {
        assert_eq!(crc16(b"123456789"), 0x4B37);
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn an_image_is_read_by_its_form_and_never_changed() {
        let bytes = image(b"mode=normal");
        let found = read(&bytes).unwrap();
        assert_eq!(found.version, "2.5.0");
        assert_eq!(found.model_code, 0x4543);
        assert_eq!(&bytes[found.payload.clone()], b"mode=normal");
        assert!(found.checksum_holds());

        let mut damaged = bytes.clone();
        damaged[15] ^= 1;
        assert!(!read(&damaged).unwrap().checksum_holds());
        assert_eq!(read(b"short"), Err(NotAnImage::Form));
        let mut cut = bytes.clone();
        cut.pop();
        assert_eq!(read(&cut), Err(NotAnImage::Length));
        let mut other = bytes;
        other[0] = b'X';
        other[1] = b'X';
        assert_eq!(read(&other), Err(NotAnImage::Form));
    }

    #[test]
    fn a_block_carries_its_number_its_count_its_checksum_and_its_bytes() {
        let bytes: Vec<u8> = (0..5).collect();
        let blocks = blocks(&bytes, 4);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0][..3], [0, 4, crc16(&bytes[..4])]);
        assert_eq!(blocks[0][3..], [0x0001, 0x0203]);
        // A last odd byte is padded with zero.
        assert_eq!(blocks[1], vec![1, 1, crc16(&bytes[4..]), 0x0400]);
    }
}
