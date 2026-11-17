//! Faults a test can switch on, so that a failure is reproduced on demand.
//!
//! A fault applies to frames as they arrive. Each frame takes at most one
//! fault: the first in the list whose scope matches and whose `skip` has run out.

use crate::protocol::{Exception, FrameKind};

/// Which frames a fault applies to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// Every frame.
    All,
    /// Frames of a read function.
    Reads,
    /// Frames of a write function.
    Writes,
    /// Frames of one function code.
    Function(u8),
}

impl Scope {
    pub(crate) fn matches(self, kind: FrameKind, function: u8) -> bool {
        match self {
            Self::All => true,
            Self::Reads => kind == FrameKind::Read,
            Self::Writes => kind == FrameKind::Write,
            Self::Function(wanted) => wanted == function,
        }
    }
}

/// How a reply is damaged on its way out. The request was carried out first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Corruption {
    /// The reply carries another transaction identifier than the request.
    TransactionId,
    /// The reply's last byte is inverted: a wrong value in a read, a wrong echo in a write.
    LastByte,
    /// The reply stops one byte short of the length its header states, and the link closes.
    Truncated,
}

/// What a fault does to a frame it applies to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaultKind {
    /// The link closes with no reply. With `applied`, the device carried the
    /// request out first: for a write, a possible commit.
    DropLink {
        /// Whether the request is carried out before the link closes.
        applied: bool,
    },
    /// No reply is sent and the link stays open: a time-out for the client.
    NoReply {
        /// Whether the request is carried out all the same.
        applied: bool,
    },
    /// The request is carried out and answered after this long.
    Delay {
        /// Milliseconds to wait before answering.
        ms: u64,
    },
    /// The device answers with this exception code and does nothing else.
    Exception {
        /// The exception code, as it travels.
        code: u8,
    },
    /// The request is carried out and the reply is damaged.
    CorruptReply(Corruption),
    /// The device restarts in place of handling the frame: no reply, every link
    /// closes, and volatile points start again.
    Restart,
}

/// One fault, the frames it applies to, and for how long.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fault {
    /// What happens to a matching frame.
    pub kind: FaultKind,
    /// Which frames match.
    pub scope: Scope,
    /// How many matching frames pass untouched before the fault starts.
    pub skip: u32,
    /// How many frames it applies to before it clears itself. Until cleared, if absent.
    pub times: Option<u32>,
}

impl Fault {
    /// A fault for the next frame only.
    #[must_use]
    pub const fn once(kind: FaultKind) -> Self {
        Self {
            kind,
            scope: Scope::All,
            skip: 0,
            times: Some(1),
        }
    }

    /// A fault for every frame until faults are cleared.
    #[must_use]
    pub const fn always(kind: FaultKind) -> Self {
        Self {
            kind,
            scope: Scope::All,
            skip: 0,
            times: None,
        }
    }

    /// The device is busy for the next frame: exception 06.
    #[must_use]
    pub const fn busy() -> Self {
        Self::once(FaultKind::Exception {
            code: Exception::ServerDeviceBusy.code(),
        })
    }

    /// The same fault, limited to the frames in `scope`.
    #[must_use]
    pub const fn on(mut self, scope: Scope) -> Self {
        self.scope = scope;
        self
    }

    /// The same fault, starting after `skip` matching frames have passed.
    #[must_use]
    pub const fn after(mut self, skip: u32) -> Self {
        self.skip = skip;
        self
    }

    /// The same fault, for this many frames.
    #[must_use]
    pub const fn times(mut self, times: u32) -> Self {
        self.times = Some(times);
        self
    }
}

/// The fault for a frame, used up by one.
pub(crate) fn take(faults: &mut Vec<Fault>, kind: FrameKind, function: u8) -> Option<FaultKind> {
    let mut taken = None;
    for (index, fault) in faults.iter_mut().enumerate() {
        if !fault.scope.matches(kind, function) {
            continue;
        }
        if fault.skip > 0 {
            fault.skip -= 1;
            continue;
        }
        if let Some(times) = fault.times.as_mut() {
            *times = times.saturating_sub(1);
        }
        taken = Some((index, fault.kind, fault.times == Some(0)));
        break;
    }
    let (index, kind, used_up) = taken?;
    if used_up {
        faults.remove(index);
    }
    Some(kind)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fault_applies_to_its_scope_after_its_skip_and_then_clears() {
        let restart = Fault::once(FaultKind::Restart).on(Scope::Writes).after(1);
        let mut faults = vec![
            restart,
            Fault::always(FaultKind::Delay { ms: 5 }).on(Scope::Reads),
        ];
        assert_eq!(
            take(&mut faults, FrameKind::Read, 3),
            Some(FaultKind::Delay { ms: 5 })
        );
        assert_eq!(take(&mut faults, FrameKind::Write, 6), None);
        assert_eq!(
            take(&mut faults, FrameKind::Write, 6),
            Some(FaultKind::Restart)
        );
        assert_eq!(take(&mut faults, FrameKind::Write, 6), None);
        assert_eq!(faults.len(), 1);
        assert_eq!(take(&mut faults, FrameKind::Other, 8), None);
    }
}
