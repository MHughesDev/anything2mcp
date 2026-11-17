//! The one interface between the driver and a device's protocol.
//!
//! A link carries one request at a time: one frame out, one reply in. The
//! driver knows registers, ranges, and failure classes; a protocol knows
//! frames. Another protocol, or the same protocol over a serial port, is
//! another implementation of [`Link`] and another arm of [`open`]. Serial
//! links are not built.

use std::collections::BTreeMap;
use std::time::Duration;

use a2m_provider::BoxFuture;
use serde::{Deserialize, Serialize};

use crate::map::Table;
use crate::modbus;

/// The protocols a `device_link` surface may name.
pub(crate) const PROTOCOLS: &[&str] = &["modbus_tcp"];

/// One request to a device, in terms every register protocol has.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Request {
    /// Ask the device what it is.
    Identify,
    /// Read bits.
    ReadBits { table: Table, offset: u16, count: u16 },
    /// Read words.
    ReadWords { table: Table, offset: u16, count: u16 },
    /// Write one bit.
    WriteBit { offset: u16, on: bool },
    /// Write one or more words, in one frame.
    WriteWords { offset: u16, words: Vec<u16> },
}

impl Request {
    /// Whether the request changes the device.
    pub const fn is_write(&self) -> bool {
        matches!(self, Self::WriteBit { .. } | Self::WriteWords { .. })
    }
}

/// Why a device refused a request, as its protocol states it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Refusal {
    /// It does not carry out this kind of request.
    Function,
    /// It will not take the address.
    Address,
    /// It will not take the value.
    Value,
    /// It is busy, and carried nothing out.
    Busy,
    /// A gateway got no answer from the device behind it.
    NoAnswerBehindGateway,
    /// It failed while carrying the request out, or refused in a way the
    /// protocol does not explain.
    Other(u8),
}

/// What a device answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Reply {
    /// What it reports about itself, by the words of a map's `identify`.
    Identity(BTreeMap<String, String>),
    /// The bits read.
    Bits(Vec<bool>),
    /// The words read.
    Words(Vec<u16>),
    /// An acknowledgement that echoes the write.
    Written,
    /// A refusal.
    Refused(Refusal),
}

/// Why a frame was not sent, or no reply was read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LinkError {
    /// The link is closed.
    Closed,
    /// Nothing came in time.
    TimedOut,
    /// The bytes that came are not a reply to the request.
    Corrupt,
}

/// How sending a frame failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SendError {
    /// No byte left: the link was already closed, or the request cannot be
    /// framed.
    NotSent,
    /// Some or all of the frame may have left.
    Unknown,
}

/// One open link to one device.
pub(crate) trait Link: Send {
    /// Sends one request as one frame. Returns the frame's bytes.
    fn send<'a>(
        &'a mut self,
        request: &'a Request,
        within: Duration,
    ) -> BoxFuture<'a, Result<Vec<u8>, SendError>>;

    /// Reads the reply to the request last sent: its bytes, whole.
    fn receive(&mut self, within: Duration) -> BoxFuture<'_, Result<Vec<u8>, LinkError>>;

    /// Reads a reply's bytes as the answer to `request`. `None` when the
    /// bytes are not a reply of a form the protocol declares for it.
    fn interpret(&self, request: &Request, reply: &[u8]) -> Option<Reply>;
}

/// Why a link could not be opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OpenError {
    /// The address is not of the form the protocol takes.
    Address,
    /// The device did not accept a link in time.
    TimedOut,
    /// The device refused the link.
    Refused,
}

/// Opens a link of the protocol named. The protocol is one of
/// [`PROTOCOLS`]: the surface's schema refuses any other.
pub(crate) async fn open(
    protocol: &str,
    address: &str,
    unit: u8,
    within: Duration,
) -> Result<Box<dyn Link>, OpenError> {
    match protocol {
        "modbus_tcp" => modbus::connect(address, unit, within)
            .await
            .map(|link| Box::new(link) as Box<dyn Link>),
        _ => Err(OpenError::Address),
    }
}
