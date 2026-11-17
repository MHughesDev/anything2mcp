//! The `device` driver: one link, one request at a time.
//!
//! `prepare` opens the link. Each step then does one thing: `open`
//! identifies the device against the step's map, `write_frame` sends one
//! frame, `read_frame` reads its reply, and `expect` reads the reply in the
//! form the map declares. A write is checked against its register's range
//! before any frame is built, and a write frame that was sent and not
//! clearly acknowledged is `possible_commit`. Nothing is retried.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::Duration;

use a2m_auth::SessionLease;
use a2m_core::{Failure, FailureClass, FailureCode, Plan, Redact, Step, Value, Values};
use a2m_provider::{
    BoxFuture, Capture, Cleanup, Deadline, Driver, DriverHealth, Observed, Posture,
    PrepareContext, Prepared, StepFailure, StepInput, StepOutcome, SurfaceRef,
};
use serde::Deserialize;

use crate::codec::{self, Unwritable};
use crate::codes;
use crate::link::{self, Link, LinkError, OpenError, Refusal, Reply, Request, SendError};
use crate::lower::{StepPayload, before_name};
use crate::map::{MapPlan, MapSection, ReadEffect, RegisterPlan, SECTION_FORMAT};
use crate::names::{Names, PACK};

/// The pace of a link whose map states none: the pack's settings.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PaceDefaults {
    pub reply_within: Duration,
    pub min_gap: Duration,
}

/// What a `device_link` surface states beside its address.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LinkFields {
    protocol: String,
    unit: u8,
}

/// The `device` driver.
pub(crate) struct DeviceDriver {
    names: Arc<Names>,
    redact: Arc<dyn Redact>,
    defaults: PaceDefaults,
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

/// What `prepare` acquired: the open link and what the plan says about it.
struct LinkState {
    /// The link; `None` once it was lost, after which nothing more is sent.
    link: Option<Box<dyn Link>>,
    /// The plan's maps, by the position of their section.
    maps: BTreeMap<usize, MapSection>,
    /// The maps the device on this link was compared with.
    identified: BTreeSet<(usize, String)>,
    pending: Option<Pending>,
    unread: Option<Unread>,
    last_frame: Option<tokio::time::Instant>,
    posture: Posture,
}

fn fail(
    class: FailureClass,
    code: FailureCode,
    message: impl Into<String>,
    next: impl Into<String>,
) -> Failure {
    codes::failure(class, code, message, next)
}

fn plan_unreadable(what: &str) -> StepFailure {
    StepFailure::not_sent(fail(
        FailureClass::Input,
        codes::PAYLOAD_UNREADABLE,
        format!("The plan cannot be carried out on a device link: {what}."),
        "Compile the target again with the device pack that is enabled now.",
    ))
}

fn link_lost_before() -> StepFailure {
    StepFailure::not_sent(fail(
        FailureClass::Transient,
        codes::CONNECTION_LOST,
        "The link to the device was lost earlier in this run, so nothing more was sent.",
        "Open the link again; the device is identified again before anything is read.",
    ))
}

fn no_confirmation(what: &str) -> StepFailure {
    StepFailure::unknown(fail(
        FailureClass::PossibleCommit,
        codes::NO_CONFIRMATION,
        format!("A write frame was sent to the device and {what}, so the write may have been applied."),
        "Reconcile: open the link, identify the device, and read the register before anything is written again.",
    ))
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    })
}

impl DeviceDriver {
    pub fn new(names: Arc<Names>, redact: Arc<dyn Redact>, defaults: PaceDefaults) -> Self {
        Self {
            names,
            redact,
            defaults,
        }
    }

    /// The plan's sections that this pack compiled, decoded.
    fn maps(&self, plan: &Plan) -> Result<BTreeMap<usize, MapSection>, StepFailure> {
        let mut maps = BTreeMap::new();
        for (index, section) in plan.packs.iter().enumerate() {
            if section.pack != PACK || section.kind != self.names.register_kind {
                continue;
            }
            if section.format != SECTION_FORMAT {
                return Err(plan_unreadable(
                    "its maps were compiled in a form this pack does not read",
                ));
            }
            let decoded = section
                .payload
                .decode::<MapSection>()
                .map_err(|_| plan_unreadable("its maps do not decode"))?;
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
    fn map<'a>(&'a self, step: &'a Step) -> Result<(usize, &'a str, &'a MapPlan), StepFailure> {
        let refers = step
            .refers_to
            .as_ref()
            .ok_or_else(|| plan_unreadable("a step names no map"))?;
        let index = refers.section.get();
        self.maps
            .get(&index)
            .and_then(|section| section.get(&refers.entry))
            .map(|map| (index, refers.entry.as_str(), map))
            .ok_or_else(|| plan_unreadable("a step names a map the plan does not hold"))
    }

    /// A register of the step's map, once the device was compared with it.
    fn register(&self, step: &Step, register: &str) -> Result<(RegisterPlan, MapPlan), StepFailure> {
        let (index, name, map) = self.map(step)?;
        if !self.identified.contains(&(index, name.to_owned())) {
            return Err(StepFailure::not_sent(fail(
                FailureClass::UnknownVariant,
                codes::DEVICE_IDENTITY_MISMATCH,
                "The device on this link was not compared with the map, so nothing was sent.",
                "Identify the device first: every binding of the device mode starts with an `open` step.",
            )));
        }
        map.registers
            .get(register)
            .map(|found| (found.clone(), map.clone()))
            .ok_or_else(|| plan_unreadable("a step names a register its map does not hold"))
    }

    /// Waits until the gap since the last frame has passed, then sends.
    async fn send(
        &mut self,
        request: Request,
        gap: Duration,
        within: Duration,
    ) -> Result<(), StepFailure> {
        if self.pending.is_some() {
            return Err(plan_unreadable(
                "a frame is sent while another still waits for its reply",
            ));
        }
        let Some(link) = self.link.as_mut() else {
            return Err(link_lost_before());
        };
        if let Some(last) = self.last_frame {
            tokio::time::sleep_until(last + gap).await;
        }
        let write = request.is_write();
        match link.send(&request, within).await {
            Ok(sent) => {
                self.last_frame = Some(tokio::time::Instant::now());
                self.pending = Some(Pending { request, sent });
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
                        FailureClass::Transient,
                        codes::CONNECTION_LOST,
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
            return Err(plan_unreadable("a reply is read and no frame waits for one"));
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
                        FailureClass::Transient,
                        codes::CONNECTION_LOST,
                        "The link to the device closed before the reply to a read came.",
                        "Open the link again and read again.",
                    )),
                    (false, LinkError::TimedOut) => StepFailure::unknown(fail(
                        FailureClass::Transient,
                        codes::TIMEOUT,
                        "The device did not reply to a read in the time its map allows.",
                        "Read again; if it keeps happening, check the link and the map's `pace`.",
                    )),
                    (false, LinkError::Corrupt) => StepFailure::unknown(fail(
                        FailureClass::Transient,
                        codes::FRAME_CORRUPT,
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
            .ok_or_else(|| plan_unreadable("a reply is expected and none was read"))?;
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
            FailureClass::Transient,
            codes::DEVICE_BUSY,
            "The device answered that it is busy and did not carry out the read.",
            "Read again later.",
        ),
        Refusal::NoAnswerBehindGateway => fail(
            FailureClass::Transient,
            codes::TIMEOUT,
            "A gateway answered that the device behind it did not respond.",
            "Check the unit address of the surface, then read again.",
        ),
        Refusal::Function | Refusal::Address | Refusal::Value | Refusal::Other(_) => fail(
            FailureClass::Rejected,
            codes::DEVICE_REFUSED,
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
            FailureClass::Transient,
            codes::DEVICE_BUSY,
            "The device answered that it is busy and did not carry out the write.",
            "Nothing was written and nothing is sent again by itself; run the operation again when the device is free.",
        )),
        Refusal::Function | Refusal::Address | Refusal::Value => StepFailure::answered(fail(
            FailureClass::Rejected,
            codes::DEVICE_REFUSED,
            "The device refused the write with an exception: an address or a value it will not take. Nothing was written.",
            "Check the register's range and access in the map against the device's documentation.",
        )),
        // A failure while carrying the write out, or an answer the protocol
        // does not explain: nothing says whether the write was applied.
        Refusal::NoAnswerBehindGateway | Refusal::Other(_) => {
            no_confirmation("the device answered with a failure that does not say whether it was applied")
        }
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
            .send(Request::Identify, self.defaults.min_gap, within)
            .await?;
        state.receive(within).await?;
        let (unread, reply) = state.answer()?;
        let capture = self.capture(&unread.sent, &unread.reply);
        match reply {
            Some(Reply::Identity(found)) => Ok((found, capture)),
            Some(Reply::Refused(refusal)) => Err(refused_read(refusal)),
            _ => Err(StepFailure::answered(fail(
                FailureClass::UnknownVariant,
                codes::UNEXPECTED_REPLY,
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
            let (found, capture) = self.identify(state, input, self.defaults.reply_within).await?;
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
                FailureClass::UnknownVariant,
                codes::DEVICE_IDENTITY_MISMATCH,
                format!("The device on this link {what} the map `{}`, so nothing is read from it or written to it.", key.1),
                "Prove the map against this device in a test environment, and add its firmware version to the map's `firmware` list.",
            ))
        };
        if let Some(missing) = map.identify.iter().find(|field| !found.contains_key(*field)) {
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
        state.identified.insert(key);
        Ok(StepOutcome::Done(Observed {
            captures: vec![capture],
            ..Observed::default()
        }))
    }

    fn reply_within(&self, map: &MapPlan) -> Duration {
        map.pace
            .reply_within_ms
            .map_or(self.defaults.reply_within, |ms| {
                Duration::from_millis(u64::from(ms))
            })
    }

    /// The gap to keep between frames: the map's, and never less than its
    /// greatest rate allows.
    fn gap(&self, map: &MapPlan) -> Duration {
        let gap = map
            .pace
            .min_gap_ms
            .map_or(self.defaults.min_gap, |ms| Duration::from_millis(u64::from(ms)));
        let by_rate = map
            .pace
            .max_per_second
            .map_or(Duration::ZERO, |rate| Duration::from_millis(1000_u64.div_ceil(u64::from(rate.max(1)))));
        gap.max(by_rate)
    }

    async fn write_frame(
        &self,
        state: &mut LinkState,
        step: &Step,
        input: &StepInput<'_>,
        payload: &StepPayload,
    ) -> Result<StepOutcome, StepFailure> {
        let (request, map) = match payload {
            StepPayload::AskRead { register } => {
                let (found, map) = state.register(step, register)?;
                if !found.access.readable() || found.read_effect != ReadEffect::None {
                    return Err(plan_unreadable(
                        "a step reads a register that has nothing to read, or that reading changes",
                    ));
                }
                (codec::read_request(&found), map)
            }
            StepPayload::Write { register, arg } => {
                if !step.commits
                    || !matches!(
                        state.posture,
                        Posture::Live | Posture::Sandbox { test_records: true }
                    )
                {
                    return Err(StepFailure::not_sent(fail(
                        FailureClass::Policy,
                        a2m_provider::codes::READ_ONLY_POSTURE,
                        "A write frame was asked for where nothing may be written.",
                        "Run the operation live, as a step that commits.",
                    )));
                }
                let (found, map) = state.register(step, register)?;
                if !found.access.writable() {
                    return Err(plan_unreadable(
                        "a step writes a register its map declares read-only",
                    ));
                }
                let value = input.args.get(arg).filter(|value| !value.is_null());
                let request = value
                    .ok_or(Unwritable::WrongKind)
                    .and_then(|value| codec::write_request(&found, value))
                    .map_err(|unwritable| {
                        StepFailure::not_sent(match unwritable {
                            Unwritable::WrongKind => fail(
                                FailureClass::Input,
                                codes::SCHEMA_INVALID,
                                format!("The value for the register `{register}` is absent or is not of the kind the register takes. No frame was sent."),
                                "Give a value of the register's field type.",
                            ),
                            Unwritable::OutOfRange => fail(
                                FailureClass::Input,
                                codes::VALUE_OUT_OF_RANGE,
                                format!("The value for the register `{register}` is outside the safe range its map declares. No frame was sent."),
                                "Give a value inside the register's range.",
                            ),
                            Unwritable::NotRepresentable => fail(
                                FailureClass::Input,
                                codes::VALUE_OUT_OF_RANGE,
                                format!("The value for the register `{register}` is not a whole number of the register's steps, or is more than it can hold. No frame was sent."),
                                "Give a value the register can hold exactly; nothing is rounded on the way to a device.",
                            ),
                        })
                    })?;
                (request, map)
            }
            _ => return Err(plan_unreadable("a `write_frame` step holds no frame to send")),
        };
        let within = self.reply_within(&map).min(input.deadline.remaining());
        state.send(request, self.gap(&map), within).await?;
        Ok(StepOutcome::Done(Observed::default()))
    }

    fn expect(
        &self,
        state: &mut LinkState,
        step: &Step,
        payload: &StepPayload,
    ) -> Result<StepOutcome, StepFailure> {
        let (unread, reply) = state.answer()?;
        let capture = self.capture(&unread.sent, &unread.reply);
        let unexpected = || {
            StepFailure::answered(fail(
                FailureClass::UnknownVariant,
                codes::UNEXPECTED_REPLY,
                "The device answered a read with a reply of no form its map declares.",
                "Check the register's layout in the map against the device's documentation.",
            ))
        };
        match payload {
            StepPayload::Value { register, .. } | StepPayload::Before { register } => {
                if unread.request.is_write() {
                    return Err(plan_unreadable("a value is expected from a write's reply"));
                }
                let (found, _) = state.register(step, register)?;
                let value = match reply {
                    Some(Reply::Refused(refusal)) => return Err(refused_read(refusal)),
                    Some(reply) => codec::decode(&found, &reply).ok_or_else(unexpected)?,
                    None => return Err(unexpected()),
                };
                let value = self.redact.redact_value(&value);
                let mut observed = Observed {
                    captures: vec![capture],
                    ..Observed::default()
                };
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
            StepPayload::Acknowledged => {
                if !unread.request.is_write() {
                    return Err(plan_unreadable(
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
            _ => Err(plan_unreadable("an `expect` step expects nothing")),
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
                    FailureClass::Policy,
                    codes::SURFACE_NOT_DECLARED,
                    format!("The link was not opened: {what}."),
                    "Declare the device's link as a surface of the kind `device_link`, with its address in the run's environment.",
                ))
            };
            if surface.kind != self.names.surface_kind {
                return Err(not_declared("the surface is not of the kind `device_link`"));
            }
            if !cx.destinations.contains(&surface.address) {
                return Err(not_declared(
                    "its address is not one this run may reach",
                ));
            }
            let fields = surface
                .payload
                .decode::<LinkFields>()
                .map_err(|_| plan_unreadable("the surface does not state its protocol and unit"))?;
            let maps = self.maps(cx.plan)?;
            let within = self.defaults.reply_within.min(cx.deadline.remaining());
            let link = link::open(&fields.protocol, &surface.address, fields.unit, within)
                .await
                .map_err(|error| {
                    StepFailure::not_sent(match error {
                        OpenError::Address => fail(
                            FailureClass::Input,
                            codes::SCHEMA_INVALID,
                            "The address of the device link is not of the form its protocol takes.",
                            "Write the address as a network address and a port, such as `192.0.2.7:502`.",
                        ),
                        OpenError::TimedOut => fail(
                            FailureClass::Transient,
                            codes::TIMEOUT,
                            "The device did not accept a link in time.",
                            "Check that the device is on and reachable, then try again.",
                        ),
                        OpenError::Refused => fail(
                            FailureClass::Transient,
                            codes::CONNECTION_LOST,
                            "The device refused the link.",
                            "Check that the device is on and reachable, then try again.",
                        ),
                    })
                })?;
            Ok(Prepared::new(LinkState {
                link: Some(link),
                maps,
                identified: BTreeSet::new(),
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
            let state = prepared
                .state::<LinkState>()
                .ok_or_else(|| plan_unreadable("the link was not prepared by this driver"))?;
            let payload = step
                .payload
                .decode::<StepPayload>()
                .map_err(|_| plan_unreadable("a step's details do not decode"))?;
            let names = &self.names;
            if step.kind == names.open {
                match payload {
                    StepPayload::Identify => self.open(state, step, input, false).await,
                    StepPayload::Report => self.open(state, step, input, true).await,
                    _ => Err(plan_unreadable("an `open` step opens nothing")),
                }
            } else if step.kind == names.write_frame {
                self.write_frame(state, step, input, &payload).await
            } else if step.kind == names.read_frame {
                if payload != StepPayload::Reply {
                    return Err(plan_unreadable("a `read_frame` step reads no reply"));
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
                self.expect(state, step, &payload)
            } else {
                Err(plan_unreadable("a step is of a kind this driver does not carry out"))
            }
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

/// The deadline of a step the pack sends itself, in discovery.
pub(crate) fn within(deadline: Deadline, most: Duration) -> Deadline {
    if deadline.remaining() < most {
        deadline
    } else {
        Deadline::after(most)
    }
}
