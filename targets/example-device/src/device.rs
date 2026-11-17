//! The simulated device: its points, its small process, and its update mechanism.
//!
//! Everything here is plain state changed by one request at a time. Nothing in
//! this module knows about sockets, faults, or the frame log.

use std::collections::BTreeMap;

use crate::firmware::{
    self, MAX_BLOCK_BYTES, MODEL_CODE, Slot, UpdateCommand, UpdateError, UpdateState, Version,
};
use crate::map::{self, Data, Point, PointId, Table, Value};
use crate::protocol::{self, Exception, Request};

/// The vendor the device reports.
pub const VENDOR: &str = "example";
/// The model the device reports.
pub const MODEL: &str = "example-controller";
/// The serial number the device reports.
pub const SERIAL_NUMBER: u32 = 100_123;
/// The raw value of the temperature sensor before the offset is added: -3.5 degrees.
pub const TEMPERATURE_SENSOR: i16 = -35;
/// Bit 0 of the fault latch: a write was refused for being out of range.
pub const LATCH_OUT_OF_RANGE: u16 = 0b01;
/// Bit 1 of the fault latch: an image failed its check.
pub const LATCH_IMAGE_FAILED: u16 = 0b10;

/// What a test may choose about a device before it starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeviceConfig {
    /// The unit identifier the device answers for. `0xFF` is always answered too.
    pub unit: u8,
    /// The firmware version in the active slot.
    pub firmware: Version,
    /// The firmware version in the standby slot, or `None` for a standby slot with no valid image.
    pub standby_firmware: Option<Version>,
    /// How far the measured pump speed moves toward the setpoint in one tick, in raw units.
    pub pump_ramp: u16,
}

impl Default for DeviceConfig {
    fn default() -> Self {
        Self {
            unit: 1,
            firmware: Version::new(2, 4, 0),
            standby_firmware: Some(Version::new(2, 3, 0)),
            pump_ramp: 4000,
        }
    }
}

/// The checksum of the image the stand pretends was installed for a version.
fn installed_checksum(version: Version) -> u32 {
    firmware::crc32(&firmware::build_image(MODEL_CODE, version, b"installed"))
}

pub(crate) struct Device {
    pub(crate) config: DeviceConfig,
    pub(crate) powered: bool,
    // The process.
    pub(crate) pump_setpoint: u16,
    pub(crate) pump_measured: u16,
    pub(crate) valve_command: bool,
    pub(crate) valve_position: bool,
    dosing_rate: u16,
    heater_power_limit: u16,
    temperature_offset: i16,
    batch_target: u32,
    fault_latch: u16,
    pub(crate) uptime: u32,
    // The update mechanism.
    image_length: u16,
    pub(crate) slots: [Slot; 2],
    pub(crate) active: usize,
    pub(crate) update_state: UpdateState,
    pub(crate) update_error: UpdateError,
    pub(crate) staged: Vec<u8>,
    pub(crate) next_block: u16,
    pub(crate) update_count: u16,
    checked: Option<firmware::Checked>,
    // Settings the control interface switches.
    pub(crate) frozen: bool,
    pub(crate) revert_on_link_loss: bool,
    pub(crate) stay_down_after_restart: bool,
    pub(crate) refuse_image: bool,
    // What the control interface reports.
    pub(crate) write_counts: BTreeMap<&'static str, u64>,
    pub(crate) restarts: u64,
    pub(crate) reverts: u64,
    /// Rises each time every link must close. Never reset, so a link opened
    /// before a reset still learns of a restart after it.
    pub(crate) cuts: u64,
}

impl Device {
    pub(crate) fn new(config: DeviceConfig) -> Self {
        let standby = config.standby_firmware.unwrap_or(config.firmware);
        Self {
            config,
            powered: true,
            pump_setpoint: 0,
            pump_measured: 0,
            valve_command: false,
            valve_position: false,
            dosing_rate: 250,
            heater_power_limit: 50,
            temperature_offset: 0,
            batch_target: 5000,
            fault_latch: 0,
            uptime: 0,
            image_length: 0,
            slots: [
                Slot {
                    valid: true,
                    version: config.firmware,
                    checksum: installed_checksum(config.firmware),
                },
                Slot {
                    valid: config.standby_firmware.is_some(),
                    version: standby,
                    checksum: installed_checksum(standby),
                },
            ],
            active: 0,
            update_state: UpdateState::Idle,
            update_error: UpdateError::None,
            staged: Vec::new(),
            next_block: 0,
            update_count: 1,
            checked: None,
            frozen: false,
            revert_on_link_loss: false,
            stay_down_after_restart: false,
            refuse_image: false,
            write_counts: BTreeMap::new(),
            restarts: 0,
            reverts: 0,
            cuts: 0,
        }
    }

    pub(crate) const fn standby(&self) -> usize {
        1 - self.active
    }

    /// The present value of a point, with no effect on the device.
    pub(crate) fn value(&self, id: PointId) -> Value {
        let number = |n: u16| Value::Number(i64::from(n));
        let running = self.slots[self.active];
        let standby = self.slots[self.standby()];
        match id {
            PointId::InletValve => Value::Bit(self.valve_command),
            PointId::FaultReset => Value::Bit(false),
            PointId::PumpRunning => Value::Bit(self.pump_measured > 0),
            PointId::ValveOpenFeedback => Value::Bit(self.valve_position),
            PointId::PumpSpeedSetpoint => number(self.pump_setpoint),
            PointId::DosingRate => number(self.dosing_rate),
            PointId::HeaterPowerLimit => number(self.heater_power_limit),
            PointId::TemperatureOffset => Value::Number(i64::from(self.temperature_offset)),
            PointId::BatchTarget => Value::Number(i64::from(self.batch_target)),
            PointId::RatedSpeed => Value::Number(map::PUMP_SPEED_MOST),
            PointId::UpdateCommand | PointId::UpdateBlock => Value::Number(0),
            PointId::UpdateImageLength => number(self.image_length),
            PointId::FaultLatch => number(self.fault_latch),
            PointId::PumpSpeedMeasured => number(self.pump_measured),
            PointId::Temperature => {
                Value::Number(i64::from(TEMPERATURE_SENSOR) + i64::from(self.temperature_offset))
            }
            PointId::UptimeTicks => Value::Number(i64::from(self.uptime)),
            PointId::FirmwareMajor => number(running.version.major),
            PointId::FirmwareMinor => number(running.version.minor),
            PointId::FirmwarePatch => number(running.version.patch),
            PointId::SerialNumber => Value::Number(i64::from(SERIAL_NUMBER)),
            PointId::Vendor => Value::Text(VENDOR.to_owned()),
            PointId::Model => Value::Text(MODEL.to_owned()),
            PointId::FirmwareChecksum => Value::Number(i64::from(running.checksum)),
            PointId::ActiveSlot => Value::Number(self.active as i64),
            PointId::StandbyValid => Value::Number(i64::from(standby.valid)),
            PointId::StandbyMajor => number(standby.version.major),
            PointId::StandbyMinor => number(standby.version.minor),
            PointId::StandbyPatch => number(standby.version.patch),
            PointId::UpdateState => number(self.update_state.code()),
            PointId::UpdateError => number(self.update_error.code()),
            PointId::UpdateNextBlock => number(self.next_block),
            PointId::UpdateBytesReceived => Value::Number(self.staged.len() as i64),
            PointId::UpdateCount => number(self.update_count),
        }
    }

    /// The word or bit at an address, with no effect on the device.
    pub(crate) fn peek(&self, table: Table, offset: u16) -> Option<u16> {
        let (point, index) = map::find(table, offset)?;
        map::encode(point, &self.value(point.id))
            .get(usize::from(index))
            .copied()
    }

    /// Carries out one request and returns the reply PDU, or the exception the device answers with.
    pub(crate) fn execute(&mut self, request: &Request) -> Result<Vec<u8>, Exception> {
        match request {
            Request::ReadCoils { address, quantity } => {
                let words = self.read(Table::Coil, *address, *quantity)?;
                Ok(protocol::bits_reply(protocol::READ_COILS, &bits(&words)))
            }
            Request::ReadDiscreteInputs { address, quantity } => {
                let words = self.read(Table::Discrete, *address, *quantity)?;
                Ok(protocol::bits_reply(
                    protocol::READ_DISCRETE_INPUTS,
                    &bits(&words),
                ))
            }
            Request::ReadHoldingRegisters { address, quantity } => {
                let words = self.read(Table::Holding, *address, *quantity)?;
                Ok(protocol::words_reply(
                    protocol::READ_HOLDING_REGISTERS,
                    &words,
                ))
            }
            Request::ReadInputRegisters { address, quantity } => {
                let words = self.read(Table::Input, *address, *quantity)?;
                Ok(protocol::words_reply(
                    protocol::READ_INPUT_REGISTERS,
                    &words,
                ))
            }
            Request::WriteSingleCoil { address, value } => {
                self.write_coil(*address, *value == protocol::COIL_ON)?;
                Ok(protocol::write_reply(
                    protocol::WRITE_SINGLE_COIL,
                    *address,
                    *value,
                ))
            }
            Request::WriteSingleRegister { address, value } => {
                self.write_registers(*address, &[*value])?;
                Ok(protocol::write_reply(
                    protocol::WRITE_SINGLE_REGISTER,
                    *address,
                    *value,
                ))
            }
            Request::WriteMultipleRegisters { address, values } => {
                self.write_registers(*address, values)?;
                let count = values.len() as u16;
                Ok(protocol::write_reply(
                    protocol::WRITE_MULTIPLE_REGISTERS,
                    *address,
                    count,
                ))
            }
            Request::ReadDeviceIdentification { code, object } => {
                self.identification(*code, *object)
            }
            Request::Unsupported { .. } | Request::BadHeader => Err(Exception::IllegalFunction),
            Request::Malformed { .. } => Err(Exception::IllegalDataValue),
        }
    }

    fn read(&mut self, table: Table, address: u16, quantity: u16) -> Result<Vec<u16>, Exception> {
        let mut words = Vec::with_capacity(usize::from(quantity));
        let mut clears_latch = false;
        for step in 0..quantity {
            let offset = address
                .checked_add(step)
                .ok_or(Exception::IllegalDataAddress)?;
            words.push(
                self.peek(table, offset)
                    .ok_or(Exception::IllegalDataAddress)?,
            );
            clears_latch |=
                map::find(table, offset).is_some_and(|(p, _)| p.id == PointId::FaultLatch);
        }
        // The read is whole before it has its effect: a refused read clears nothing.
        if clears_latch {
            self.fault_latch = 0;
        }
        Ok(words)
    }

    fn write_coil(&mut self, address: u16, on: bool) -> Result<(), Exception> {
        let (point, _) = map::find(Table::Coil, address).ok_or(Exception::IllegalDataAddress)?;
        if !point.access.writable() {
            return Err(Exception::IllegalDataAddress);
        }
        if self.update_state != UpdateState::Idle {
            return Err(Exception::ServerDeviceBusy);
        }
        match point.id {
            PointId::InletValve => self.valve_command = on,
            PointId::FaultReset if on => self.fault_latch = 0,
            _ => {}
        }
        self.count_write(point);
        Ok(())
    }

    fn write_registers(&mut self, address: u16, values: &[u16]) -> Result<(), Exception> {
        // Which points the frame covers. Each must be writable and covered whole.
        let mut covered: Vec<(&'static Point, Vec<u16>)> = Vec::new();
        for (step, word) in values.iter().enumerate() {
            let offset = u16::try_from(step)
                .ok()
                .and_then(|step| address.checked_add(step))
                .ok_or(Exception::IllegalDataAddress)?;
            let (point, index) =
                map::find(Table::Holding, offset).ok_or(Exception::IllegalDataAddress)?;
            if !point.access.writable() {
                return Err(Exception::IllegalDataAddress);
            }
            match covered.last_mut() {
                Some((last, words)) if last.id == point.id => words.push(*word),
                _ if index == 0 => covered.push((point, vec![*word])),
                _ => return Err(Exception::IllegalDataAddress),
            }
        }
        if let [(point, words)] = covered.as_slice()
            && point.data == Data::Block
        {
            self.take_block(words)?;
            self.count_write(point);
            return Ok(());
        }
        let alone = covered.len() == 1;
        for (point, words) in &covered {
            let whole = words.len() == usize::from(point.words);
            let must_be_alone = matches!(point.id, PointId::UpdateCommand | PointId::UpdateBlock);
            if !whole || (must_be_alone && !alone) {
                return Err(Exception::IllegalDataAddress);
            }
        }
        if let [(point, words)] = covered.as_slice()
            && point.id == PointId::UpdateCommand
        {
            let Value::Number(code) = map::decode(point, words) else {
                return Err(Exception::IllegalDataValue);
            };
            self.update_command(code)?;
            self.count_write(point);
            return Ok(());
        }
        if self.update_state != UpdateState::Idle {
            return Err(Exception::ServerDeviceBusy);
        }
        // Every value is checked before any is stored: a refused frame stores nothing.
        let mut accepted = Vec::with_capacity(covered.len());
        for (point, words) in &covered {
            let Value::Number(sent) = map::decode(point, words) else {
                return Err(Exception::IllegalDataValue);
            };
            if point
                .range
                .is_some_and(|(least, most)| sent < least || sent > most)
            {
                self.fault_latch |= LATCH_OUT_OF_RANGE;
                return Err(Exception::IllegalDataValue);
            }
            accepted.push((*point, sent));
        }
        for (point, sent) in accepted {
            self.store(point.id, sent);
            self.count_write(point);
        }
        Ok(())
    }

    /// Stores a value that passed its range check, as the device keeps it.
    fn store(&mut self, id: PointId, sent: i64) {
        match id {
            PointId::PumpSpeedSetpoint => self.pump_setpoint = sent as u16,
            PointId::DosingRate => {
                let step = map::DOSING_RATE_STEP;
                self.dosing_rate = (sent as u16 + step / 2) / step * step;
            }
            PointId::HeaterPowerLimit => {
                self.heater_power_limit = (sent as u16).min(map::HEATER_POWER_MOST);
            }
            PointId::TemperatureOffset => self.temperature_offset = sent as i16,
            PointId::BatchTarget => self.batch_target = sent as u32,
            PointId::UpdateImageLength => self.image_length = sent as u16,
            _ => {}
        }
    }

    fn count_write(&mut self, point: &'static Point) {
        *self.write_counts.entry(point.name).or_insert(0) += 1;
    }

    fn refuse(&mut self, error: UpdateError) -> Exception {
        self.update_error = error;
        Exception::ServerDeviceFailure
    }

    fn update_command(&mut self, code: i64) -> Result<(), Exception> {
        let command = UpdateCommand::from_code(code).ok_or(Exception::IllegalDataValue)?;
        match (command, self.update_state) {
            (UpdateCommand::Enter, UpdateState::Idle) => {
                if self.pump_setpoint > 0 || self.pump_measured > 0 {
                    return Err(self.refuse(UpdateError::PumpRunning));
                }
                if self.image_length == 0 {
                    return Err(self.refuse(UpdateError::LengthNotDeclared));
                }
                // The image is received into the standby slot, so what the slot held is gone.
                let standby = self.standby();
                self.slots[standby].valid = false;
                self.staged.clear();
                self.next_block = 0;
                self.checked = None;
                self.update_state = UpdateState::Receiving;
                self.update_error = UpdateError::None;
            }
            (UpdateCommand::Verify, UpdateState::Receiving) => self.verify(),
            (UpdateCommand::Commit, UpdateState::Verified) => {
                let Some(checked) = self.checked else {
                    return Err(self.refuse(UpdateError::WrongState));
                };
                let standby = self.standby();
                self.slots[standby] = Slot {
                    valid: true,
                    version: checked.version,
                    checksum: checked.checksum,
                };
                self.active = standby;
                self.update_count = self.update_count.saturating_add(1);
                self.restart();
            }
            (
                UpdateCommand::Abort,
                UpdateState::Receiving | UpdateState::Verified | UpdateState::Failed,
            ) => self.leave_update_mode(),
            (UpdateCommand::RollBack, UpdateState::Idle) => {
                if !self.slots[self.standby()].valid {
                    return Err(self.refuse(UpdateError::NoStandbyImage));
                }
                self.active = self.standby();
                self.restart();
            }
            _ => return Err(self.refuse(UpdateError::WrongState)),
        }
        Ok(())
    }

    fn verify(&mut self) {
        let outcome = if self.staged.len() != usize::from(self.image_length) {
            Err(UpdateError::ImageIncomplete)
        } else {
            firmware::check_image(&self.staged).and_then(|ok| {
                if self.refuse_image {
                    Err(UpdateError::ImageRefused)
                } else {
                    Ok(ok)
                }
            })
        };
        match outcome {
            Ok(checked) => {
                self.checked = Some(checked);
                self.update_state = UpdateState::Verified;
                self.update_error = UpdateError::None;
            }
            Err(error) => {
                self.checked = None;
                self.update_state = UpdateState::Failed;
                self.update_error = error;
                self.fault_latch |= LATCH_IMAGE_FAILED;
            }
        }
    }

    fn leave_update_mode(&mut self) {
        self.update_state = UpdateState::Idle;
        self.update_error = UpdateError::None;
        self.staged.clear();
        self.next_block = 0;
        self.checked = None;
    }

    /// One block: its number, its byte count, its checksum, then its bytes two to a word.
    fn take_block(&mut self, words: &[u16]) -> Result<(), Exception> {
        if self.update_state != UpdateState::Receiving {
            return Err(self.refuse(UpdateError::WrongState));
        }
        let [number, count, checksum, data @ ..] = words else {
            self.update_error = UpdateError::BlockLength;
            return Err(Exception::IllegalDataValue);
        };
        let count = usize::from(*count);
        let fits = self.staged.len() + count <= usize::from(self.image_length);
        if count == 0 || count > MAX_BLOCK_BYTES || data.len() != count.div_ceil(2) || !fits {
            self.update_error = UpdateError::BlockLength;
            return Err(Exception::IllegalDataValue);
        }
        if *number != self.next_block {
            return Err(self.refuse(UpdateError::BlockOutOfOrder));
        }
        let mut bytes: Vec<u8> = data.iter().flat_map(|w| w.to_be_bytes()).collect();
        bytes.truncate(count);
        if firmware::crc16(&bytes) != *checksum {
            self.update_error = UpdateError::BlockChecksum;
            return Err(Exception::IllegalDataValue);
        }
        self.staged.extend(bytes);
        self.next_block = self.next_block.saturating_add(1);
        self.update_error = UpdateError::None;
        Ok(())
    }

    /// Function 43, MEI type 14: what the device says it is.
    fn identification(&self, code: u8, object: u8) -> Result<Vec<u8>, Exception> {
        let objects: [(u8, String); 4] = [
            (0x00, VENDOR.to_owned()),
            (0x01, MODEL.to_owned()),
            (0x02, self.slots[self.active].version.to_string()),
            (0x80, SERIAL_NUMBER.to_string()),
        ];
        let chosen: Vec<&(u8, String)> = match code {
            1..=3 => {
                let top = [0x02, 0x7F, 0xFF][usize::from(code - 1)];
                let known = objects.iter().any(|(id, _)| *id == object && *id <= top);
                let start = if known { object } else { 0 };
                objects
                    .iter()
                    .filter(|(id, _)| (start..=top).contains(id))
                    .collect()
            }
            4 => vec![
                objects
                    .iter()
                    .find(|(id, _)| *id == object)
                    .ok_or(Exception::IllegalDataAddress)?,
            ],
            _ => return Err(Exception::IllegalDataValue),
        };
        // Conformity level 0x83: extended identification, by stream and by single object.
        // Nothing more follows, so the next object is zero.
        let mut pdu = vec![
            protocol::ENCAPSULATED_INTERFACE,
            protocol::MEI_READ_DEVICE_IDENTIFICATION,
            code,
            0x83,
            0x00,
            0x00,
            chosen.len() as u8,
        ];
        for (id, text) in chosen {
            pdu.push(*id);
            pdu.push(text.len() as u8);
            pdu.extend(text.as_bytes());
        }
        Ok(pdu)
    }

    /// One step of the device's own time: the uptime rises, the valve follows
    /// its command, and the pump moves one ramp step toward its setpoint.
    pub(crate) fn tick(&mut self) {
        if !self.powered {
            return;
        }
        self.uptime = self.uptime.wrapping_add(1);
        self.valve_position = self.valve_command;
        let ramp = self.config.pump_ramp;
        self.pump_measured = if self.pump_measured < self.pump_setpoint {
            self.pump_measured
                .saturating_add(ramp)
                .min(self.pump_setpoint)
        } else {
            self.pump_measured
                .saturating_sub(ramp)
                .max(self.pump_setpoint)
        };
    }

    /// Restarts the device: every link closes and every volatile point starts again.
    pub(crate) fn restart(&mut self) {
        self.pump_setpoint = 0;
        self.pump_measured = 0;
        self.valve_command = false;
        self.valve_position = false;
        self.fault_latch = 0;
        self.uptime = 0;
        self.image_length = 0;
        self.leave_update_mode();
        self.restarts += 1;
        self.cuts += 1;
        self.powered = !self.stay_down_after_restart;
    }

    /// What the device does by itself when its last link is lost, where that is switched on.
    pub(crate) fn revert_to_safe_state(&mut self) {
        if self.pump_setpoint > 0 || self.valve_command {
            self.pump_setpoint = 0;
            self.valve_command = false;
            self.reverts += 1;
        }
    }

    /// Puts another version in the active slot, as if it had been installed out of sight.
    pub(crate) fn set_firmware_version(&mut self, version: Version) {
        self.slots[self.active] = Slot {
            valid: true,
            version,
            checksum: installed_checksum(version),
        };
    }

    /// The recovery route: start from the standby slot if it holds an image,
    /// from the same slot otherwise. `false` if the standby slot held none.
    pub(crate) fn recover(&mut self) -> bool {
        let standby = self.standby();
        let had_standby = self.slots[standby].valid;
        if had_standby {
            self.active = standby;
        }
        self.restart();
        self.powered = true;
        had_standby
    }
}

fn bits(words: &[u16]) -> Vec<bool> {
    words.iter().map(|word| *word != 0).collect()
}
