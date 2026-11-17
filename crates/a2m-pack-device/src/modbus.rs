//! The first protocol: a subset of the Modbus application protocol over
//! TCP, written here with no protocol library (decision 0106).
//!
//! Functions 1 to 6, 16, and 43/14, with exception replies. One request is
//! answered before the next is sent. This module shares no code with the
//! test stand it is proven against.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::time::Duration;

use a2m_provider::BoxFuture;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::link::{Link, LinkError, OpenError, Refusal, Reply, Request, SendError};
use crate::map::Table;

const READ_COILS: u8 = 0x01;
const READ_DISCRETE_INPUTS: u8 = 0x02;
const READ_HOLDING_REGISTERS: u8 = 0x03;
const READ_INPUT_REGISTERS: u8 = 0x04;
const WRITE_SINGLE_COIL: u8 = 0x05;
const WRITE_SINGLE_REGISTER: u8 = 0x06;
const WRITE_MULTIPLE_REGISTERS: u8 = 0x10;
const ENCAPSULATED_INTERFACE: u8 = 0x2B;
const READ_DEVICE_IDENTIFICATION: u8 = 0x0E;
/// Read every object, as a stream, from the first.
const IDENTIFICATION_EXTENDED: u8 = 0x03;
const EXCEPTION_FLAG: u8 = 0x80;

/// The most bits, and the most words, one read may ask for.
const MOST_BITS: u16 = 2000;
const MOST_WORDS_READ: u16 = 125;
const MOST_WORDS_WRITTEN: usize = 123;
/// The header before the function code: transaction, protocol, length, unit.
const HEADER: usize = 7;
/// The longest a frame's length field may say: the unit and a full PDU.
const LONGEST: usize = 254;

/// The identification objects, by the words of a map's `identify`.
const OBJECTS: &[(u8, &str)] = &[
    (0x00, "vendor"),
    (0x01, "model"),
    (0x02, "firmware_version"),
    (0x80, "serial"),
];

/// One open link over TCP.
pub(crate) struct ModbusTcp {
    stream: Option<TcpStream>,
    unit: u8,
    /// The transaction identifier of the last request. It counts from one
    /// on every link, so the same run sends the same bytes.
    transaction: u16,
}

/// Opens a link to `address`, a socket address such as `192.0.2.7:502`.
pub(crate) async fn connect(
    address: &str,
    unit: u8,
    within: Duration,
) -> Result<ModbusTcp, OpenError> {
    // A socket address only: a name would need a lookup, and a lookup is a
    // request the plan did not state.
    let address: SocketAddr = address.parse().map_err(|_| OpenError::Address)?;
    let stream = tokio::time::timeout(within, TcpStream::connect(address))
        .await
        .map_err(|_| OpenError::TimedOut)?
        .map_err(|_| OpenError::Refused)?;
    // One small frame at a time: nothing is gained by waiting to fill a
    // segment. A link that cannot be set so still works.
    let _ = stream.set_nodelay(true);
    Ok(ModbusTcp {
        stream: Some(stream),
        unit,
        transaction: 0,
    })
}

/// The function code and the bytes after it for a request, or `None` when
/// the protocol cannot carry it.
fn pdu(request: &Request) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    match request {
        Request::Identify => {
            out.extend([
                ENCAPSULATED_INTERFACE,
                READ_DEVICE_IDENTIFICATION,
                IDENTIFICATION_EXTENDED,
                0x00,
            ]);
        }
        Request::ReadBits {
            table,
            offset,
            count,
        } => {
            let function = match table {
                Table::Coil => READ_COILS,
                Table::Discrete => READ_DISCRETE_INPUTS,
                Table::Holding | Table::Input => return None,
            };
            if *count == 0 || *count > MOST_BITS {
                return None;
            }
            out.push(function);
            out.extend(offset.to_be_bytes());
            out.extend(count.to_be_bytes());
        }
        Request::ReadWords {
            table,
            offset,
            count,
        } => {
            let function = match table {
                Table::Holding => READ_HOLDING_REGISTERS,
                Table::Input => READ_INPUT_REGISTERS,
                Table::Coil | Table::Discrete => return None,
            };
            if *count == 0 || *count > MOST_WORDS_READ {
                return None;
            }
            out.push(function);
            out.extend(offset.to_be_bytes());
            out.extend(count.to_be_bytes());
        }
        Request::WriteBit { offset, on } => {
            out.push(WRITE_SINGLE_COIL);
            out.extend(offset.to_be_bytes());
            out.extend(if *on { [0xFF, 0x00] } else { [0x00, 0x00] });
        }
        Request::WriteWords { offset, words } => match words.as_slice() {
            [] => return None,
            [word] => {
                out.push(WRITE_SINGLE_REGISTER);
                out.extend(offset.to_be_bytes());
                out.extend(word.to_be_bytes());
            }
            words => {
                if words.len() > MOST_WORDS_WRITTEN {
                    return None;
                }
                let count = u16::try_from(words.len()).ok()?;
                out.push(WRITE_MULTIPLE_REGISTERS);
                out.extend(offset.to_be_bytes());
                out.extend(count.to_be_bytes());
                out.push(u8::try_from(words.len() * 2).ok()?);
                for word in words {
                    out.extend(word.to_be_bytes());
                }
            }
        },
    }
    Some(out)
}

/// The whole frame for a request: the header and the PDU.
fn frame(transaction: u16, unit: u8, pdu: &[u8]) -> Option<Vec<u8>> {
    let length = u16::try_from(pdu.len() + 1).ok()?;
    let mut out = Vec::with_capacity(HEADER + pdu.len());
    out.extend(transaction.to_be_bytes());
    out.extend([0x00, 0x00]);
    out.extend(length.to_be_bytes());
    out.push(unit);
    out.extend(pdu);
    Some(out)
}

const fn refusal(code: u8) -> Refusal {
    match code {
        0x01 => Refusal::Function,
        0x02 => Refusal::Address,
        0x03 => Refusal::Value,
        0x06 => Refusal::Busy,
        0x0B => Refusal::NoAnswerBehindGateway,
        other => Refusal::Other(other),
    }
}

/// The objects of an identification reply, after its function and type.
fn identification(body: &[u8]) -> Option<Reply> {
    // Read code, conformity level, more follows, next object, object count.
    let [code, _conformity, more, _next, count, objects @ ..] = body else {
        return None;
    };
    // Everything must fit in one reply; a device that needs several is not
    // built, and is not guessed at.
    if *code != IDENTIFICATION_EXTENDED || *more != 0x00 {
        return None;
    }
    let mut found = BTreeMap::new();
    let mut rest = objects;
    for _ in 0..*count {
        let [id, length, tail @ ..] = rest else {
            return None;
        };
        let length = usize::from(*length);
        let (text, tail) = tail.split_at_checked(length)?;
        let text = std::str::from_utf8(text).ok()?;
        if text.chars().any(char::is_control) {
            return None;
        }
        if let Some((_, name)) = OBJECTS.iter().find(|(known, _)| known == id) {
            found.insert((*name).to_owned(), text.to_owned());
        }
        rest = tail;
    }
    rest.is_empty().then_some(Reply::Identity(found))
}

/// Reads a reply's PDU as the answer to `request`.
fn interpret(request: &Request, sent: &[u8], reply: &[u8]) -> Option<Reply> {
    let (function, body) = reply.split_first()?;
    let asked = *sent.first()?;
    if *function == asked | EXCEPTION_FLAG {
        return match body {
            [code] => Some(Reply::Refused(refusal(*code))),
            _ => None,
        };
    }
    if *function != asked {
        return None;
    }
    match request {
        Request::Identify => match body {
            [READ_DEVICE_IDENTIFICATION, rest @ ..] => identification(rest),
            _ => None,
        },
        Request::ReadBits { count, .. } => {
            let (bytes, data) = body.split_first()?;
            let count = usize::from(*count);
            if usize::from(*bytes) != count.div_ceil(8) || data.len() != usize::from(*bytes) {
                return None;
            }
            Some(Reply::Bits(
                (0..count)
                    .map(|bit| data[bit / 8] >> (bit % 8) & 1 == 1)
                    .collect(),
            ))
        }
        Request::ReadWords { count, .. } => {
            let (bytes, data) = body.split_first()?;
            if usize::from(*bytes) != usize::from(*count) * 2 || data.len() != usize::from(*bytes) {
                return None;
            }
            Some(Reply::Words(
                data.as_chunks::<2>()
                    .0
                    .iter()
                    .map(|pair| u16::from_be_bytes(*pair))
                    .collect(),
            ))
        }
        // A single write is acknowledged by its own bytes; a write of
        // several words by its address and its count.
        Request::WriteBit { .. } => (reply == sent).then_some(Reply::Written),
        Request::WriteWords { words, .. } if words.len() == 1 => {
            (reply == sent).then_some(Reply::Written)
        }
        Request::WriteWords { .. } => (sent.get(..5) == Some(reply)).then_some(Reply::Written),
    }
}

impl ModbusTcp {
    async fn read_reply(&mut self, within: Duration) -> Result<Vec<u8>, LinkError> {
        let Some(stream) = self.stream.as_mut() else {
            return Err(LinkError::Closed);
        };
        let transaction = self.transaction;
        let unit = self.unit;
        let read = async {
            let mut header = [0_u8; HEADER];
            let mut filled = 0;
            while filled < HEADER {
                match stream.read(&mut header[filled..]).await {
                    // The link closed: before any byte, it is lost; after
                    // some, the reply was cut off.
                    Ok(0) | Err(_) if filled == 0 => return Err(LinkError::Closed),
                    Ok(0) | Err(_) => return Err(LinkError::Corrupt),
                    Ok(count) => filled += count,
                }
            }
            let length = usize::from(u16::from_be_bytes([header[4], header[5]]));
            if header[0..2] != transaction.to_be_bytes()
                || header[2..4] != [0x00, 0x00]
                || header[6] != unit
                || !(2..=LONGEST).contains(&length)
            {
                return Err(LinkError::Corrupt);
            }
            let mut pdu = vec![0_u8; length - 1];
            stream
                .read_exact(&mut pdu)
                .await
                .map_err(|_| LinkError::Corrupt)?;
            Ok(pdu)
        };
        let result = match tokio::time::timeout(within, read).await {
            Ok(result) => result,
            Err(_) => Err(LinkError::TimedOut),
        };
        // After any failure the link's bytes can no longer be told apart:
        // a late reply would be read as the answer to the next request.
        if result.is_err() {
            self.stream = None;
        }
        result
    }
}

impl Link for ModbusTcp {
    fn frame(&mut self, request: &Request) -> Option<Vec<u8>> {
        let transaction = self.transaction.wrapping_add(1);
        let bytes = pdu(request).and_then(|pdu| frame(transaction, self.unit, &pdu))?;
        // The reply is matched against the frame built last.
        self.transaction = transaction;
        Some(bytes)
    }

    fn send<'a>(
        &'a mut self,
        frame: &'a [u8],
        within: Duration,
    ) -> BoxFuture<'a, Result<(), SendError>> {
        Box::pin(async move {
            let Some(stream) = self.stream.as_mut() else {
                return Err(SendError::NotSent);
            };
            let sent = tokio::time::timeout(within, async {
                stream.write_all(frame).await?;
                stream.flush().await
            })
            .await;
            match sent {
                Ok(Ok(())) => Ok(()),
                // Some of the frame may have left: nothing says how much.
                Ok(Err(_)) | Err(_) => {
                    self.stream = None;
                    Err(SendError::Unknown)
                }
            }
        })
    }

    fn receive(&mut self, within: Duration) -> BoxFuture<'_, Result<Vec<u8>, LinkError>> {
        Box::pin(self.read_reply(within))
    }

    fn interpret(&self, request: &Request, reply: &[u8]) -> Option<Reply> {
        let sent = pdu(request)?;
        interpret(request, &sent, reply)
    }

    fn written(&self, frame: &[u8]) -> Option<Request> {
        written(frame)
    }
}

/// The write a whole frame carries: the header is checked, and then the
/// PDU, in the three forms of write this module builds. Anything else is
/// not read.
fn written(frame: &[u8]) -> Option<Request> {
    let (header, pdu) = frame.split_at_checked(HEADER)?;
    let length = u16::from_be_bytes([header[4], header[5]]);
    if header[2..4] != [0, 0] || usize::from(length) != pdu.len() + 1 {
        return None;
    }
    match pdu {
        [WRITE_SINGLE_COIL, high, low, state @ ..] => {
            let on = match state {
                [0xFF, 0x00] => true,
                [0x00, 0x00] => false,
                _ => return None,
            };
            Some(Request::WriteBit {
                offset: u16::from_be_bytes([*high, *low]),
                on,
            })
        }
        [WRITE_SINGLE_REGISTER, high, low, first, second] => Some(Request::WriteWords {
            offset: u16::from_be_bytes([*high, *low]),
            words: vec![u16::from_be_bytes([*first, *second])],
        }),
        [WRITE_MULTIPLE_REGISTERS, high, low, count_high, count_low, bytes, data @ ..] => {
            let count = usize::from(u16::from_be_bytes([*count_high, *count_low]));
            if count < 2 || usize::from(*bytes) != count * 2 || data.len() != count * 2 {
                return None;
            }
            Some(Request::WriteWords {
                offset: u16::from_be_bytes([*high, *low]),
                words: data
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|pair| u16::from_be_bytes(*pair))
                    .collect(),
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{Refusal, Reply, Request, Table, frame, interpret, pdu, written};

    fn answer(request: &Request, reply: &[u8]) -> Option<Reply> {
        interpret(request, &pdu(request).unwrap(), reply)
    }

    #[test]
    fn a_frame_that_was_built_for_a_write_reads_back_into_the_same_write() {
        for request in [
            Request::WriteBit {
                offset: 4,
                on: true,
            },
            Request::WriteBit {
                offset: 4,
                on: false,
            },
            Request::WriteWords {
                offset: 100,
                words: vec![0xFFDD],
            },
            Request::WriteWords {
                offset: 104,
                words: vec![0x4240, 0x000F],
            },
        ] {
            let bytes = frame(9, 7, &pdu(&request).unwrap()).unwrap();
            assert_eq!(written(&bytes), Some(request));
        }
        // A read is no write, and bytes that are cut, padded, or of another
        // protocol are not read.
        let read = frame(
            1,
            7,
            &pdu(&Request::ReadWords {
                table: Table::Holding,
                offset: 100,
                count: 1,
            })
            .unwrap(),
        )
        .unwrap();
        assert_eq!(written(&read), None);
        let mut bytes = frame(
            1,
            7,
            &pdu(&Request::WriteWords {
                offset: 100,
                words: vec![1],
            })
            .unwrap(),
        )
        .unwrap();
        assert!(written(&bytes).is_some());
        bytes.push(0);
        assert_eq!(written(&bytes), None);
        bytes.truncate(8);
        assert_eq!(written(&bytes), None);
        assert_eq!(written(&[]), None);
    }

    #[test]
    fn a_request_is_framed_as_the_standard_writes_it() {
        let read = Request::ReadWords {
            table: Table::Holding,
            offset: 100,
            count: 2,
        };
        assert_eq!(pdu(&read).unwrap(), [0x03, 0x00, 0x64, 0x00, 0x02]);
        assert_eq!(
            frame(1, 7, &pdu(&read).unwrap()).unwrap(),
            [
                0x00, 0x01, 0x00, 0x00, 0x00, 0x06, 0x07, 0x03, 0x00, 0x64, 0x00, 0x02
            ]
        );
        let coil = Request::WriteBit {
            offset: 4,
            on: true,
        };
        assert_eq!(pdu(&coil).unwrap(), [0x05, 0x00, 0x04, 0xFF, 0x00]);
        let two = Request::WriteWords {
            offset: 104,
            words: vec![0x1388, 0x0000],
        };
        assert_eq!(
            pdu(&two).unwrap(),
            [0x10, 0x00, 0x68, 0x00, 0x02, 0x04, 0x13, 0x88, 0x00, 0x00]
        );
        // A table that holds words is never read as bits, and no frame asks
        // for more than the standard allows.
        assert!(
            pdu(&Request::ReadBits {
                table: Table::Holding,
                offset: 0,
                count: 1
            })
            .is_none()
        );
        assert!(
            pdu(&Request::ReadWords {
                table: Table::Input,
                offset: 0,
                count: 126
            })
            .is_none()
        );
    }

    #[test]
    fn a_reply_is_read_only_in_the_form_declared_for_its_request() {
        let read = Request::ReadWords {
            table: Table::Input,
            offset: 30,
            count: 1,
        };
        assert_eq!(
            answer(&read, &[0x04, 0x02, 0x2E, 0xE0]),
            Some(Reply::Words(vec![12_000]))
        );
        // Another function, a wrong byte count, or a short body is no reply.
        assert_eq!(answer(&read, &[0x03, 0x02, 0x2E, 0xE0]), None);
        assert_eq!(answer(&read, &[0x04, 0x04, 0x2E, 0xE0]), None);
        assert_eq!(answer(&read, &[0x04, 0x02, 0x2E]), None);
        assert_eq!(
            answer(&read, &[0x84, 0x06]),
            Some(Reply::Refused(Refusal::Busy))
        );
        let bits = Request::ReadBits {
            table: Table::Coil,
            offset: 0,
            count: 10,
        };
        assert_eq!(
            answer(&bits, &[0x01, 0x02, 0b0001_0001, 0b0000_0010]),
            Some(Reply::Bits(vec![
                true, false, false, false, true, false, false, false, false, true
            ]))
        );
        let write = Request::WriteWords {
            offset: 102,
            words: vec![90],
        };
        assert_eq!(
            answer(&write, &[0x06, 0x00, 0x66, 0x00, 0x5A]),
            Some(Reply::Written)
        );
        // An echo of another value is not an acknowledgement.
        assert_eq!(answer(&write, &[0x06, 0x00, 0x66, 0x00, 0x50]), None);
        assert_eq!(
            answer(&write, &[0x86, 0x03]),
            Some(Reply::Refused(Refusal::Value))
        );
    }

    #[test]
    fn identification_is_read_object_by_object() {
        let mut reply = vec![0x2B, 0x0E, 0x03, 0x83, 0x00, 0x00, 0x02];
        reply.extend([0x01, 0x03]);
        reply.extend(b"abc");
        reply.extend([0x02, 0x05]);
        reply.extend(b"2.4.0");
        let Some(Reply::Identity(found)) = answer(&Request::Identify, &reply) else {
            panic!("an identity");
        };
        assert_eq!(found.get("model").map(String::as_str), Some("abc"));
        assert_eq!(
            found.get("firmware_version").map(String::as_str),
            Some("2.4.0")
        );
        // An object that runs past the reply, or a reply that says more
        // follows, is not read.
        let mut short = reply.clone();
        short.pop();
        assert_eq!(answer(&Request::Identify, &short), None);
        let mut more = reply;
        more[4] = 0xFF;
        assert_eq!(answer(&Request::Identify, &more), None);
    }
}
