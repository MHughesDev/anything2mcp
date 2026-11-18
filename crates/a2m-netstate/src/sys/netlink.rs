//! The Linux kernel's route-netlink tables, read by dump.
//!
//! A route-netlink socket speaks only to this machine's kernel: the one
//! message sent here is addressed to the kernel (port 0, no groups) and asks
//! it to list one table. Nothing reaches a network. The request types are a
//! closed list of four `GET` dumps; none adds, changes, or removes an entry,
//! and none makes the kernel ask another machine anything (a neighbour dump
//! lists the cache; it does not refresh it).

use std::os::fd::OwnedFd;
use std::time::Duration;

use rustix::net::netlink::SocketAddrNetlink;
use rustix::net::sockopt::{self, Timeout};
use rustix::net::{AddressFamily, RecvFlags, SendFlags, SocketAddrAny, SocketFlags, SocketType};

use crate::bound::MOST_TABLE_BYTES;
use crate::error::{Fact, NetError};

/// Ask for the link table.
pub(crate) const RTM_GETLINK: u16 = 18;
/// Ask for the address table.
pub(crate) const RTM_GETADDR: u16 = 22;
/// Ask for the route table.
pub(crate) const RTM_GETROUTE: u16 = 26;
/// Ask for the neighbour table.
pub(crate) const RTM_GETNEIGH: u16 = 30;

/// The only requests this module sends. A test holds the list.
pub(crate) const REQUESTS: &[u16] = &[RTM_GETLINK, RTM_GETADDR, RTM_GETROUTE, RTM_GETNEIGH];

const NLMSG_ERROR: u16 = 2;
const NLMSG_DONE: u16 = 3;
const NLM_F_REQUEST: u16 = 0x1;
const NLM_F_DUMP: u16 = 0x300;
const NLM_F_DUMP_INTR: u16 = 0x10;
const HEADER: usize = 16;
const SEQUENCE: u32 = 1;
/// The longest one receive waits. The kernel answers a dump at once; a
/// receive that waits this long is a failure (`net_read_failed`), never a
/// thread blocked for good.
const READ_TIMEOUT: Duration = Duration::from_secs(5);
/// The most datagrams from a sender other than the kernel dropped in one
/// dump before the reader stops.
const MOST_FOREIGN_DATAGRAMS: usize = 1024;

/// One message of a dump: its type and its body after the header.
pub(crate) struct Message {
    pub(crate) kind: u16,
    pub(crate) body: Vec<u8>,
}

/// A dump: its messages, and whether it was cut at [`MOST_TABLE_BYTES`].
pub(crate) struct Dump {
    pub(crate) messages: Vec<Message>,
    pub(crate) cut: bool,
}

/// The length of the request body each request type takes: `ifinfomsg`,
/// `ifaddrmsg`, `rtmsg`, and `ndmsg`.
fn body_length(request: u16) -> Option<usize> {
    match request {
        RTM_GETLINK => Some(16),
        RTM_GETADDR => Some(8),
        RTM_GETROUTE | RTM_GETNEIGH => Some(12),
        _ => None,
    }
}

/// Dumps one table, for every address family. A dump the kernel marks as
/// interrupted by a change is read again, up to three times.
pub(crate) fn dump(request: u16, fact: Fact) -> Result<Dump, NetError> {
    for _ in 0..3 {
        if let Some(dump) = dump_once(request, fact)? {
            return Ok(dump);
        }
    }
    Err(NetError::Unreadable {
        fact,
        what: "the table changed each time it was read",
    })
}

fn dump_once(request: u16, fact: Fact) -> Result<Option<Dump>, NetError> {
    let Some(body) = body_length(request).filter(|_| REQUESTS.contains(&request)) else {
        return Err(NetError::Unreadable {
            fact,
            what: "a request this reader does not send",
        });
    };
    let socket = open(fact, READ_TIMEOUT)?;
    let length = HEADER + body;
    let mut message = Vec::with_capacity(length);
    message.extend_from_slice(&u32::try_from(length).unwrap_or(0).to_ne_bytes());
    message.extend_from_slice(&request.to_ne_bytes());
    message.extend_from_slice(&(NLM_F_REQUEST | NLM_F_DUMP).to_ne_bytes());
    message.extend_from_slice(&SEQUENCE.to_ne_bytes());
    message.extend_from_slice(&0u32.to_ne_bytes());
    // The body is all zero: address family 0 asks for every family, and no
    // field filters or sets anything.
    message.resize(length, 0);
    // To the kernel: port 0, no multicast groups.
    let kernel = SocketAddrNetlink::new(0, 0);
    rustix::net::sendto(&socket, &message, SendFlags::empty(), &kernel).map_err(io(fact))?;
    read_dump(&socket, fact)
}

fn io(fact: Fact) -> impl Fn(rustix::io::Errno) -> NetError {
    move |e| NetError::from_io(fact, &std::io::Error::from(e))
}

/// A route-netlink socket whose every receive waits at most `timeout`.
fn open(fact: Fact, timeout: Duration) -> Result<OwnedFd, NetError> {
    let socket = rustix::net::socket_with(
        AddressFamily::NETLINK,
        SocketType::RAW,
        SocketFlags::CLOEXEC,
        None,
    )
    .map_err(io(fact))?;
    sockopt::set_socket_timeout(&socket, Timeout::Recv, Some(timeout)).map_err(io(fact))?;
    Ok(socket)
}

/// Whether a datagram came from this machine's kernel, which sends from
/// port 0. Another process in the same namespace may address this socket's
/// port; what it sends is dropped unread.
fn from_kernel(sender: Option<SocketAddrAny>) -> bool {
    sender
        .and_then(|address| SocketAddrNetlink::try_from(address).ok())
        .is_some_and(|address| address.pid() == 0)
}

/// Reads the kernel's answer to the one request sent on `socket`.
fn read_dump(socket: &OwnedFd, fact: Fact) -> Result<Option<Dump>, NetError> {
    let mut messages = Vec::new();
    let mut total = 0usize;
    let mut foreign = 0usize;
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let (_, received, sender) =
            rustix::net::recvfrom(socket, &mut buffer[..], RecvFlags::TRUNC).map_err(io(fact))?;
        if !from_kernel(sender) {
            foreign += 1;
            if foreign > MOST_FOREIGN_DATAGRAMS {
                return Err(NetError::Unreadable {
                    fact,
                    what: "many messages from a sender other than the kernel",
                });
            }
            continue;
        }
        if received > buffer.len() {
            return Err(NetError::Unreadable {
                fact,
                what: "a message longer than the reader's buffer",
            });
        }
        total += received;
        let mut at = 0usize;
        let datagram = &buffer[..received];
        while at + HEADER <= datagram.len() {
            let field = |from: usize, to: usize| datagram.get(at + from..at + to);
            let (Some(len), Some(kind), Some(flags), Some(sequence)) =
                (field(0, 4), field(4, 6), field(6, 8), field(8, 12))
            else {
                break;
            };
            let len = u32_of(len) as usize;
            let kind = u16_of(kind);
            let flags = u16_of(flags);
            if len < HEADER || at + len > datagram.len() {
                return Err(NetError::Unreadable {
                    fact,
                    what: "a message whose length does not fit its datagram",
                });
            }
            if u32_of(sequence) == SEQUENCE {
                if flags & NLM_F_DUMP_INTR != 0 {
                    return Ok(None);
                }
                match kind {
                    NLMSG_DONE => {
                        return Ok(Some(Dump {
                            messages,
                            cut: false,
                        }));
                    }
                    NLMSG_ERROR => {
                        let code = datagram
                            .get(at + HEADER..at + HEADER + 4)
                            .map(|b| i32::from_ne_bytes([b[0], b[1], b[2], b[3]]))
                            .unwrap_or(0);
                        if code == 0 {
                            return Ok(Some(Dump {
                                messages,
                                cut: false,
                            }));
                        }
                        let error = std::io::Error::from_raw_os_error(code.saturating_neg());
                        return Err(NetError::from_io(fact, &error));
                    }
                    _ => messages.push(Message {
                        kind,
                        body: datagram[at + HEADER..at + len].to_vec(),
                    }),
                }
            }
            at += align(len);
        }
        if total > MOST_TABLE_BYTES {
            return Ok(Some(Dump {
                messages,
                cut: true,
            }));
        }
    }
}

/// Rounds a length up to netlink's four-byte alignment.
pub(crate) fn align(length: usize) -> usize {
    (length + 3) & !3
}

pub(crate) fn u16_of(bytes: &[u8]) -> u16 {
    match bytes {
        [a, b, ..] => u16::from_ne_bytes([*a, *b]),
        _ => 0,
    }
}

pub(crate) fn u32_of(bytes: &[u8]) -> u32 {
    match bytes {
        [a, b, c, d, ..] => u32::from_ne_bytes([*a, *b, *c, *d]),
        _ => 0,
    }
}

/// The attributes after a message's fixed part: each type, with the nested
/// and byte-order bits cleared, and its payload. A malformed attribute ends
/// the list.
pub(crate) fn attributes(bytes: &[u8]) -> Vec<(u16, &[u8])> {
    let mut found = Vec::new();
    let mut at = 0usize;
    while at + 4 <= bytes.len() {
        let length = usize::from(u16_of(&bytes[at..]));
        let kind = u16_of(&bytes[at + 2..]) & 0x3fff;
        if length < 4 || at + length > bytes.len() {
            break;
        }
        found.push((kind, &bytes[at + 4..at + length]));
        at += align(length);
    }
    found
}

/// A string attribute, up to its first zero byte.
pub(crate) fn text_of(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attributes_are_read_and_a_malformed_one_ends_the_list() {
        let mut bytes = Vec::new();
        // One attribute of type 3 holding "lo\0", padded to four bytes.
        bytes.extend_from_slice(&7u16.to_ne_bytes());
        bytes.extend_from_slice(&3u16.to_ne_bytes());
        bytes.extend_from_slice(b"lo\0\0");
        // A nested attribute of type 18 (the nested bit set) holding nothing.
        bytes.extend_from_slice(&4u16.to_ne_bytes());
        bytes.extend_from_slice(&(18u16 | 0x8000).to_ne_bytes());
        // A malformed one: a length past the end.
        bytes.extend_from_slice(&200u16.to_ne_bytes());
        bytes.extend_from_slice(&1u16.to_ne_bytes());
        let found = attributes(&bytes);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].0, 3);
        assert_eq!(text_of(found[0].1), "lo");
        assert_eq!(found[1], (18, &[][..]));
    }

    #[test]
    fn only_the_four_listing_requests_are_sent() {
        assert_eq!(
            REQUESTS,
            &[RTM_GETLINK, RTM_GETADDR, RTM_GETROUTE, RTM_GETNEIGH]
        );
        // Every request type is a `GET`: in route-netlink, a request type
        // whose remainder modulo four is 2 asks for a table.
        for request in REQUESTS {
            assert_eq!(request % 4, 2, "{request}");
        }
        // A request outside the list is refused before anything is sent.
        assert!(matches!(
            dump(16, Fact::Interfaces),
            Err(NetError::Unreadable { .. })
        ));
    }

    /// A request for the link table, as `dump_once` sends it.
    fn link_request() -> Vec<u8> {
        let length = HEADER + 16;
        let mut message = Vec::new();
        message.extend_from_slice(&u32::try_from(length).unwrap().to_ne_bytes());
        message.extend_from_slice(&RTM_GETLINK.to_ne_bytes());
        message.extend_from_slice(&(NLM_F_REQUEST | NLM_F_DUMP).to_ne_bytes());
        message.extend_from_slice(&SEQUENCE.to_ne_bytes());
        message.extend_from_slice(&0u32.to_ne_bytes());
        message.resize(length, 0);
        message
    }

    #[test]
    fn a_message_from_another_process_is_dropped_unread() {
        let fact = Fact::Interfaces;
        let reader = open(fact, READ_TIMEOUT).unwrap();
        // Take a port now, so that another socket can address it.
        rustix::net::bind(&reader, &SocketAddrNetlink::new(0, 0)).unwrap();
        let port = SocketAddrNetlink::try_from(rustix::net::getsockname(&reader).unwrap())
            .unwrap()
            .pid();
        assert_ne!(port, 0);
        // Another socket, standing for another process, sends this reader a
        // forged end of the dump before the kernel answers.
        let forger = open(fact, READ_TIMEOUT).unwrap();
        let mut forged = Vec::new();
        forged.extend_from_slice(&20u32.to_ne_bytes());
        forged.extend_from_slice(&NLMSG_DONE.to_ne_bytes());
        forged.extend_from_slice(&0u16.to_ne_bytes());
        forged.extend_from_slice(&SEQUENCE.to_ne_bytes());
        forged.extend_from_slice(&0u32.to_ne_bytes());
        forged.extend_from_slice(&0i32.to_ne_bytes());
        rustix::net::sendto(
            &forger,
            &forged,
            SendFlags::empty(),
            &SocketAddrNetlink::new(port, 0),
        )
        .unwrap();
        rustix::net::sendto(
            &reader,
            &link_request(),
            SendFlags::empty(),
            &SocketAddrNetlink::new(0, 0),
        )
        .unwrap();
        // Read as the forged end, the dump would hold nothing; the kernel's
        // answer holds at least the loopback interface.
        let dump = read_dump(&reader, fact).unwrap().unwrap();
        assert!(dump.messages.iter().any(|m| m.kind == 16));
    }

    #[test]
    fn a_kernel_that_never_answers_is_a_read_failure_not_a_wait_for_good() {
        let fact = Fact::Routes;
        // Nothing was asked, so nothing comes; the receive's limit ends it.
        let socket = open(fact, Duration::from_millis(50)).unwrap();
        let error = read_dump(&socket, fact).err().unwrap();
        assert!(matches!(error, NetError::Failed { .. }), "{error:?}");
        assert_eq!(a2m_core::Failure::from(error).code(), &NetError::FAILED);
    }
}
