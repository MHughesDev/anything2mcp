//! The control interface: ground truth about one running device, and its faults.
//!
//! A test holds a [`Control`] in the same process as the device. Nothing here
//! travels over the device's link, and no fault ever applies to it. A test
//! proves a write by asking here what the device stored.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use tokio::sync::watch;

use crate::device::{Device, DeviceConfig};
use crate::faults::{self, Corruption, Fault, FaultKind};
use crate::firmware::{Slot, UpdateError, UpdateState, Version};
use crate::map::{self, Table, Value};
use crate::protocol::{self, Exception, FrameKind, Header, Request};

/// What the device put on the wire in answer to a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    /// The function's normal reply.
    Normal,
    /// An exception reply with this code.
    Exception(u8),
    /// Nothing.
    None,
}

/// One frame the device received.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameRecord {
    /// Its place among the frames received since the last reset, from 1.
    pub sequence: u64,
    /// Which link it came in on. Links are numbered from 1 in the order they opened.
    pub link: u64,
    /// The transaction identifier in its header.
    pub transaction: u16,
    /// The unit identifier in its header.
    pub unit: u8,
    /// Its function code.
    pub function: u8,
    /// Whether it is a read, a write, or neither.
    pub kind: FrameKind,
    /// The request, decoded.
    pub request: Request,
    /// The whole frame as it arrived, header included.
    pub raw: Vec<u8>,
    /// Whether the device carried the request out. For a write: whether it was stored.
    pub applied: bool,
    /// What the device answered. With a corrupted reply, this is the answer before the damage.
    pub answer: Answer,
    /// The fault that applied to this frame, if one did.
    pub fault: Option<FaultKind>,
}

/// The state of the things the device drives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outputs {
    /// The pump's speed, in tenths of a revolution per minute.
    pub pump_speed: u16,
    /// Whether the valve is open.
    pub valve_open: bool,
}

/// The device's firmware and where its update mechanism stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FirmwareState {
    /// The version the device is running.
    pub version: Version,
    /// The checksum of the running image.
    pub checksum: u32,
    /// The slot the device started from: 0 or 1.
    pub active_slot: usize,
    /// Both slots.
    pub slots: [Slot; 2],
    /// The state of the update sequence.
    pub update_state: UpdateState,
    /// Why the last update step was refused.
    pub update_error: UpdateError,
    /// The number the next block must carry.
    pub next_block: u16,
    /// How many image bytes were received in this update.
    pub bytes_received: usize,
    /// How many images were ever committed.
    pub update_count: u16,
}

/// Everything the control interface knows, taken at one moment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    /// Whether the device is up. A device that is down closes every link at once.
    pub powered: bool,
    /// The present value of every point, by name. Numbers are raw.
    pub points: BTreeMap<&'static str, Value>,
    /// The state of the simulated outputs.
    pub outputs: Outputs,
    /// The firmware and the update state.
    pub firmware: FirmwareState,
    /// How many frames were received since the last reset.
    pub frames: usize,
    /// How many times each point was written since the last reset.
    pub write_counts: BTreeMap<&'static str, u64>,
    /// How many links are open now.
    pub open_links: usize,
    /// How many times the device restarted since the last reset.
    pub restarts: u64,
    /// How many times the device put itself in its safe state on losing its link.
    pub reverts: u64,
    /// How many faults are switched on.
    pub faults: usize,
}

/// What the link task does after the device has dealt with a frame.
pub(crate) struct Plan {
    /// The frame to send, if any.
    pub(crate) reply: Option<Vec<u8>>,
    /// How long to wait before sending it.
    pub(crate) delay_ms: Option<u64>,
    /// Whether the link closes afterwards.
    pub(crate) close: bool,
}

pub(crate) struct Inner {
    pub(crate) device: Device,
    frames: Vec<FrameRecord>,
    faults: Vec<Fault>,
    open_links: usize,
    links_opened: u64,
}

impl Inner {
    fn new(config: DeviceConfig) -> Self {
        Self {
            device: Device::new(config),
            frames: Vec::new(),
            faults: Vec::new(),
            open_links: 0,
            links_opened: 0,
        }
    }

    /// A link was accepted. `None` while the device is down: the link closes at once.
    pub(crate) fn link_opened(&mut self) -> Option<u64> {
        if !self.device.powered {
            return None;
        }
        self.open_links += 1;
        self.links_opened += 1;
        Some(self.links_opened)
    }

    /// A link closed, whoever closed it.
    pub(crate) fn link_closed(&mut self) {
        self.open_links = self.open_links.saturating_sub(1);
        if self.open_links == 0 && self.device.powered && self.device.revert_on_link_loss {
            self.device.revert_to_safe_state();
        }
    }

    fn record(&mut self, mut frame: FrameRecord) {
        frame.sequence = self.frames.len() as u64 + 1;
        self.frames.push(frame);
    }

    /// A header that is not this protocol's arrived. The link closes with no reply.
    pub(crate) fn bad_header(&mut self, link: u64, header: &Header, raw: Vec<u8>) {
        if self.device.powered {
            self.record(FrameRecord {
                sequence: 0,
                link,
                transaction: header.transaction,
                unit: header.unit,
                function: 0,
                kind: FrameKind::Other,
                request: Request::BadHeader,
                raw,
                applied: false,
                answer: Answer::None,
                fault: None,
            });
        }
    }

    fn execute(
        &mut self,
        header: &Header,
        request: &Request,
        refused: Option<Exception>,
    ) -> Result<Vec<u8>, Exception> {
        let unit = self.device.config.unit;
        if header.unit != unit && header.unit != protocol::UNIT_NOT_USED {
            return Err(Exception::GatewayTargetFailedToRespond);
        }
        match refused {
            Some(exception) => Err(exception),
            None => self.device.execute(request),
        }
    }

    /// One frame arrived: apply the fault that is due, let the device deal
    /// with the request, write the record, and let the device's time step once.
    pub(crate) fn handle(&mut self, link: u64, header: &Header, pdu: &[u8], raw: Vec<u8>) -> Plan {
        if !self.device.powered {
            return Plan {
                reply: None,
                delay_ms: None,
                close: true,
            };
        }
        let function = pdu.first().copied().unwrap_or(0);
        let (request, refused) = protocol::decode(pdu);
        let fault = faults::take(&mut self.faults, protocol::kind_of(function), function);
        let restarts = self.device.restarts;

        let mut plan = Plan {
            reply: None,
            delay_ms: None,
            close: false,
        };
        let (applied, answer) = match fault {
            Some(FaultKind::Exception { code }) => {
                plan.reply = Some(protocol::exception_reply(function, code));
                (false, Answer::Exception(code))
            }
            Some(FaultKind::Restart) => {
                self.device.restart();
                (false, Answer::None)
            }
            Some(FaultKind::DropLink { applied } | FaultKind::NoReply { applied }) => {
                plan.close = matches!(fault, Some(FaultKind::DropLink { .. }));
                let done = applied && self.execute(header, &request, refused).is_ok();
                (done, Answer::None)
            }
            Some(FaultKind::Delay { .. } | FaultKind::CorruptReply(_)) | None => {
                if let Some(FaultKind::Delay { ms }) = fault {
                    plan.delay_ms = Some(ms);
                }
                match self.execute(header, &request, refused) {
                    Ok(pdu) => {
                        plan.reply = Some(pdu);
                        (true, Answer::Normal)
                    }
                    Err(exception) => {
                        plan.reply = Some(protocol::exception_reply(function, exception.code()));
                        (false, Answer::Exception(exception.code()))
                    }
                }
            }
        };

        // The PDU becomes a frame, damaged if the fault says so.
        plan.reply = plan.reply.map(|pdu| {
            let mut transaction = header.transaction;
            if fault == Some(FaultKind::CorruptReply(Corruption::TransactionId)) {
                transaction = transaction.wrapping_add(1);
            }
            let mut bytes = protocol::frame(transaction, header.unit, &pdu);
            match fault {
                Some(FaultKind::CorruptReply(Corruption::LastByte)) => {
                    if let Some(last) = bytes.last_mut() {
                        *last = !*last;
                    }
                }
                Some(FaultKind::CorruptReply(Corruption::Truncated)) => {
                    bytes.pop();
                    plan.close = true;
                }
                _ => {}
            }
            bytes
        });

        self.record(FrameRecord {
            sequence: 0,
            link,
            transaction: header.transaction,
            unit: header.unit,
            function,
            kind: protocol::kind_of(function),
            request,
            raw,
            applied,
            answer,
            fault,
        });
        if self.device.restarts != restarts {
            // A restart closes this link too, after the reply if there is one.
            plan.close = true;
        } else if !self.device.frozen {
            self.device.tick();
        }
        plan
    }

    fn snapshot(&self) -> Snapshot {
        let device = &self.device;
        Snapshot {
            powered: device.powered,
            points: map::POINTS
                .iter()
                .map(|p| (p.name, device.value(p.id)))
                .collect(),
            outputs: Outputs {
                pump_speed: device.pump_measured,
                valve_open: device.valve_position,
            },
            firmware: FirmwareState {
                version: device.slots[device.active].version,
                checksum: device.slots[device.active].checksum,
                active_slot: device.active,
                slots: device.slots,
                update_state: device.update_state,
                update_error: device.update_error,
                next_block: device.next_block,
                bytes_received: device.staged.len(),
                update_count: device.update_count,
            },
            frames: self.frames.len(),
            write_counts: device.write_counts.clone(),
            open_links: self.open_links,
            restarts: device.restarts,
            reverts: device.reverts,
            faults: self.faults.len(),
        }
    }
}

/// What the device's tasks and the control interface share.
pub(crate) struct Shared {
    inner: Mutex<Inner>,
    /// Rises with every change, so a test can wait on a condition.
    revision: watch::Sender<u64>,
    /// Rises when every link must close: a restart, or the power going off.
    pub(crate) cut: watch::Sender<u64>,
}

impl Shared {
    pub(crate) fn new(config: DeviceConfig) -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(Inner::new(config)),
            revision: watch::Sender::new(0),
            cut: watch::Sender::new(0),
        })
    }

    fn read<R>(&self, look: impl FnOnce(&Inner) -> R) -> R {
        // A poisoned lock means a test panicked while holding it; the state is still the truth.
        look(&self.inner.lock().unwrap_or_else(PoisonError::into_inner))
    }

    pub(crate) fn change<R>(&self, act: impl FnOnce(&mut Inner) -> R) -> R {
        let mut inner = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        let out = act(&mut inner);
        let cuts = inner.device.cuts;
        drop(inner);
        self.cut
            .send_if_modified(|seen| std::mem::replace(seen, cuts) != cuts);
        self.revision.send_modify(|revision| *revision += 1);
        out
    }

    pub(crate) fn powered(&self) -> bool {
        self.read(|inner| inner.device.powered)
    }
}

/// The control interface of one running device. Cloning shares it.
#[derive(Clone)]
pub struct Control {
    shared: Arc<Shared>,
}

impl std::fmt::Debug for Control {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Control").finish_non_exhaustive()
    }
}

impl Control {
    pub(crate) fn new(shared: Arc<Shared>) -> Self {
        Self { shared }
    }

    /// Restores the starting state: points, slots, the update mechanism, and
    /// power. Clears the frame log, the write counts, every fault, and every
    /// setting. Open links stay open.
    pub fn reset(&self) {
        self.shared.change(|inner| {
            let cuts = inner.device.cuts;
            inner.device = Device::new(inner.device.config);
            inner.device.cuts = cuts;
            inner.frames.clear();
            inner.faults.clear();
        });
    }

    /// Everything the control interface knows, taken at one moment. Looking has no effect.
    #[must_use]
    pub fn inspect(&self) -> Snapshot {
        self.shared.read(Inner::snapshot)
    }

    /// The present value of a point, by its name in the point map. Numbers are raw.
    /// Looking has no effect, even on a point that a read over the link would clear.
    #[must_use]
    pub fn value(&self, name: &str) -> Option<Value> {
        let point = map::by_name(name)?;
        Some(self.shared.read(|inner| inner.device.value(point.id)))
    }

    /// The present value of a point that holds a number, or `None` for any other point.
    #[must_use]
    pub fn number(&self, name: &str) -> Option<i64> {
        match self.value(name)? {
            Value::Number(number) => Some(number),
            _ => None,
        }
    }

    /// The present value of a point that holds a bit, or `None` for any other point.
    #[must_use]
    pub fn bit(&self, name: &str) -> Option<bool> {
        match self.value(name)? {
            Value::Bit(on) => Some(on),
            _ => None,
        }
    }

    /// The word, or the bit as 0 or 1, the device holds at an address. `None` where it has no point.
    #[must_use]
    pub fn word(&self, table: Table, offset: u16) -> Option<u16> {
        self.shared.read(|inner| inner.device.peek(table, offset))
    }

    /// The state of the simulated outputs.
    #[must_use]
    pub fn outputs(&self) -> Outputs {
        self.inspect().outputs
    }

    /// The firmware version, the slots, and the update state.
    #[must_use]
    pub fn firmware(&self) -> FirmwareState {
        self.inspect().firmware
    }

    /// Every frame received since the last reset, oldest first.
    #[must_use]
    pub fn frames(&self) -> Vec<FrameRecord> {
        self.shared.read(|inner| inner.frames.clone())
    }

    /// Forgets the frames received so far. Nothing else changes.
    pub fn clear_frames(&self) {
        self.shared.change(|inner| inner.frames.clear());
    }

    /// How many times a point was written since the last reset. A refused write does not count.
    #[must_use]
    pub fn write_count(&self, name: &str) -> u64 {
        self.shared
            .read(|inner| inner.device.write_counts.get(name).copied().unwrap_or(0))
    }

    /// Switches a fault on.
    pub fn set_fault(&self, fault: Fault) {
        self.shared.change(|inner| inner.faults.push(fault));
    }

    /// The faults that are switched on now, in the order they are tried.
    #[must_use]
    pub fn faults(&self) -> Vec<Fault> {
        self.shared.read(|inner| inner.faults.clone())
    }

    /// Switches every frame fault off. Settings are left as they are.
    pub fn clear_faults(&self) {
        self.shared.change(|inner| inner.faults.clear());
    }

    /// Whether the device puts itself in its safe state (pump setpoint zero,
    /// valve closed) when its last link closes. Off at the start.
    pub fn set_revert_on_link_loss(&self, on: bool) {
        self.shared
            .change(|inner| inner.device.revert_on_link_loss = on);
    }

    /// Whether the device stays down after its next restart, whatever caused
    /// it: the device that does not come back. Off at the start.
    pub fn set_stay_down_after_restart(&self, on: bool) {
        self.shared
            .change(|inner| inner.device.stay_down_after_restart = on);
    }

    /// Whether the device refuses a well-formed image when asked to verify it. Off at the start.
    pub fn set_refuse_image(&self, on: bool) {
        self.shared.change(|inner| inner.device.refuse_image = on);
    }

    /// Whether the device's time stands still between calls to [`Control::advance`].
    /// Off at the start: every frame received is one tick.
    pub fn set_process_frozen(&self, on: bool) {
        self.shared.change(|inner| inner.device.frozen = on);
    }

    /// Puts another firmware version in the active slot, as if it had been installed out of sight.
    pub fn set_firmware_version(&self, version: Version) {
        self.shared
            .change(|inner| inner.device.set_firmware_version(version));
    }

    /// Lets the device's time step this many ticks, frozen or not.
    pub fn advance(&self, ticks: u32) {
        self.shared
            .change(|inner| (0..ticks).for_each(|_| inner.device.tick()));
    }

    /// Restarts the device: every link closes and volatile points start again.
    pub fn restart(&self) {
        self.shared.change(|inner| inner.device.restart());
    }

    /// Takes the power away: a restart from which the device does not come back by itself.
    pub fn power_off(&self) {
        self.shared.change(|inner| {
            inner.device.restart();
            inner.device.powered = false;
        });
    }

    /// Gives the power back. The device starts from its active slot.
    pub fn power_on(&self) {
        self.shared.change(|inner| inner.device.powered = true);
    }

    /// The device's recovery route, as a person would take it at the device:
    /// start from the standby slot. Returns `false`, and starts from the same
    /// slot, if the standby slot held no valid image.
    pub fn recover(&self) -> bool {
        self.shared.change(|inner| inner.device.recover())
    }

    /// Waits until a condition holds, looking again after every change, and
    /// returns whether it held within the time given. A test waits here in
    /// place of sleeping.
    pub async fn wait_until(
        &self,
        within: Duration,
        mut condition: impl FnMut(&Snapshot) -> bool,
    ) -> bool {
        let mut revision = self.shared.revision.subscribe();
        let held = async {
            loop {
                revision.borrow_and_update();
                if condition(&self.inspect()) {
                    return;
                }
                if revision.changed().await.is_err() {
                    std::future::pending::<()>().await;
                }
            }
        };
        tokio::time::timeout(within, held).await.is_ok()
    }
}
