//! The device's own update mechanism: the image format, the checksums, and the
//! names of the states, commands, and errors of the declared sequence.
//!
//! The sequence itself is carried out in `device.rs`. This module holds what a
//! test needs to make an image the device will take, or one it will refuse.

use std::fmt;

/// The four bytes every image starts with.
pub const MAGIC: [u8; 4] = *b"EXFW";
/// The model code an image must carry for this device to take it.
pub const MODEL_CODE: u16 = 0x4543;
/// The largest image the device takes, in bytes.
pub const MAX_IMAGE_BYTES: usize = 4096;
/// The most image bytes one block carries.
pub const MAX_BLOCK_BYTES: usize = 240;
/// Bytes before the payload: magic, model code, version, payload length.
const HEAD_BYTES: usize = 14;
/// Bytes after the payload: the checksum.
const TAIL_BYTES: usize = 4;

/// A firmware version, as the device reports it in three registers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    /// The first number.
    pub major: u16,
    /// The second number.
    pub minor: u16,
    /// The third number.
    pub patch: u16,
}

impl Version {
    /// A version from its three numbers.
    #[must_use]
    pub const fn new(major: u16, minor: u16, patch: u16) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// The values of the `update_state` point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateState {
    /// 0: not in update mode.
    Idle,
    /// 1: in update mode, taking blocks.
    Receiving,
    /// 2: the image was checked and may be committed.
    Verified,
    /// 3: the image was checked and refused; only an abort leaves this state.
    Failed,
}

impl UpdateState {
    /// The value the device reports.
    #[must_use]
    pub const fn code(self) -> u16 {
        match self {
            Self::Idle => 0,
            Self::Receiving => 1,
            Self::Verified => 2,
            Self::Failed => 3,
        }
    }
}

/// The values a client writes to the `update_command` point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateCommand {
    /// 1: enter update mode. The image length must be declared first.
    Enter,
    /// 2: check the image received.
    Verify,
    /// 3: make the checked image the running one, and restart.
    Commit,
    /// 4: leave update mode and discard what was received.
    Abort,
    /// 5: start from the standby slot again, and restart.
    RollBack,
}

impl UpdateCommand {
    /// The value written to the device.
    #[must_use]
    pub const fn code(self) -> u16 {
        match self {
            Self::Enter => 1,
            Self::Verify => 2,
            Self::Commit => 3,
            Self::Abort => 4,
            Self::RollBack => 5,
        }
    }

    pub(crate) const fn from_code(code: i64) -> Option<Self> {
        match code {
            1 => Some(Self::Enter),
            2 => Some(Self::Verify),
            3 => Some(Self::Commit),
            4 => Some(Self::Abort),
            5 => Some(Self::RollBack),
            _ => None,
        }
    }
}

/// The values of the `update_error` point: why the last update step was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateError {
    /// 0: nothing was refused.
    None,
    /// 1: the command or block is not allowed in the present update state.
    WrongState,
    /// 2: update mode was refused because the pump is not stopped.
    PumpRunning,
    /// 3: update mode was refused because no image length was declared.
    LengthNotDeclared,
    /// 4: a block did not carry the next block number.
    BlockOutOfOrder,
    /// 5: a block's byte count is wrong, or it runs past the declared length.
    BlockLength,
    /// 6: a block's checksum does not match its bytes.
    BlockChecksum,
    /// 7: fewer bytes were received than the declared length.
    ImageIncomplete,
    /// 8: the image does not have the form of an image.
    ImageMalformed,
    /// 9: the image's checksum does not match its bytes.
    ImageChecksum,
    /// 10: the image is for another model.
    ImageWrongModel,
    /// 11: the device refused an image that is well formed.
    ImageRefused,
    /// 12: a roll back was refused because the standby slot holds no valid image.
    NoStandbyImage,
}

impl UpdateError {
    /// The value the device reports.
    #[must_use]
    pub const fn code(self) -> u16 {
        match self {
            Self::None => 0,
            Self::WrongState => 1,
            Self::PumpRunning => 2,
            Self::LengthNotDeclared => 3,
            Self::BlockOutOfOrder => 4,
            Self::BlockLength => 5,
            Self::BlockChecksum => 6,
            Self::ImageIncomplete => 7,
            Self::ImageMalformed => 8,
            Self::ImageChecksum => 9,
            Self::ImageWrongModel => 10,
            Self::ImageRefused => 11,
            Self::NoStandbyImage => 12,
        }
    }
}

/// One of the device's two firmware slots.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot {
    /// Whether the slot holds an image the device can start from.
    pub valid: bool,
    /// The version of the image in the slot.
    pub version: Version,
    /// The checksum of the image in the slot.
    pub checksum: u32,
}

/// The checksum of one block: CRC-16 with polynomial `0xA001` reflected and a
/// start value of `0xFFFF`, the one known as CRC-16/MODBUS.
#[must_use]
pub fn crc16(bytes: &[u8]) -> u16 {
    let mut crc = 0xFFFFu16;
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

/// The checksum of an image: CRC-32 as used by Ethernet and zip files.
#[must_use]
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
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

/// Builds an image as the device's imaginary maker would issue it.
///
/// The layout, all numbers high byte first: the four bytes `EXFW`, the model
/// code, the three numbers of the version, the payload length, the payload,
/// and the CRC-32 of everything before it. A payload longer than 65535 bytes
/// is cut to that length; the device takes far less in any case.
#[must_use]
pub fn build_image(model_code: u16, version: Version, payload: &[u8]) -> Vec<u8> {
    let payload = &payload[..payload.len().min(usize::from(u16::MAX))];
    let mut image = Vec::with_capacity(HEAD_BYTES + payload.len() + TAIL_BYTES);
    image.extend(MAGIC);
    image.extend(model_code.to_be_bytes());
    image.extend(version.major.to_be_bytes());
    image.extend(version.minor.to_be_bytes());
    image.extend(version.patch.to_be_bytes());
    image.extend((payload.len() as u16).to_be_bytes());
    image.extend(payload);
    image.extend(crc32(&image).to_be_bytes());
    image
}

/// The checksum the device reports for an image: the CRC-32 of the image
/// without its last four bytes, which is the value those four bytes hold.
/// `None` for bytes too short to be an image.
#[must_use]
pub fn image_checksum(image: &[u8]) -> Option<u32> {
    let body = image.len().checked_sub(TAIL_BYTES)?;
    (body >= HEAD_BYTES).then(|| crc32(&image[..body]))
}

/// What the device learns from an image it accepts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Checked {
    pub(crate) version: Version,
    pub(crate) checksum: u32,
}

/// Checks an image as the device does: form, then checksum, then model.
pub(crate) fn check_image(image: &[u8]) -> Result<Checked, UpdateError> {
    let two = |at: usize| u16::from_be_bytes([image[at], image[at + 1]]);
    if image.len() < HEAD_BYTES + TAIL_BYTES || image[..4] != MAGIC {
        return Err(UpdateError::ImageMalformed);
    }
    if image.len() != HEAD_BYTES + usize::from(two(12)) + TAIL_BYTES {
        return Err(UpdateError::ImageMalformed);
    }
    let body = image.len() - TAIL_BYTES;
    let carried = u32::from_be_bytes([
        image[body],
        image[body + 1],
        image[body + 2],
        image[body + 3],
    ]);
    let checksum = crc32(&image[..body]);
    if carried != checksum {
        return Err(UpdateError::ImageChecksum);
    }
    if two(4) != MODEL_CODE {
        return Err(UpdateError::ImageWrongModel);
    }
    Ok(Checked {
        version: Version::new(two(6), two(8), two(10)),
        checksum,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_checksums_match_their_published_check_values() {
        assert_eq!(crc16(b"123456789"), 0x4B37);
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn an_image_is_checked_for_form_then_checksum_then_model() {
        let version = Version::new(2, 5, 0);
        let image = build_image(MODEL_CODE, version, b"payload");
        let checked = check_image(&image).unwrap();
        assert_eq!(checked.version, version);
        assert_eq!(Some(checked.checksum), image_checksum(&image));

        assert_eq!(check_image(b"short"), Err(UpdateError::ImageMalformed));
        let mut cut = image.clone();
        cut.pop();
        assert_eq!(check_image(&cut), Err(UpdateError::ImageMalformed));
        let mut flipped = image.clone();
        flipped[15] ^= 1;
        assert_eq!(check_image(&flipped), Err(UpdateError::ImageChecksum));
        let other = build_image(0x0001, version, b"payload");
        assert_eq!(check_image(&other), Err(UpdateError::ImageWrongModel));
    }
}
