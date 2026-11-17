//! The device's point map: every address it answers for, how the value is laid
//! out in words, and whether it may be written.
//!
//! This is the stand's own statement of what the device is. A pack's tests
//! write their `registers/` file by hand and may check it against this table;
//! the stand never reads a pack's file.

/// The four tables of the protocol.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Table {
    /// Bits a client may read and write: functions 01 and 05.
    Coil,
    /// Bits a client may only read: function 02.
    Discrete,
    /// Words a client may read and write: functions 03, 06, and 16.
    Holding,
    /// Words a client may only read: function 04.
    Input,
}

/// How a value is laid out in the words of its point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Data {
    /// One bit.
    Bit,
    /// One word, unsigned.
    U16,
    /// One word, signed, two's complement.
    I16,
    /// Two words, unsigned, the high word at the lower address.
    U32HighFirst,
    /// Two words, unsigned, the low word at the lower address.
    U32LowFirst,
    /// Text, two characters per word, the first in the high byte, padded with zero bytes.
    Ascii,
    /// The update mechanism's block window; see the README.
    Block,
}

/// Whether a client may read a point, write it, or both.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    /// Read only. A write is refused with exception 02.
    Read,
    /// A command: a write acts, and a read returns zero.
    Write,
    /// Read and write.
    ReadWrite,
}

impl Access {
    /// Whether a write function may address the point.
    #[must_use]
    pub const fn writable(self) -> bool {
        !matches!(self, Self::Read)
    }
}

/// The value of a point, decoded from its words. Numbers are raw: the scale is not applied.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    /// A bit.
    Bit(bool),
    /// A raw number.
    Number(i64),
    /// Text.
    Text(String),
}

/// Every point, by name. Each is described by its row in [`POINTS`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PointId {
    InletValve,
    FaultReset,
    PumpRunning,
    ValveOpenFeedback,
    PumpSpeedSetpoint,
    DosingRate,
    HeaterPowerLimit,
    TemperatureOffset,
    BatchTarget,
    RatedSpeed,
    UpdateCommand,
    UpdateImageLength,
    UpdateBlock,
    FaultLatch,
    PumpSpeedMeasured,
    Temperature,
    UptimeTicks,
    FirmwareMajor,
    FirmwareMinor,
    FirmwarePatch,
    SerialNumber,
    Vendor,
    Model,
    FirmwareChecksum,
    ActiveSlot,
    StandbyValid,
    StandbyMajor,
    StandbyMinor,
    StandbyPatch,
    UpdateState,
    UpdateError,
    UpdateNextBlock,
    UpdateBytesReceived,
    UpdateCount,
}

/// One point of the device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Point {
    /// Which point this is.
    pub id: PointId,
    /// Its name, as the control interface reports it.
    pub name: &'static str,
    /// The table it is in.
    pub table: Table,
    /// The address of its first word or its bit, counted from zero.
    pub offset: u16,
    /// How many words it spans. A bit counts as one.
    pub words: u16,
    /// How its value is laid out.
    pub data: Data,
    /// Whether it may be read, written, or both.
    pub access: Access,
    /// The factor that turns the raw number into the value in its unit, as decimal text.
    pub scale: &'static str,
    /// The unit the scaled value is in, or an empty text.
    pub unit: &'static str,
    /// The raw range the device itself enforces on a write, both ends included.
    pub range: Option<(i64, i64)>,
    /// Whether a restart puts it back to its starting value.
    pub volatile: bool,
    /// What else the device does with it.
    pub note: &'static str,
}

impl Point {
    const fn new(
        id: PointId,
        name: &'static str,
        table: Table,
        offset: u16,
        data: Data,
        access: Access,
    ) -> Self {
        let words = match data {
            Data::U32HighFirst | Data::U32LowFirst => 2,
            _ => 1,
        };
        Self {
            id,
            name,
            table,
            offset,
            words,
            data,
            access,
            scale: "1",
            unit: "",
            range: None,
            volatile: false,
            note: "",
        }
    }

    const fn words(mut self, words: u16) -> Self {
        self.words = words;
        self
    }

    const fn scaled(mut self, scale: &'static str, unit: &'static str) -> Self {
        self.scale = scale;
        self.unit = unit;
        self
    }

    const fn range(mut self, least: i64, most: i64) -> Self {
        self.range = Some((least, most));
        self
    }

    const fn volatile(mut self) -> Self {
        self.volatile = true;
        self
    }

    const fn note(mut self, note: &'static str) -> Self {
        self.note = note;
        self
    }
}

/// The address of the block window's first register.
pub const BLOCK_OFFSET: u16 = 1000;
/// The words in the block window: a block number, a byte count, a checksum, and up to 120 words of bytes.
pub const BLOCK_WORDS: u16 = 123;
/// The highest raw setpoint the device takes: 1450.0 rpm.
pub const PUMP_SPEED_MOST: i64 = 14_500;
/// The value the device clamps the heater power limit to, in percent.
pub const HEATER_POWER_MOST: u16 = 80;
/// The step the device rounds the dosing rate to, in raw units.
pub const DOSING_RATE_STEP: u16 = 5;

use Access::{Read, ReadWrite, Write};
use Data::{Ascii, Bit, Block, I16, U16, U32HighFirst, U32LowFirst};
use PointId as Id;
use Table::{Coil, Discrete, Holding, Input};

/// Every point the device has. An address not covered here is refused with exception 02.
pub const POINTS: &[Point] = &[
    Point::new(Id::InletValve, "inlet_valve", Coil, 4, Bit, ReadWrite)
        .volatile()
        .note("Commands the valve. The valve follows at the next tick."),
    Point::new(Id::FaultReset, "fault_reset", Coil, 8, Bit, Write)
        .note("A command: writing on clears the fault latch. Reads as off."),
    Point::new(Id::PumpRunning, "pump_running", Discrete, 0, Bit, Read)
        .volatile()
        .note("On while the measured pump speed is above zero."),
    Point::new(Id::ValveOpenFeedback, "valve_open_feedback", Discrete, 1, Bit, Read)
        .volatile()
        .note("The valve's position sensor."),
    Point::new(Id::PumpSpeedSetpoint, "pump_speed_setpoint", Holding, 100, U16, ReadWrite)
        .scaled("0.1", "rpm")
        .range(0, PUMP_SPEED_MOST)
        .volatile()
        .note("Outside the range: exception 03, nothing stored, fault latch bit 0 set."),
    Point::new(Id::DosingRate, "dosing_rate", Holding, 101, U16, ReadWrite)
        .scaled("0.01", "l/min")
        .range(0, 10_000)
        .note("The device rounds the value to the nearest multiple of 5, so a read-back may differ."),
    Point::new(Id::HeaterPowerLimit, "heater_power_limit", Holding, 102, U16, ReadWrite)
        .scaled("1", "%")
        .note("The device clamps the value to 80 and still echoes what was sent."),
    Point::new(Id::TemperatureOffset, "temperature_offset", Holding, 103, I16, ReadWrite)
        .scaled("0.1", "degC")
        .range(-100, 100),
    Point::new(Id::BatchTarget, "batch_target", Holding, 104, U32LowFirst, ReadWrite)
        .scaled("1", "ml")
        .range(0, 1_000_000)
        .note("Both words must be written in one frame."),
    Point::new(Id::RatedSpeed, "rated_speed", Holding, 106, U16, Read)
        .scaled("0.1", "rpm")
        .note("Read only, in a table that can be written: a write is refused with exception 02."),
    Point::new(Id::UpdateCommand, "update_command", Holding, 900, U16, Write)
        .note("A command of the update mechanism. Must be written alone. Reads as zero."),
    Point::new(Id::UpdateImageLength, "update_image_length", Holding, 901, U16, ReadWrite)
        .scaled("1", "bytes")
        .range(1, 4096)
        .volatile()
        .note("Declared before update mode is entered."),
    Point::new(Id::UpdateBlock, "update_block", Holding, BLOCK_OFFSET, Block, Write)
        .words(BLOCK_WORDS)
        .note("One block of the image per frame, function 16 only. Reads as zeros."),
    Point::new(Id::FaultLatch, "fault_latch", Input, 7, U16, Read)
        .volatile()
        .note("Reading it clears it. Bit 0: a write was out of range. Bit 1: an image failed its check."),
    Point::new(Id::PumpSpeedMeasured, "pump_speed_measured", Input, 30, U16, Read)
        .scaled("0.1", "rpm")
        .volatile()
        .note("Follows the setpoint by one ramp step per tick."),
    Point::new(Id::Temperature, "temperature", Input, 31, I16, Read)
        .scaled("0.1", "degC")
        .note("A fixed sensor value below zero, plus the temperature offset."),
    Point::new(Id::UptimeTicks, "uptime_ticks", Input, 32, U32HighFirst, Read)
        .volatile()
        .note("Changes on its own: one more at every tick."),
    Point::new(Id::FirmwareMajor, "firmware_major", Input, 100, U16, Read),
    Point::new(Id::FirmwareMinor, "firmware_minor", Input, 101, U16, Read),
    Point::new(Id::FirmwarePatch, "firmware_patch", Input, 102, U16, Read),
    Point::new(Id::SerialNumber, "serial_number", Input, 103, U32HighFirst, Read),
    Point::new(Id::Vendor, "vendor", Input, 105, Ascii, Read).words(4),
    Point::new(Id::Model, "model", Input, 109, Ascii, Read).words(9),
    Point::new(Id::FirmwareChecksum, "firmware_checksum", Input, 118, U32HighFirst, Read)
        .note("The checksum of the running image."),
    Point::new(Id::ActiveSlot, "active_slot", Input, 120, U16, Read)
        .note("0 or 1: the slot the device started from."),
    Point::new(Id::StandbyValid, "standby_valid", Input, 121, U16, Read)
        .note("1 while the other slot holds an image a roll back can start from."),
    Point::new(Id::StandbyMajor, "standby_major", Input, 122, U16, Read),
    Point::new(Id::StandbyMinor, "standby_minor", Input, 123, U16, Read),
    Point::new(Id::StandbyPatch, "standby_patch", Input, 124, U16, Read),
    Point::new(Id::UpdateState, "update_state", Input, 130, U16, Read)
        .volatile()
        .note("0 idle, 1 receiving, 2 verified, 3 failed."),
    Point::new(Id::UpdateError, "update_error", Input, 131, U16, Read)
        .volatile()
        .note("Why the last update step was refused; see the README."),
    Point::new(Id::UpdateNextBlock, "update_next_block", Input, 132, U16, Read)
        .volatile()
        .note("The number the next block must carry."),
    Point::new(Id::UpdateBytesReceived, "update_bytes_received", Input, 133, U16, Read)
        .scaled("1", "bytes")
        .volatile(),
    Point::new(Id::UpdateCount, "update_count", Input, 134, U16, Read)
        .note("How many images were committed. It only rises, and a roll back does not lower it."),
];

/// The point that covers an address, and which of its words the address is.
#[must_use]
pub fn find(table: Table, offset: u16) -> Option<(&'static Point, u16)> {
    POINTS.iter().find_map(|point| {
        let index = offset.checked_sub(point.offset)?;
        (point.table == table && index < point.words).then_some((point, index))
    })
}

/// The point with a name.
#[must_use]
pub fn by_name(name: &str) -> Option<&'static Point> {
    POINTS.iter().find(|point| point.name == name)
}

/// Lays a value out in the words of a point.
#[must_use]
pub fn encode(point: &Point, value: &Value) -> Vec<u16> {
    let mut words = match (point.data, value) {
        (Data::Bit, Value::Bit(on)) => vec![u16::from(*on)],
        (Data::U16 | Data::I16, Value::Number(n)) => vec![*n as u16],
        (Data::U32HighFirst, Value::Number(n)) => vec![(*n >> 16) as u16, *n as u16],
        (Data::U32LowFirst, Value::Number(n)) => vec![*n as u16, (*n >> 16) as u16],
        (Data::Ascii, Value::Text(text)) => text
            .as_bytes()
            .chunks(2)
            .map(|pair| u16::from_be_bytes([pair[0], pair.get(1).copied().unwrap_or(0)]))
            .collect(),
        _ => Vec::new(),
    };
    words.resize(usize::from(point.words), 0);
    words
}

/// Reads a value from the words of a point. Missing words count as zero.
#[must_use]
pub fn decode(point: &Point, words: &[u16]) -> Value {
    let word = |at: usize| words.get(at).copied().unwrap_or(0);
    match point.data {
        Data::Bit => Value::Bit(word(0) != 0),
        Data::U16 => Value::Number(i64::from(word(0))),
        Data::I16 => Value::Number(i64::from(word(0) as i16)),
        Data::U32HighFirst => Value::Number((i64::from(word(0)) << 16) | i64::from(word(1))),
        Data::U32LowFirst => Value::Number((i64::from(word(1)) << 16) | i64::from(word(0))),
        Data::Ascii => {
            let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_be_bytes()).collect();
            Value::Text(
                String::from_utf8_lossy(&bytes)
                    .trim_end_matches('\0')
                    .to_owned(),
            )
        }
        Data::Block => Value::Number(0),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn no_two_points_share_a_name_or_an_address() {
        let mut names = BTreeSet::new();
        let mut addresses = BTreeSet::new();
        for point in POINTS {
            assert!(names.insert(point.name), "{} is named twice", point.name);
            for index in 0..point.words {
                let address = (point.table, point.offset + index);
                assert!(
                    addresses.insert(address),
                    "{} overlaps another point",
                    point.name
                );
            }
            let bit_table = matches!(point.table, Coil | Discrete);
            assert_eq!(
                bit_table,
                point.data == Bit,
                "{} is in the wrong table",
                point.name
            );
            let read_table = matches!(point.table, Discrete | Input);
            assert!(
                !read_table || point.access == Read,
                "{} cannot be written",
                point.name
            );
            assert_eq!(by_name(point.name), Some(point));
        }
    }

    #[test]
    fn a_value_survives_its_layout_in_words() {
        let batch = by_name("batch_target").unwrap();
        assert_eq!(encode(batch, &Value::Number(100_000)), [0x86A0, 0x0001]);
        assert_eq!(decode(batch, &[0x86A0, 0x0001]), Value::Number(100_000));
        let uptime = by_name("uptime_ticks").unwrap();
        assert_eq!(encode(uptime, &Value::Number(100_000)), [0x0001, 0x86A0]);
        let offset = by_name("temperature_offset").unwrap();
        assert_eq!(encode(offset, &Value::Number(-50)), [0xFFCE]);
        assert_eq!(decode(offset, &[0xFFCE]), Value::Number(-50));
        let vendor = by_name("vendor").unwrap();
        let words = encode(vendor, &Value::Text("example".to_owned()));
        assert_eq!(words, [0x6578, 0x616D, 0x706C, 0x6500]);
        assert_eq!(decode(vendor, &words), Value::Text("example".to_owned()));
    }
}
