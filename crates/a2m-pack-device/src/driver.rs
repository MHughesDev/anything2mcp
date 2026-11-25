//! The `device` driver: one link, one request at a time.
//!
//! `prepare` opens the link. Each step then does one thing: `open`
//! identifies the device against the step's map and the surface's serial,
//! `build_frame` builds the frame of a write and gives its bytes out to be
//! recorded, `write_frame` sends one frame, `read_frame` reads its reply,
//! and `expect` reads the reply in the form the map declares. A write is
//! checked against its register's range before any frame is built, its
//! frame is recorded before the step that sends it, and a write frame that
//! was sent and not clearly acknowledged is `possible_commit`. Nothing is
//! retried.
//!
//! The driver holds its own lines, whatever the compiled target says: no
//! frame but identification in the discovery posture, no write to a register
//! with the hazard `physical`, no write to a device whose serial the surface
//! does not declare, and no frame past the run's ceiling.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::Duration;

use a2m_auth::SessionLease;
use a2m_core::{CompiledTarget, Decimal, Digest, Failure, Hazard, Redact, Step, Value, Values};
use a2m_logs::{Acting, Detail};
use a2m_provider::{
    BoxFuture, Capture, Cleanup, Driver, DriverHealth, Observed, Posture, PrepareContext, Prepared,
    StepFailure, StepInput, StepOutcome, SurfaceRef,
};
use serde::Deserialize;

use crate::codec::{self, Unwritable};
use crate::codes;
use crate::firmware;
use crate::link::{self, Link, LinkError, OpenError, Refusal, Reply, Request, SendError};
use crate::lower::{StepPayload, before_name, frame_name};
use crate::map::{
    CompiledMap, CompiledRegister, CompiledUpdate, MapSection, ReadEffect, SECTION_FORMAT,
};
use crate::names::{Names, PACK};

/// How long a reply may take when neither a map nor a setting says.
pub(crate) const DEFAULT_REPLY_WITHIN_MS: u64 = 1000;
/// The least time between two frames when neither a map nor a setting
/// says: four frames a second at most.
pub(crate) const DEFAULT_MIN_GAP_MS: u64 = 250;
/// The most frames one run may send on one link when no setting says.
pub(crate) const DEFAULT_MAX_FRAMES: u64 = 100;

/// The pack's settings: the pace of a link whose map states none, and the
/// most frames one run may send.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Limits {
    pub reply_within: Duration,
    pub min_gap: Duration,
    pub max_frames: u32,
}

/// What a `device_link` surface states beside its address.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LinkFields {
    protocol: String,
    unit: u8,
    /// The serial of the device that answers in each environment, by the
    /// environment's name.
    #[serde(default)]
    serials: BTreeMap<String, String>,
}

/// The `device` driver.
pub(crate) struct DeviceDriver {
    names: Arc<Names>,
    redact: Arc<dyn Redact>,
    defaults: Limits,
}

/// The frame of a write, built and given out to be recorded, and not sent
/// yet.
struct Staged {
    register: String,
    request: Request,
    bytes: Vec<u8>,
}

/// Where a link goes, to open it again after the device restarts.
struct Endpoint {
    protocol: String,
    address: String,
    unit: u8,
}

/// An image that was checked and given out to be recorded, and waits to be
/// installed.
struct StagedImage {
    digest: Digest,
    version: String,
    bytes: Vec<u8>,
}

/// A frame that was sent and whose reply has not been read.
struct Pending {
    request: Request,
    sent: Vec<u8>,
}

/// A reply that was read and has not been looked at.
struct Unread {
    request: Request,
    sent: Vec<u8>,
    reply: Vec<u8>,
}

/// What `prepare` acquired: the open link and what the compiled target says
/// about it.
struct LinkState {
    /// The link; `None` once it was lost, after which nothing more is sent.
    link: Option<Box<dyn Link>>,
    /// The compiled target's maps, by the position of their section.
    maps: BTreeMap<usize, MapSection>,
    /// The maps the device on this link was compared with.
    identified: BTreeSet<(usize, String)>,
    /// The serial the surface declares for the device of the run's
    /// environment. Every identification is compared with it, and nothing
    /// is written where it is absent.
    serial: Option<String>,
    /// The write frame that was built and waits to be sent. While one
    /// waits, no other frame is sent.
    staged: Option<Staged>,
    /// The image that was checked and waits to be installed. While one
    /// waits, no other frame is sent.
    staged_image: Option<StagedImage>,
    /// Where the link goes.
    endpoint: Endpoint,
    /// What the device reported about itself when it was compared with a
    /// map.
    identity: BTreeMap<String, String>,
    /// How many more frames this run may send on the link.
    frames_left: u32,
    pending: Option<Pending>,
    unread: Option<Unread>,
    last_frame: Option<tokio::time::Instant>,
    posture: Posture,
}

fn fail(row: &a2m_core::CodeEntry, message: impl Into<String>, next: impl Into<String>) -> Failure {
    codes::failure(row, message, next)
}

fn compiled_target_unreadable(what: &str) -> StepFailure {
    StepFailure::not_sent(fail(
        &codes::row::PAYLOAD_UNREADABLE,
        format!("The compiled target cannot be carried out on a device link: {what}."),
        "Compile the target again with the device pack that is enabled now.",
    ))
}

fn link_lost_before() -> StepFailure {
    StepFailure::not_sent(fail(
        &codes::row::CONNECTION_LOST,
        "The link to the device was lost earlier in this run, so nothing more was sent.",
        "Open the link again; the device is identified again before anything is read.",
    ))
}

fn no_confirmation(what: &str) -> StepFailure {
    StepFailure::unknown(fail(
        &codes::row::NO_CONFIRMATION,
        format!(
            "A write frame was sent to the device and {what}, so the write may have been applied."
        ),
        "Reconcile: open the link, identify the device, and read the register before anything is written again.",
    ))
}

fn read_only() -> StepFailure {
    StepFailure::not_sent(fail(
        &a2m_provider::codes::row::READ_ONLY_POSTURE,
        "A write frame was asked for where nothing may be written.",
        "Run the operation live, as a step that commits.",
    ))
}

const fn may_write(posture: Posture) -> bool {
    matches!(
        posture,
        Posture::Live | Posture::Sandbox { test_records: true }
    )
}

fn budget_exceeded(what: &str) -> StepFailure {
    StepFailure::not_sent(fail(
        &codes::row::FRAME_BUDGET_EXCEEDED,
        format!("{what} Nothing was sent."),
        "Do the work in smaller runs, or raise the device pack's setting `max_frames_per_run`.",
    ))
}

/// The bytes a hexadecimal text stands for, or `None` when it is not one.
fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.is_ascii() || !text.len().is_multiple_of(2) {
        return None;
    }
    text.as_bytes()
        .chunks(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok())
        .collect()
}

/// What the device holds, set beside what the frame recorded for the
/// register would have written. It is evidence, in the register's raw
/// layout, where the engine compares the normalized values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FrameCheck {
    /// The device holds what the frame carried.
    Matches,
    /// The device holds something else.
    Differs,
    /// The recorded frame is not one this link's protocol reads back into
    /// a write of this register.
    Unreadable,
}

impl FrameCheck {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Matches => "matches",
            Self::Differs => "differs",
            Self::Unreadable => "unreadable",
        }
    }
}

/// How a step was done, as the capture the trace takes. `means` is what the
/// trace's signature covers: the kind of step, what it does, and the
/// register it concerns. `measured` is what it does not.
fn acting(step: &Step, payload: &StepPayload, framed: bool, measured: Vec<Detail>) -> Capture {
    let (what, register) = match payload {
        StepPayload::Identify => ("identify", None),
        StepPayload::Report => ("report", None),
        StepPayload::AskRead { register } => ("ask_read", Some(register)),
        StepPayload::Build { register, .. } => ("build", Some(register)),
        StepPayload::Write { register, .. } => ("write", Some(register)),
        StepPayload::Reply => ("reply", None),
        StepPayload::Value { register, .. } => ("value", Some(register)),
        StepPayload::Before { register } => ("before", Some(register)),
        StepPayload::Acknowledged => ("acknowledged", None),
        StepPayload::Precondition { register, .. } => ("precondition", Some(register)),
        StepPayload::BuildFirmware { .. } => ("build_firmware", None),
        StepPayload::Install { .. } => ("install", None),
        StepPayload::AfterUpdate { .. } => ("after_update", None),
        StepPayload::Checksum { register, .. } => ("checksum", Some(register)),
    };
    let mut means = vec![
        Detail::new("step", step.kind.as_str()),
        Detail::new("does", what),
    ];
    if let Some(register) = register {
        means.push(Detail::new("register", register.as_str()));
    }
    if framed {
        means.push(Detail::new("compared_with", "recorded_frame"));
    }
    Capture::acting(&Acting { means, measured })
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    })
}

impl DeviceDriver {
    pub fn new(names: Arc<Names>, redact: Arc<dyn Redact>, defaults: Limits) -> Self {
        Self {
            names,
            redact,
            defaults,
        }
    }

    /// The compiled target's sections that this pack compiled, decoded.
    fn maps(
        &self,
        compiled_target: &CompiledTarget,
    ) -> Result<BTreeMap<usize, MapSection>, StepFailure> {
        let mut maps = BTreeMap::new();
        for (index, section) in compiled_target.packs.iter().enumerate() {
            if section.pack != PACK || section.kind != self.names.register_kind {
                continue;
            }
            if section.format != SECTION_FORMAT {
                return Err(compiled_target_unreadable(
                    "its maps were compiled in a form this pack does not read",
                ));
            }
            let decoded = section
                .payload
                .decode::<MapSection>()
                .map_err(|_| compiled_target_unreadable("its maps do not decode"))?;
            maps.insert(index, decoded);
        }
        Ok(maps)
    }

    /// The exchange a step kept as evidence: the frame sent, and the size
    /// of what came back. A reply's content is given out only as values.
    fn capture(&self, sent: &[u8], reply: &[u8]) -> Capture {
        let text = format!(
            "{{\"reply_bytes\":{},\"sent\":\"{}\"}}",
            reply.len(),
            hex(sent)
        );
        Capture {
            name: "exchange".to_owned(),
            media_type: "application/json".to_owned(),
            bytes: self.redact.redact(&text).into_bytes(),
        }
    }
}

impl LinkState {
    fn map<'a>(&'a self, step: &'a Step) -> Result<(usize, &'a str, &'a CompiledMap), StepFailure> {
        let refers = step
            .refers_to
            .as_ref()
            .ok_or_else(|| compiled_target_unreadable("a step names no map"))?;
        let index = refers.section.get();
        self.maps
            .get(&index)
            .and_then(|section| section.get(&refers.entry))
            .map(|map| (index, refers.entry.as_str(), map))
            .ok_or_else(|| {
                compiled_target_unreadable("a step names a map the compiled target does not hold")
            })
    }

    /// A register of the step's map, once the device was compared with it.
    fn register(
        &self,
        step: &Step,
        register: &str,
    ) -> Result<(CompiledRegister, CompiledMap), StepFailure> {
        let (index, name, map) = self.map(step)?;
        if !self.identified.contains(&(index, name.to_owned())) {
            return Err(StepFailure::not_sent(fail(
                &codes::row::DEVICE_IDENTITY_MISMATCH,
                "The device on this link was not compared with the map, so nothing was sent.",
                "Identify the device first: every binding of the device mode starts with an `open` step.",
            )));
        }
        map.registers
            .get(register)
            .map(|found| (found.clone(), map.clone()))
            .ok_or_else(|| {
                compiled_target_unreadable("a step names a register its map does not hold")
            })
    }

    /// What every frame passes before it is built, whatever step asks for
    /// it: the posture, the order of frames, and the run's ceiling.
    fn may_send(&self, request: &Request) -> Result<(), StepFailure> {
        if request.is_write() && !may_write(self.posture) {
            return Err(read_only());
        }
        // Discovery asks a device what it is, and nothing else: a read is
        // not always without effect on a device.
        if self.posture == Posture::Discovery && *request != Request::Identify {
            return Err(StepFailure::not_sent(fail(
                &codes::row::DISCOVERY_IDENTIFIES_ONLY,
                "A frame other than identification was asked for during discovery, so nothing was sent.",
                "Read a register through an operation, in a run that is not discovery.",
            )));
        }
        if self.pending.is_some() {
            return Err(compiled_target_unreadable(
                "a frame is sent while another still waits for its reply",
            ));
        }
        if self.staged.is_some() || self.staged_image.is_some() {
            return Err(compiled_target_unreadable(
                "a frame is sent while a write frame that was built still waits to be sent",
            ));
        }
        if self.frames_left == 0 {
            return Err(budget_exceeded(
                "The run reached the most frames it may send on a device link.",
            ));
        }
        Ok(())
    }

    /// Builds the frame for a request. Nothing is sent.
    fn frame(&mut self, request: &Request) -> Result<Vec<u8>, StepFailure> {
        let Some(link) = self.link.as_mut() else {
            return Err(link_lost_before());
        };
        link.frame(request)
            .ok_or_else(|| compiled_target_unreadable("the link's protocol cannot carry a request"))
    }

    /// Builds the frame of a request that reads or identifies, and sends
    /// it.
    async fn ask(
        &mut self,
        request: Request,
        gap: Duration,
        within: Duration,
    ) -> Result<(), StepFailure> {
        if request.is_write() {
            return Err(compiled_target_unreadable(
                "a write frame is sent that no step built and recorded first",
            ));
        }
        self.may_send(&request)?;
        let bytes = self.frame(&request)?;
        self.send(request, bytes, gap, within).await
    }

    /// Waits until the gap since the last frame has passed, then sends the
    /// frame. The caller passed [`LinkState::may_send`] first.
    async fn send(
        &mut self,
        request: Request,
        bytes: Vec<u8>,
        gap: Duration,
        within: Duration,
    ) -> Result<(), StepFailure> {
        let Some(link) = self.link.as_mut() else {
            return Err(link_lost_before());
        };
        if let Some(last) = self.last_frame {
            tokio::time::sleep_until(last + gap).await;
        }
        let write = request.is_write();
        // A frame counts from the moment its first byte may leave.
        self.frames_left = self.frames_left.saturating_sub(1);
        match link.send(&bytes, within).await {
            Ok(()) => {
                self.last_frame = Some(tokio::time::Instant::now());
                self.pending = Some(Pending {
                    request,
                    sent: bytes,
                });
                Ok(())
            }
            Err(SendError::NotSent) => {
                self.link = None;
                Err(link_lost_before())
            }
            Err(SendError::Unknown) => {
                self.link = None;
                Err(if write {
                    no_confirmation("the link failed while it was being sent")
                } else {
                    StepFailure::unknown(fail(
                        &codes::row::CONNECTION_LOST,
                        "The link to the device failed while a read request was being sent.",
                        "Open the link again and read again.",
                    ))
                })
            }
        }
    }

    /// Reads the reply to the pending frame.
    async fn receive(&mut self, within: Duration) -> Result<(), StepFailure> {
        let Some(pending) = self.pending.take() else {
            return Err(compiled_target_unreadable(
                "a reply is read and no frame waits for one",
            ));
        };
        let Some(link) = self.link.as_mut() else {
            return Err(link_lost_before());
        };
        let write = pending.request.is_write();
        match link.receive(within).await {
            Ok(reply) => {
                self.unread = Some(Unread {
                    request: pending.request,
                    sent: pending.sent,
                    reply,
                });
                Ok(())
            }
            Err(error) => {
                self.link = None;
                Err(match (write, error) {
                    (true, LinkError::Closed) => {
                        no_confirmation("the link closed before an acknowledgement came")
                    }
                    (true, LinkError::TimedOut) => {
                        no_confirmation("no acknowledgement came in time")
                    }
                    (true, LinkError::Corrupt) => {
                        no_confirmation("the acknowledgement that came was damaged")
                    }
                    (false, LinkError::Closed) => StepFailure::unknown(fail(
                        &codes::row::CONNECTION_LOST,
                        "The link to the device closed before the reply to a read came.",
                        "Open the link again and read again.",
                    )),
                    (false, LinkError::TimedOut) => StepFailure::unknown(fail(
                        &codes::row::TIMEOUT,
                        "The device did not reply to a read in the time its map allows.",
                        "Read again; if it keeps happening, check the link and the map's `pace`.",
                    )),
                    (false, LinkError::Corrupt) => StepFailure::unknown(fail(
                        &codes::row::FRAME_CORRUPT,
                        "The reply to a read was damaged or was not for the request.",
                        "Open the link again and read again.",
                    )),
                })
            }
        }
    }

    /// The reply last read, in the form its protocol declares.
    fn answer(&mut self) -> Result<(Unread, Option<Reply>), StepFailure> {
        let unread = self
            .unread
            .take()
            .ok_or_else(|| compiled_target_unreadable("a reply is expected and none was read"))?;
        let reply = self
            .link
            .as_ref()
            .and_then(|link| link.interpret(&unread.request, &unread.reply));
        Ok((unread, reply))
    }
}

/// The failure for a refusal the device answered a read with.
fn refused_read(refusal: Refusal) -> StepFailure {
    StepFailure::answered(match refusal {
        Refusal::Busy => fail(
            &codes::row::DEVICE_BUSY,
            "The device answered that it is busy and did not carry out the read.",
            "Read again later.",
        ),
        Refusal::NoAnswerBehindGateway => fail(
            &codes::row::TIMEOUT,
            "A gateway answered that the device behind it did not respond.",
            "Check the unit address of the surface, then read again.",
        ),
        Refusal::Function | Refusal::Address | Refusal::Value | Refusal::Other(_) => fail(
            &codes::row::DEVICE_REFUSED,
            "The device refused the read with an exception.",
            "Check the register's address and layout in the map against the device's documentation.",
        ),
    })
}

/// The failure for a refusal the device answered a write with.
fn refused_write(refusal: Refusal) -> StepFailure {
    match refusal {
        // Busy says the request was not accepted. It is still never sent
        // again by itself: the step is not idempotent.
        Refusal::Busy => StepFailure::answered(fail(
            &codes::row::DEVICE_BUSY,
            "The device answered that it is busy and did not carry out the write.",
            "Nothing was written and nothing is sent again by itself; run the operation again when the device is free.",
        )),
        Refusal::Function | Refusal::Address | Refusal::Value => StepFailure::answered(fail(
            &codes::row::DEVICE_REFUSED,
            "The device refused the write with an exception: an address or a value it will not take. Nothing was written.",
            "Check the register's range and access in the map against the device's documentation.",
        )),
        // A failure while carrying the write out, or an answer the protocol
        // does not explain: nothing says whether the write was applied.
        Refusal::NoAnswerBehindGateway | Refusal::Other(_) => no_confirmation(
            "the device answered with a failure that does not say whether it was applied",
        ),
    }
}

impl DeviceDriver {
    async fn identify(
        &self,
        state: &mut LinkState,
        input: &StepInput<'_>,
        reply_within: Duration,
    ) -> Result<(BTreeMap<String, String>, Capture), StepFailure> {
        let within = reply_within.min(input.deadline.remaining());
        state
            .ask(Request::Identify, self.defaults.min_gap, within)
            .await?;
        state.receive(within).await?;
        let (unread, reply) = state.answer()?;
        let capture = self.capture(&unread.sent, &unread.reply);
        match reply {
            Some(Reply::Identity(found)) => Ok((found, capture)),
            Some(Reply::Refused(refusal)) => Err(refused_read(refusal)),
            _ => Err(StepFailure::answered(fail(
                &codes::row::UNEXPECTED_REPLY,
                "The device answered the request to identify itself with a reply of no declared form.",
                "Check that the surface names the protocol the device speaks.",
            ))),
        }
    }

    async fn open(
        &self,
        state: &mut LinkState,
        step: &Step,
        input: &StepInput<'_>,
        report: bool,
    ) -> Result<StepOutcome, StepFailure> {
        if report {
            let (found, capture) = self
                .identify(state, input, self.defaults.reply_within)
                .await?;
            let values: Values = found
                .into_iter()
                .map(|(name, text)| (name, Value::Text(self.redact.redact(&text))))
                .collect();
            return Ok(StepOutcome::Done(Observed {
                values,
                captures: vec![capture],
                ..Observed::default()
            }));
        }
        let (index, name, map) = state.map(step)?;
        let key = (index, name.to_owned());
        if state.identified.contains(&key) {
            return Ok(StepOutcome::Done(Observed::default()));
        }
        let map = map.clone();
        let reply_within = self.reply_within(&map);
        let (found, capture) = self.identify(state, input, reply_within).await?;
        let mismatch = |what: &str| {
            StepFailure::answered(fail(
                &codes::row::DEVICE_IDENTITY_MISMATCH,
                format!(
                    "The device on this link {what} the map `{}`, so nothing is read from it or written to it.",
                    key.1
                ),
                "Prove the map against this device in a test environment, and add its firmware version to the map's `firmware` list.",
            ))
        };
        if let Some(missing) = map
            .identify
            .iter()
            .find(|field| !found.contains_key(*field))
        {
            return Err(mismatch(&format!(
                "does not report its `{missing}`, which is needed to compare it with"
            )));
        }
        if found.get("model") != Some(&map.model) {
            return Err(mismatch("reports another model than"));
        }
        if !found
            .get("firmware_version")
            .is_some_and(|version| map.firmware.contains(version))
        {
            return Err(mismatch(
                "reports a firmware version that is not on the list of",
            ));
        }
        // The unit, not only its model: a map proven on one device must
        // not drive another of the same model that answers at the address.
        if let Some(declared) = &state.serial
            && found.get("serial") != Some(declared)
        {
            return Err(StepFailure::answered(fail(
                &codes::row::DEVICE_IDENTITY_MISMATCH,
                "The device on this link does not report the serial its surface declares for this environment: it is another unit, so nothing is read from it or written to it.",
                "Check the surface's address in this environment. If the unit was replaced, prove the map against the new unit in a test environment and declare its serial under `serials:`.",
            )));
        }
        state.identified.insert(key);
        state.identity = found;
        Ok(StepOutcome::Done(Observed {
            captures: vec![capture],
            ..Observed::default()
        }))
    }

    /// The map of the step, once the device was compared with it, and the
    /// update mechanism it declares. Anything that writes needs the serial
    /// the surface declares, too.
    fn firmware_scene(
        state: &LinkState,
        step: &Step,
        writing: bool,
    ) -> Result<(CompiledMap, CompiledUpdate), StepFailure> {
        let (index, name, map) = state.map(step)?;
        if !state.identified.contains(&(index, name.to_owned())) {
            return Err(StepFailure::not_sent(fail(
                &codes::row::DEVICE_IDENTITY_MISMATCH,
                "The device on this link was not compared with the map, so nothing was sent.",
                "Identify the device first: every binding of the device mode starts with an `open` step.",
            )));
        }
        if writing && state.serial.is_none() {
            return Err(StepFailure::not_sent(fail(
                &codes::row::DEVICE_SERIAL_NOT_DECLARED,
                "The surface declares no serial for the device of this environment, so nothing is written to it. No frame was sent.",
                "Declare the unit's serial for this environment under `serials:` on the surface, after proving the map against that unit.",
            )));
        }
        let update = map.firmware_update.clone().ok_or_else(|| {
            compiled_target_unreadable(
                "a step updates firmware on a map that declares no update mechanism",
            )
        })?;
        Ok((map.clone(), update))
    }

    /// The image an update sends, once it passed every check made before
    /// a byte of it is sent: it is hexadecimal text, of the form an image
    /// has, with a checksum that holds, no larger than the device takes,
    /// carrying the version the operation names, and on the operation's
    /// list of digests where there is one.
    fn image(
        input: &StepInput<'_>,
        image: &str,
        version: &str,
        update: &CompiledUpdate,
        accepts: &[String],
    ) -> Result<Vec<u8>, StepFailure> {
        let refuse = |why: &str| {
            StepFailure::not_sent(fail(
                &codes::row::IMAGE_NOT_ACCEPTED,
                format!("The firmware image was not sent: {why}."),
                "Give the image the device's maker issued for this model, as hexadecimal text, with the version it carries.",
            ))
        };
        let bytes = input
            .args
            .get(image)
            .and_then(Value::as_text)
            .and_then(unhex)
            .ok_or_else(|| refuse("it is absent or is not hexadecimal text"))?;
        if bytes.len() > usize::try_from(update.max_image_bytes).unwrap_or(usize::MAX) {
            return Err(refuse("it is larger than the device takes"));
        }
        let form =
            firmware::read(&bytes).map_err(|_| refuse("it is not of the form an image has"))?;
        if !form.checksum_holds() {
            return Err(refuse("its checksum does not hold, so it is damaged"));
        }
        if input.args.get(version).and_then(Value::as_text) != Some(form.version.as_str()) {
            return Err(refuse(
                "it carries another version than the one the operation names",
            ));
        }
        if !accepts.is_empty() && !accepts.contains(&Digest::of(&bytes).to_string()) {
            return Err(refuse("its digest is not on the operation's list"));
        }
        Ok(bytes)
    }

    /// Checks the image, and gives out what is recorded before the step that
    /// installs it: the image's digest and length, how it will be cut, what
    /// the operation declares, and the version the device runs now. Nothing
    /// is sent.
    fn build_firmware(
        &self,
        state: &mut LinkState,
        step: &Step,
        input: &StepInput<'_>,
        payload: &StepPayload,
    ) -> Result<StepOutcome, StepFailure> {
        let StepPayload::BuildFirmware {
            image,
            version,
            accepts,
            recovery,
            cannot_undo,
            signature_checked,
            reads_after,
        } = payload
        else {
            return Err(compiled_target_unreadable(
                "a `build_frame` step builds no frame",
            ));
        };
        if !may_write(state.posture) {
            return Err(read_only());
        }
        let (_, update) = Self::firmware_scene(state, step, true)?;
        let bytes = Self::image(input, image, version, &update, accepts)?;
        state.may_send(&Request::WriteWords {
            offset: update.command,
            words: vec![firmware::ENTER],
        })?;
        let blocks = bytes.len().div_ceil(usize::from(update.block_bytes.max(1)));
        // An update that could not be read back afterwards is not started.
        if firmware::frames(bytes.len(), update.block_bytes, *reads_after) > state.frames_left {
            return Err(budget_exceeded(
                "The frames this run may still send on the device link would not cover the update and its read-back, so the update was not started.",
            ));
        }
        let digest = Digest::of(&bytes);
        let current = state
            .identity
            .get("firmware_version")
            .cloned()
            .unwrap_or_default();
        let evidence = serde_json::json!({
            "digest": digest.to_string(),
            "bytes": bytes.len(),
            "blocks": blocks,
            "from_version": current,
            "to_version": input.args.get(version).and_then(Value::as_text),
            "recovery": recovery,
            "cannot_undo": cannot_undo,
            "signature_checked": signature_checked,
        })
        .to_string();
        let mut observed = Observed {
            captures: vec![Capture {
                name: "firmware".to_owned(),
                media_type: "application/json".to_owned(),
                bytes: self.redact.redact(&evidence).into_bytes(),
            }],
            ..Observed::default()
        };
        observed.for_reconcile.insert(
            "firmware_image_digest".to_owned(),
            Value::Text(digest.to_string()),
        );
        observed
            .for_reconcile
            .insert("before_firmware_version".to_owned(), Value::Text(current));
        state.staged_image = Some(StagedImage {
            digest,
            version: input
                .args
                .get(version)
                .and_then(Value::as_text)
                .unwrap_or_default()
                .to_owned(),
            bytes,
        });
        Ok(StepOutcome::Done(observed))
    }

    /// One write of the update's sequence, sent and answered. A refusal
    /// before update mode was entered changes nothing. A refusal after it
    /// asks the device to leave update mode, and ends the update as refused.
    async fn update_write(
        &self,
        state: &mut LinkState,
        step: &Step,
        update: &CompiledUpdate,
        (gap, within): (Duration, Duration),
        (offset, words): (u16, Vec<u16>),
        entered: bool,
    ) -> Result<(), StepFailure> {
        let request = Request::WriteWords { offset, words };
        state.may_send(&request)?;
        let bytes = state.frame(&request)?;
        state.send(request, bytes, gap, within).await?;
        state.receive(within).await?;
        let (_, reply) = state.answer()?;
        match reply {
            Some(Reply::Written) => Ok(()),
            Some(Reply::Refused(refusal)) => {
                // The device's own word for a refusal of an update command.
                let definite = matches!(
                    refusal,
                    Refusal::Function | Refusal::Address | Refusal::Value | Refusal::Other(4)
                );
                if !entered {
                    return Err(refused_write(if refusal == Refusal::Other(4) {
                        Refusal::Value
                    } else {
                        refusal
                    }));
                }
                if !definite {
                    state.link = None;
                    return Err(no_confirmation(
                        "the device answered a block with a failure that does not say whether it was applied",
                    ));
                }
                let why = self.update_error(state, step, update, (gap, within)).await;
                Err(self
                    .leave_update_mode(state, update, (gap, within), why)
                    .await)
            }
            _ => {
                state.link = None;
                Err(no_confirmation(
                    "the reply that came does not acknowledge it",
                ))
            }
        }
    }

    /// The number a register of the update's mechanism holds, read now.
    async fn read_number(
        &self,
        state: &mut LinkState,
        step: &Step,
        register: &str,
        (gap, within): (Duration, Duration),
    ) -> Result<i64, StepFailure> {
        let (found, _) = state.register(step, register)?;
        state.ask(codec::read_request(&found), gap, within).await?;
        state.receive(within).await?;
        let (_, reply) = state.answer()?;
        match reply {
            Some(Reply::Refused(refusal)) => Err(refused_read(refusal)),
            Some(reply) => codec::decode(&found, &reply)
                .and_then(|value| value.as_int())
                .ok_or_else(|| {
                    StepFailure::answered(fail(
                        &codes::row::UNEXPECTED_REPLY,
                        "The device answered a read of the update's state with a reply of no form its map declares.",
                        "Check the register's layout in the map against the device's documentation.",
                    ))
                }),
            None => Err(StepFailure::answered(fail(
                &codes::row::UNEXPECTED_REPLY,
                "The device answered a read of the update's state with a reply of no declared form.",
                "Check the register's layout in the map against the device's documentation.",
            ))),
        }
    }

    /// Why the device refused the last step of its update, when it can be
    /// read.
    async fn update_error(
        &self,
        state: &mut LinkState,
        step: &Step,
        update: &CompiledUpdate,
        timing: (Duration, Duration),
    ) -> Option<i64> {
        state.link.as_ref()?;
        self.read_number(state, step, &update.error, timing)
            .await
            .ok()
    }

    /// Asks the device to leave update mode, and says the image was
    /// refused. The device's other slot held an image until update mode
    /// began: it holds none now.
    async fn leave_update_mode(
        &self,
        state: &mut LinkState,
        update: &CompiledUpdate,
        (gap, within): (Duration, Duration),
        why: Option<i64>,
    ) -> StepFailure {
        let mut left = false;
        if state.link.is_some() {
            let request = Request::WriteWords {
                offset: update.command,
                words: vec![firmware::ABORT],
            };
            if state.may_send(&request).is_ok()
                && let Ok(bytes) = state.frame(&request)
                && state.send(request, bytes, gap, within).await.is_ok()
                && state.receive(within).await.is_ok()
            {
                left = matches!(state.answer(), Ok((_, Some(Reply::Written))));
            }
        }
        let code = why.map_or_else(String::new, |code| format!(" (update error {code})"));
        StepFailure::answered(fail(
            &codes::row::IMAGE_REFUSED,
            format!(
                "The device refused the image{code}. {} The image its other slot held was given up when update mode began.",
                if left {
                    "It was told to leave update mode, and did."
                } else {
                    "It could not be told to leave update mode; check its state before anything else is sent."
                }
            ),
            "Check the image and the device's reason against its documentation; the device still runs its present firmware.",
        ))
    }

    /// Carries out the device's own update sequence for the image the step
    /// before built, and sends the commit. The commit's reply is left for
    /// the steps that confirm. Nothing is sent twice: a frame whose fate is
    /// unknown ends the update as `possible_commit` and the link is
    /// dropped.
    async fn install(
        &self,
        state: &mut LinkState,
        step: &Step,
        input: &StepInput<'_>,
        payload: &StepPayload,
    ) -> Result<StepOutcome, StepFailure> {
        let StepPayload::Install { image, version } = payload else {
            return Err(compiled_target_unreadable("a step installs nothing"));
        };
        if !step.commits || !may_write(state.posture) {
            return Err(read_only());
        }
        let (map, update) = Self::firmware_scene(state, step, true)?;
        let bytes = Self::image(input, image, version, &update, &[])?;
        // The image sent is the image that was recorded, or none.
        let staged = state
            .staged_image
            .take()
            .filter(|staged| {
                staged.digest == Digest::of(&bytes)
                    && staged.bytes == bytes
                    && input.args.get(version).and_then(Value::as_text)
                        == Some(staged.version.as_str())
            })
            .ok_or_else(|| {
                compiled_target_unreadable(
                    "an image is installed that was not built and recorded first, or that changed since",
                )
            })?;
        let timing = (
            self.gap(&map),
            self.reply_within(&map).min(input.deadline.remaining()),
        );
        let length = u16::try_from(staged.bytes.len()).map_err(|_| {
            compiled_target_unreadable("an image is longer than the device's length register")
        })?;
        self.update_write(
            state,
            step,
            &update,
            timing,
            (update.length, vec![length]),
            false,
        )
        .await?;
        self.update_write(
            state,
            step,
            &update,
            timing,
            (update.command, vec![firmware::ENTER]),
            false,
        )
        .await?;
        // From here the device's other slot is empty, and anything that is
        // not a clear refusal may have changed it.
        for words in firmware::blocks(&staged.bytes, update.block_bytes) {
            self.update_write(state, step, &update, timing, (update.block, words), true)
                .await?;
        }
        self.update_write(
            state,
            step,
            &update,
            timing,
            (update.command, vec![firmware::VERIFY]),
            true,
        )
        .await?;
        let verified = match self.read_number(state, step, &update.state, timing).await {
            Ok(number) => number,
            Err(_) => {
                state.link = None;
                return Err(no_confirmation(
                    "the device could not be read after the image was sent to it",
                ));
            }
        };
        if verified != firmware::STATE_VERIFIED {
            let why = self.update_error(state, step, &update, timing).await;
            return Err(self.leave_update_mode(state, &update, timing, why).await);
        }
        // The one frame that makes the image the running one. Its reply is
        // read by the step that confirms.
        let request = Request::WriteWords {
            offset: update.command,
            words: vec![firmware::COMMIT],
        };
        state.may_send(&request)?;
        let bytes = state.frame(&request)?;
        state.send(request, bytes, timing.0, timing.1).await?;
        Ok(StepOutcome::Done(Observed::default()))
    }

    /// After the commit: waits for the device to come back, within the
    /// window its map declares, on a link of its own, and compares its
    /// model and serial with the map and the surface. The firmware version
    /// is not compared with the map's list: a device that runs the version
    /// the image carries is what the update is for, and the next run stops
    /// until the map names it.
    async fn after_update(
        &self,
        state: &mut LinkState,
        step: &Step,
        input: &StepInput<'_>,
        output: &str,
    ) -> Result<StepOutcome, StepFailure> {
        let (map, update) = Self::firmware_scene(state, step, false)?;
        let (index, name, _) = state.map(step)?;
        let key = (index, name.to_owned());
        state.identified.remove(&key);
        // The device closed every link when it restarted.
        state.link = None;
        state.pending = None;
        state.unread = None;
        state.staged = None;
        state.staged_image = None;
        let gap = self.gap(&map);
        let within = self.reply_within(&map);
        let window = Duration::from_millis(u64::from(update.restart_within_ms))
            .min(input.deadline.remaining());
        let until = tokio::time::Instant::now() + window;
        let (found, capture) = loop {
            let left = until.saturating_duration_since(tokio::time::Instant::now());
            if left.is_zero() {
                return Err(StepFailure::unknown(fail(
                    &codes::row::DEVICE_NOT_RETURNED,
                    "The device did not come back and identify itself within the restart window its map declares, after its update was committed.",
                    "Alert a person and show the recovery route the operation declares; do not send the update again.",
                )));
            }
            let endpoint = &state.endpoint;
            if let Ok(link) = link::open(
                &endpoint.protocol,
                &endpoint.address,
                endpoint.unit,
                within.min(left),
            )
            .await
            {
                state.link = Some(link);
                state.pending = None;
                state.unread = None;
                state.last_frame = None;
                match self.identify_within(state, within.min(left)).await {
                    Ok(identified) => break identified,
                    Err(_) => state.link = None,
                }
            }
            tokio::time::sleep(gap.min(left)).await;
        };
        let mismatch = |what: &str| {
            StepFailure::answered(fail(
                &codes::row::DEVICE_IDENTITY_MISMATCH,
                format!("After its update the device {what}, so nothing more is read from it."),
                "Check the device against the map and the surface; do not send the update again.",
            ))
        };
        if found.get("model") != Some(&map.model) {
            return Err(mismatch("reports another model than the map"));
        }
        if let Some(declared) = &state.serial
            && found.get("serial") != Some(declared)
        {
            return Err(mismatch("does not report the serial its surface declares"));
        }
        let mut observed = Observed {
            captures: vec![capture],
            ..Observed::default()
        };
        if let Some(version) = found.get("firmware_version") {
            observed
                .values
                .insert(output.to_owned(), Value::Text(self.redact.redact(version)));
        }
        state.identified.insert(key);
        state.identity = found;
        Ok(StepOutcome::Done(observed))
    }

    /// Identification on a link just opened, with the time it has left.
    async fn identify_within(
        &self,
        state: &mut LinkState,
        within: Duration,
    ) -> Result<(BTreeMap<String, String>, Capture), StepFailure> {
        state
            .ask(Request::Identify, self.defaults.min_gap, within)
            .await?;
        state.receive(within).await?;
        let (unread, reply) = state.answer()?;
        let capture = self.capture(&unread.sent, &unread.reply);
        match reply {
            Some(Reply::Identity(found)) => Ok((found, capture)),
            Some(Reply::Refused(refusal)) => Err(refused_read(refusal)),
            _ => Err(StepFailure::answered(fail(
                &codes::row::UNEXPECTED_REPLY,
                "The device answered the request to identify itself with a reply of no declared form.",
                "Check that the surface names the protocol the device speaks.",
            ))),
        }
    }

    fn reply_within(&self, map: &CompiledMap) -> Duration {
        map.pace
            .reply_within_ms
            .map_or(self.defaults.reply_within, |ms| {
                Duration::from_millis(u64::from(ms))
            })
    }

    /// The gap to keep between frames: the map's, and never less than its
    /// greatest rate allows.
    fn gap(&self, map: &CompiledMap) -> Duration {
        let gap = map.pace.min_gap_ms.map_or(self.defaults.min_gap, |ms| {
            Duration::from_millis(u64::from(ms))
        });
        let by_rate = map.pace.max_per_second.map_or(Duration::ZERO, |rate| {
            Duration::from_millis(1000_u64.div_ceil(u64::from(rate.max(1))))
        });
        gap.max(by_rate)
    }

    /// The request that writes one argument to a register, after every
    /// check a write passes before a frame is built: the register is one
    /// the device was compared with, it may be written, its hazard is one
    /// whose writes are built, the unit is the one the surface declares,
    /// and the value is inside its range, twice.
    fn write_request(
        state: &LinkState,
        step: &Step,
        input: &StepInput<'_>,
        register: &str,
        arg: &str,
    ) -> Result<(Request, CompiledMap), StepFailure> {
        let (found, map) = state.register(step, register)?;
        if !found.access.writable() {
            return Err(compiled_target_unreadable(
                "a step writes a register its map declares read-only",
            ));
        }
        match found.hazard {
            None => {
                return Err(compiled_target_unreadable(
                    "a step writes a register that declares no hazard",
                ));
            }
            // The compiler refuses such a binding. A compiled target is data,
            // and a write that moves something is refused here too, until
            // `confirm_by`, the safe state, and link-loss handling are built.
            Some(Hazard::Physical) => {
                return Err(StepFailure::not_sent(fail(
                    &codes::row::PHYSICAL_NOT_BUILT,
                    format!(
                        "The register `{register}` has the hazard `physical`, and a write that moves, heats, or opens something is not built. No frame was sent."
                    ),
                    "Leave this operation out until `confirm_by` and the safe-state operation are built.",
                )));
            }
            Some(_) => {}
        }
        if state.serial.is_none() {
            return Err(StepFailure::not_sent(fail(
                &codes::row::DEVICE_SERIAL_NOT_DECLARED,
                "The surface declares no serial for the device of this environment, so nothing is written to it. No frame was sent.",
                "Declare the unit's serial for this environment under `serials:` on the surface, after proving the map against that unit.",
            )));
        }
        let value = input.args.get(arg).filter(|value| !value.is_null());
        let request = value
            .ok_or(Unwritable::WrongKind)
            .and_then(|value| codec::write_request(&found, value))
            .map_err(|unwritable| {
                StepFailure::not_sent(match unwritable {
                    Unwritable::WrongKind => fail(
                        &codes::row::SCHEMA_INVALID,
                        format!("The value for the register `{register}` is absent or is not of the kind the register takes. No frame was sent."),
                        "Give a value of the register's field type.",
                    ),
                    Unwritable::OutOfRange => fail(
                        &codes::row::VALUE_OUT_OF_RANGE,
                        format!("The value for the register `{register}` is outside the safe range its map declares. No frame was sent."),
                        "Give a value inside the register's range.",
                    ),
                    Unwritable::NotRepresentable => fail(
                        &codes::row::VALUE_OUT_OF_RANGE,
                        format!("The value for the register `{register}` is not a whole number of the register's steps, or is more than it can hold. No frame was sent."),
                        "Give a value the register can hold exactly; nothing is rounded on the way to a device.",
                    ),
                })
            })?;
        Ok((request, map))
    }

    /// Builds the frame of a write and gives its bytes out, so that they
    /// are recorded before the step that sends them. Nothing is sent.
    fn build_frame(
        &self,
        state: &mut LinkState,
        step: &Step,
        input: &StepInput<'_>,
        register: &str,
        arg: &str,
        reads_after: u32,
    ) -> Result<StepOutcome, StepFailure> {
        let (request, _) = Self::write_request(state, step, input, register, arg)?;
        state.may_send(&request)?;
        // A write that could not be read back afterwards is not started.
        if state.frames_left <= reads_after {
            return Err(budget_exceeded(
                "The frames this run may still send on the device link would not cover the write and its read-back, so the write was not started.",
            ));
        }
        let bytes = state.frame(&request)?;
        let frame = self.redact.redact(&hex(&bytes));
        let evidence = format!(
            "{{\"digest\":\"{}\",\"frame\":\"{frame}\"}}",
            Digest::of(&bytes)
        );
        let mut observed = Observed {
            captures: vec![Capture {
                name: "frame".to_owned(),
                media_type: "application/json".to_owned(),
                bytes: self.redact.redact(&evidence).into_bytes(),
            }],
            ..Observed::default()
        };
        observed
            .for_reconcile
            .insert(frame_name(register), Value::Text(frame));
        state.staged = Some(Staged {
            register: register.to_owned(),
            request,
            bytes,
        });
        Ok(StepOutcome::Done(observed))
    }

    async fn write_frame(
        &self,
        state: &mut LinkState,
        step: &Step,
        input: &StepInput<'_>,
        payload: &StepPayload,
    ) -> Result<StepOutcome, StepFailure> {
        match payload {
            StepPayload::AskRead { register } => {
                let (found, map) = state.register(step, register)?;
                if !found.access.readable() || found.read_effect != ReadEffect::None {
                    return Err(compiled_target_unreadable(
                        "a step reads a register that has nothing to read, or that reading changes",
                    ));
                }
                let within = self.reply_within(&map).min(input.deadline.remaining());
                state
                    .ask(codec::read_request(&found), self.gap(&map), within)
                    .await?;
            }
            StepPayload::Write { register, arg } => {
                if !step.commits || !may_write(state.posture) {
                    return Err(read_only());
                }
                // Every check again, on the value this step was given.
                let (request, map) = Self::write_request(state, step, input, register, arg)?;
                // The frame sent is the frame that was recorded, or none.
                let staged = state
                    .staged
                    .take()
                    .filter(|staged| staged.register == *register && staged.request == request)
                    .ok_or_else(|| {
                        compiled_target_unreadable(
                            "a write frame is sent that was not built and recorded first, or whose value changed since",
                        )
                    })?;
                state.may_send(&request)?;
                let within = self.reply_within(&map).min(input.deadline.remaining());
                state
                    .send(request, staged.bytes, self.gap(&map), within)
                    .await?;
            }
            StepPayload::Install { .. } => {
                return self.install(state, step, input, payload).await;
            }
            _ => {
                return Err(compiled_target_unreadable(
                    "a `write_frame` step holds no frame to send",
                ));
            }
        }
        Ok(StepOutcome::Done(Observed::default()))
    }

    /// What the register holds, against what the frame recorded for it
    /// before the write would have written. `None` when no frame was
    /// recorded for the register, which is every plain read.
    fn frame_check(
        state: &LinkState,
        found: &CompiledRegister,
        recorded: &Values,
        register: &str,
        held: &Value,
    ) -> Option<FrameCheck> {
        let frame = recorded.get(&frame_name(register))?;
        let would_write = frame
            .as_text()
            .and_then(unhex)
            .and_then(|bytes| state.link.as_ref()?.written(&bytes))
            .and_then(|request| match request {
                Request::WriteWords { offset, words } if offset == found.offset => {
                    codec::decode(found, &Reply::Words(words))
                }
                Request::WriteBit { offset, on } if offset == found.offset => {
                    codec::decode(found, &Reply::Bits(vec![on]))
                }
                _ => None,
            });
        Some(match would_write {
            None => FrameCheck::Unreadable,
            Some(value) if value == *held => FrameCheck::Matches,
            Some(_) => FrameCheck::Differs,
        })
    }

    fn expect(
        &self,
        state: &mut LinkState,
        step: &Step,
        input: &StepInput<'_>,
        payload: &StepPayload,
        recorded: &Values,
        measured: &mut Vec<Detail>,
    ) -> Result<StepOutcome, StepFailure> {
        let (unread, reply) = state.answer()?;
        let capture = self.capture(&unread.sent, &unread.reply);
        let unexpected = || {
            StepFailure::answered(fail(
                &codes::row::UNEXPECTED_REPLY,
                "The device answered a read with a reply of no form its map declares.",
                "Check the register's layout in the map against the device's documentation.",
            ))
        };
        match payload {
            StepPayload::Value { register, .. } | StepPayload::Before { register } => {
                if unread.request.is_write() {
                    return Err(compiled_target_unreadable(
                        "a value is expected from a write's reply",
                    ));
                }
                let (found, _) = state.register(step, register)?;
                let value = match reply {
                    Some(Reply::Refused(refusal)) => return Err(refused_read(refusal)),
                    Some(reply) => codec::decode(&found, &reply).ok_or_else(unexpected)?,
                    None => return Err(unexpected()),
                };
                // A read after a write that recorded its frame: what the device
                // holds is set beside what the frame carried. The verdict on
                // the value is the engine's; this is evidence of it.
                let checked = matches!(payload, StepPayload::Value { .. })
                    .then(|| Self::frame_check(state, &found, recorded, register, &value))
                    .flatten();
                let value = self.redact.redact_value(&value);
                let mut observed = Observed {
                    captures: vec![capture],
                    ..Observed::default()
                };
                if let Some(check) = checked {
                    measured.push(Detail::new("frame_match", check.as_str()));
                    let text = format!(
                        "{{\"register\":\"{register}\",\"device_vs_frame\":\"{}\"}}",
                        check.as_str()
                    );
                    observed.captures.push(Capture {
                        name: "frame_check".to_owned(),
                        media_type: "application/json".to_owned(),
                        bytes: self.redact.redact(&text).into_bytes(),
                    });
                }
                match payload {
                    StepPayload::Value { output, .. } => {
                        observed.values.insert(output.clone(), value);
                    }
                    _ => {
                        observed.for_reconcile.insert(before_name(register), value);
                    }
                }
                Ok(StepOutcome::Done(observed))
            }
            StepPayload::Precondition { register, equals } => {
                let (found, _) = state.register(step, register)?;
                let value = match reply {
                    Some(Reply::Refused(refusal)) => return Err(refused_read(refusal)),
                    Some(reply) => codec::decode(&found, &reply).ok_or_else(unexpected)?,
                    None => return Err(unexpected()),
                };
                let expected = Decimal::parse(equals).map_err(|_| {
                    compiled_target_unreadable("a precondition holds a number that is not one")
                })?;
                let held = match &value {
                    Value::Int(number) => Some(Decimal::from_i64(*number)),
                    Value::Decimal(number) => Some(*number),
                    _ => None,
                };
                if held.as_ref() != Some(&expected) {
                    return Err(StepFailure::not_sent(fail(
                        &codes::row::UPDATE_PRECONDITION_UNMET,
                        format!(
                            "The register `{register}` does not hold `{equals}`, which the update needs before anything is written. Nothing was written."
                        ),
                        "Bring the device to the state its documentation sets for an update, then run the operation again.",
                    )));
                }
                Ok(StepOutcome::Done(Observed {
                    captures: vec![capture],
                    ..Observed::default()
                }))
            }
            StepPayload::Checksum {
                register,
                image,
                output,
            } => {
                let (found, _) = state.register(step, register)?;
                let value = match reply {
                    Some(Reply::Refused(refusal)) => return Err(refused_read(refusal)),
                    Some(reply) => codec::decode(&found, &reply).ok_or_else(unexpected)?,
                    None => return Err(unexpected()),
                };
                let reported = value.as_int().ok_or_else(unexpected)?;
                let carried = input
                    .args
                    .get(image)
                    .and_then(Value::as_text)
                    .and_then(unhex)
                    .and_then(|bytes| firmware::read(&bytes).ok())
                    .map(|image| image.carried)
                    .ok_or_else(|| {
                        compiled_target_unreadable("a checksum is compared with no image")
                    })?;
                if reported != i64::from(carried) {
                    return Err(StepFailure::answered(fail(
                        &codes::row::IMAGE_CHECKSUM_DIFFERS,
                        "After the update the device reports a checksum of its image that is not the one the image carries.",
                        "Do not trust the device's firmware. Follow the recovery route the operation declares, then read the device's version and checksum before anything else is written.",
                    )));
                }
                let mut observed = Observed {
                    captures: vec![capture],
                    ..Observed::default()
                };
                if let Some(output) = output {
                    observed
                        .values
                        .insert(output.clone(), Value::Text(format!("{reported:08x}")));
                }
                Ok(StepOutcome::Done(observed))
            }
            StepPayload::Acknowledged => {
                if !unread.request.is_write() {
                    return Err(compiled_target_unreadable(
                        "an acknowledgement is expected from a read's reply",
                    ));
                }
                match reply {
                    Some(Reply::Written) => Ok(StepOutcome::Done(Observed {
                        captures: vec![capture],
                        ..Observed::default()
                    })),
                    Some(Reply::Refused(refusal)) => Err(refused_write(refusal)),
                    // An echo of something else, or no declared form: the
                    // frame was sent, and nothing clear came back.
                    _ => {
                        state.link = None;
                        Err(no_confirmation(
                            "the reply that came does not acknowledge it",
                        ))
                    }
                }
            }
            _ => Err(compiled_target_unreadable(
                "an `expect` step expects nothing",
            )),
        }
    }
}

impl Driver for DeviceDriver {
    fn prepare<'a>(
        &'a self,
        cx: &'a PrepareContext<'a>,
        _lease: Option<&'a SessionLease>,
    ) -> BoxFuture<'a, Result<Prepared, StepFailure>> {
        Box::pin(async move {
            let surface = &cx.surface.surface;
            let not_declared = |what: &str| {
                StepFailure::not_sent(fail(
                    &codes::row::SURFACE_NOT_DECLARED,
                    format!("The link was not opened: {what}."),
                    "Declare the device's link as a surface of the kind `device_link`, with its address in the run's environment.",
                ))
            };
            if surface.kind != self.names.surface_kind {
                return Err(not_declared("the surface is not of the kind `device_link`"));
            }
            if !cx.destinations.contains(&surface.address) {
                return Err(not_declared("its address is not one this run may reach"));
            }
            let fields = surface.payload.decode::<LinkFields>().map_err(|_| {
                compiled_target_unreadable("the surface does not state its protocol and unit")
            })?;
            let maps = self.maps(cx.compiled_target)?;
            // The unit this run means: the serial the surface declares for
            // the run's environment.
            let serial = fields.serials.get(cx.surface.environment.as_str()).cloned();
            let within = self.defaults.reply_within.min(cx.deadline.remaining());
            let link = link::open(&fields.protocol, &surface.address, fields.unit, within)
                .await
                .map_err(|error| {
                    StepFailure::not_sent(match error {
                        OpenError::Address => fail(
                            &codes::row::SCHEMA_INVALID,
                            "The address of the device link is not of the form its protocol takes.",
                            "Write the address as a network address and a port, such as `192.0.2.7:502`.",
                        ),
                        OpenError::TimedOut => fail(
                            &codes::row::TIMEOUT,
                            "The device did not accept a link in time.",
                            "Check that the device is on and reachable, then try again.",
                        ),
                        OpenError::Refused => fail(
                            &codes::row::CONNECTION_LOST,
                            "The device refused the link.",
                            "Check that the device is on and reachable, then try again.",
                        ),
                    })
                })?;
            Ok(Prepared::new(LinkState {
                link: Some(link),
                maps,
                identified: BTreeSet::new(),
                serial,
                staged: None,
                staged_image: None,
                endpoint: Endpoint {
                    protocol: fields.protocol.clone(),
                    address: surface.address.clone(),
                    unit: fields.unit,
                },
                identity: BTreeMap::new(),
                frames_left: self.defaults.max_frames,
                pending: None,
                unread: None,
                last_frame: None,
                posture: cx.posture,
            }))
        })
    }

    fn act<'a>(
        &'a self,
        prepared: &'a mut Prepared,
        step: &'a Step,
        input: &'a StepInput<'a>,
    ) -> BoxFuture<'a, Result<StepOutcome, StepFailure>> {
        Box::pin(async move {
            // What this record's earlier steps gave for reconcile: the frame
            // of a write, and the value before it.
            let recorded = prepared.recorded().clone();
            let state = prepared.state::<LinkState>().ok_or_else(|| {
                compiled_target_unreadable("the link was not prepared by this driver")
            })?;
            let payload = step
                .payload
                .decode::<StepPayload>()
                .map_err(|_| compiled_target_unreadable("a step's details do not decode"))?;
            let names = &self.names;
            let mut measured = Vec::new();
            let outcome = if step.kind == names.open {
                match &payload {
                    StepPayload::Identify => self.open(state, step, input, false).await,
                    StepPayload::Report => self.open(state, step, input, true).await,
                    StepPayload::AfterUpdate { output } => {
                        self.after_update(state, step, input, output).await
                    }
                    _ => Err(compiled_target_unreadable("an `open` step opens nothing")),
                }
            } else if step.kind == names.build_frame {
                match &payload {
                    StepPayload::Build {
                        register,
                        arg,
                        reads_after,
                    } => self.build_frame(state, step, input, register, arg, *reads_after),
                    StepPayload::BuildFirmware { .. } => {
                        self.build_firmware(state, step, input, &payload)
                    }
                    _ => Err(compiled_target_unreadable(
                        "a `build_frame` step builds no frame",
                    )),
                }
            } else if step.kind == names.write_frame {
                self.write_frame(state, step, input, &payload).await
            } else if step.kind == names.read_frame {
                if payload != StepPayload::Reply {
                    return Err(compiled_target_unreadable(
                        "a `read_frame` step reads no reply",
                    ));
                }
                let within = match state.map(step) {
                    Ok((_, _, map)) => self.reply_within(map),
                    Err(_) => self.defaults.reply_within,
                };
                state
                    .receive(within.min(input.deadline.remaining()))
                    .await?;
                Ok(StepOutcome::Done(Observed::default()))
            } else if step.kind == names.expect {
                self.expect(state, step, input, &payload, &recorded, &mut measured)
            } else {
                Err(compiled_target_unreadable(
                    "a step is of a kind this driver does not carry out",
                ))
            };
            // How the step was done goes to the trace with what it returned.
            let mut outcome = outcome?;
            let framed = matches!(
                &payload,
                StepPayload::Value { register, .. } if recorded.get(&frame_name(register)).is_some()
            );
            measured.push(Detail::new(
                "frames_sent",
                self.defaults
                    .max_frames
                    .saturating_sub(state.frames_left)
                    .to_string(),
            ));
            let (StepOutcome::Done(observed) | StepOutcome::Absent(observed)) = &mut outcome;
            observed
                .captures
                .push(acting(step, &payload, framed, measured));
            Ok(outcome)
        })
    }

    fn health(&self, _surface: &SurfaceRef) -> DriverHealth {
        DriverHealth::Ready
    }

    fn release<'a>(&'a self, prepared: Prepared, _poisoned: bool) -> BoxFuture<'a, Cleanup> {
        Box::pin(async move {
            // Dropping the state closes the link. A link is never kept for
            // another run: each run opens its own and identifies again.
            drop(prepared);
            Cleanup::complete()
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use a2m_core::{
        FailsAs, NoSecrets, PackPayload, PackRef, SectionIndex, Step, StepKind, Values,
    };
    use a2m_provider::{BoxFuture, Deadline, Driver, Posture, Prepared, StepInput};
    use tokio::time::Instant;

    use super::{DEFAULT_MAX_FRAMES, DEFAULT_MIN_GAP_MS, DeviceDriver, Limits, LinkState};
    use crate::codes;
    use crate::link::{Link, LinkError, Reply, Request, SendError};
    use crate::lower::StepPayload;
    use crate::map::{
        Access, CompiledMap, CompiledRegister, Data, MapSection, Pace, ReadEffect, Table,
        ValueKind, WordOrder,
    };
    use crate::names::Names;

    /// A link in memory that answers at once, and notes the moment each
    /// frame left.
    struct Timed(Arc<Mutex<Vec<Instant>>>);

    impl Link for Timed {
        fn frame(&mut self, request: &Request) -> Option<Vec<u8>> {
            Some(vec![u8::from(request.is_write())])
        }

        fn send<'a>(
            &'a mut self,
            _frame: &'a [u8],
            _within: Duration,
        ) -> BoxFuture<'a, Result<(), SendError>> {
            self.0.lock().unwrap().push(Instant::now());
            Box::pin(async { Ok(()) })
        }

        fn receive(&mut self, _within: Duration) -> BoxFuture<'_, Result<Vec<u8>, LinkError>> {
            Box::pin(async { Ok(Vec::new()) })
        }

        fn interpret(&self, request: &Request, _reply: &[u8]) -> Option<Reply> {
            Some(match request {
                Request::Identify => Reply::Identity(BTreeMap::from([
                    ("model".to_owned(), "example-controller".to_owned()),
                    ("firmware_version".to_owned(), "1.0.0".to_owned()),
                ])),
                _ => Reply::Words(vec![7]),
            })
        }
    }

    fn map(pace: Pace) -> CompiledMap {
        CompiledMap {
            surface: "controller".to_owned(),
            model: "example-controller".to_owned(),
            firmware: vec!["1.0.0".to_owned()],
            identify: vec!["model".to_owned(), "firmware_version".to_owned()],
            pace,
            registers: BTreeMap::from([(
                "level".to_owned(),
                CompiledRegister {
                    table: Table::Input,
                    offset: 1,
                    data: Data::U16,
                    words: 1,
                    word_order: WordOrder::HighFirst,
                    scale: None,
                    kind: ValueKind::Int,
                    access: Access::Read,
                    range: None,
                    hazard: None,
                    read_effect: ReadEffect::None,
                    confirm_by: None,
                },
            )]),
            safe_state: None,
            on_link_loss: None,
            firmware_update: None,
        }
    }

    fn step(kind: &StepKind, payload: &StepPayload) -> Step {
        Step {
            kind: kind.clone(),
            refers_to: Some(PackRef {
                section: SectionIndex::new(0),
                entry: "main".to_owned(),
            }),
            args: Vec::new(),
            timeout_ms: 1000,
            fails_as: FailsAs::from(&codes::row::TIMEOUT),
            commits: false,
            lookup: false,
            idempotent: false,
            when: None,
            payload: PackPayload::encode(payload).unwrap(),
        }
    }

    /// Identifies the device and reads one register `reads` times, with
    /// the gap between frames that `min_gap_ms` and the map's `pace` give.
    /// Returns the moment each frame left.
    async fn moments(min_gap_ms: u64, pace: Pace, reads: usize) -> Vec<Instant> {
        let names = Arc::new(Names::new().unwrap());
        let driver = DeviceDriver::new(
            Arc::clone(&names),
            Arc::new(NoSecrets),
            Limits {
                reply_within: Duration::from_secs(1),
                min_gap: Duration::from_millis(min_gap_ms),
                max_frames: u32::try_from(DEFAULT_MAX_FRAMES).unwrap(),
            },
        );
        let sent = Arc::new(Mutex::new(Vec::new()));
        let mut prepared = Prepared::new(LinkState {
            link: Some(Box::new(Timed(Arc::clone(&sent)))),
            maps: BTreeMap::from([(0, MapSection::from([("main".to_owned(), map(pace))]))]),
            identified: BTreeSet::new(),
            serial: None,
            staged: None,
            staged_image: None,
            endpoint: super::Endpoint {
                protocol: "modbus_tcp".to_owned(),
                address: "127.0.0.1:1".to_owned(),
                unit: 1,
            },
            identity: BTreeMap::new(),
            frames_left: u32::try_from(DEFAULT_MAX_FRAMES).unwrap(),
            pending: None,
            unread: None,
            last_frame: None,
            posture: Posture::Live,
        });
        let register = "level".to_owned();
        let mut steps = vec![step(&names.open, &StepPayload::Identify)];
        for _ in 0..reads {
            steps.push(step(
                &names.write_frame,
                &StepPayload::AskRead {
                    register: register.clone(),
                },
            ));
            steps.push(step(&names.read_frame, &StepPayload::Reply));
            steps.push(step(
                &names.expect,
                &StepPayload::Value {
                    register: register.clone(),
                    output: "level".to_owned(),
                },
            ));
        }
        let args = Values::new();
        for step in &steps {
            let input = StepInput::new(&args, Deadline::after(Duration::from_secs(1)));
            driver.act(&mut prepared, step, &input).await.unwrap();
        }
        sent.lock().unwrap().clone()
    }

    fn gaps(moments: &[Instant]) -> Vec<Duration> {
        moments.windows(2).map(|pair| pair[1] - pair[0]).collect()
    }

    // The clock is the test's: it moves only when the driver waits on it,
    // so each gap is exactly what the driver asked for.
    #[tokio::test(start_paused = true)]
    async fn a_map_with_no_pace_sends_four_frames_a_second_at_most() {
        let quarter = Duration::from_millis(250);
        assert_eq!(Duration::from_millis(DEFAULT_MIN_GAP_MS), quarter);

        let left = moments(DEFAULT_MIN_GAP_MS, Pace::default(), 9).await;
        assert_eq!(left.len(), 10);
        assert_eq!(gaps(&left), vec![quarter; 9]);
        // No second holds a fifth frame.
        assert!(
            left.windows(5)
                .all(|five| five[4] - five[0] >= Duration::from_secs(1))
        );

        // A map's own pace: the larger of its gap and what its rate
        // allows, whatever the setting says.
        let slow = Pace {
            min_gap_ms: Some(20),
            reply_within_ms: None,
            max_per_second: Some(2),
        };
        assert_eq!(
            gaps(&moments(DEFAULT_MIN_GAP_MS, slow, 3).await),
            vec![Duration::from_millis(500); 3]
        );
        let quick = Pace {
            min_gap_ms: Some(20),
            reply_within_ms: None,
            max_per_second: None,
        };
        assert_eq!(
            gaps(&moments(DEFAULT_MIN_GAP_MS, quick, 3).await),
            vec![Duration::from_millis(20); 3]
        );

        // A person who knows the device can take more lowers the setting.
        assert_eq!(
            gaps(&moments(100, Pace::default(), 3).await),
            vec![Duration::from_millis(100); 3]
        );
    }
}
