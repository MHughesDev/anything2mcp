//! The trace context a caller sends, so that its trace and ours join into one.
//!
//! The text form is the W3C `traceparent` header: a version, a trace id, the
//! parent's span id, and flags, as lowercase hexadecimal joined by hyphens.
//! Only that form is read. Anything else a caller sends is ignored.

use std::fmt;

/// A trace id and the span id of the parent, parsed and checked.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct TraceContext {
    trace: u128,
    parent: u64,
    sampled: bool,
}

impl TraceContext {
    /// Reads a `traceparent` value. Returns `None` for anything that is not
    /// exactly the standard form: a caller's mistake never stops a call.
    #[must_use]
    pub fn parse(traceparent: &str) -> Option<Self> {
        let mut parts = traceparent.split('-');
        let version = hex_part(parts.next()?, 2)?;
        let trace = hex_part(parts.next()?, 32)?;
        let parent = hex_part(parts.next()?, 16)?;
        let flags = hex_part(parts.next()?, 2)?;
        // Version 00 has exactly four parts; a later version may add more.
        if version == 0xff || (version == 0 && parts.next().is_some()) {
            return None;
        }
        if trace == 0 || parent == 0 {
            return None;
        }
        Some(Self {
            trace,
            parent: u64::try_from(parent).ok()?,
            sampled: flags & 1 == 1,
        })
    }

    pub(crate) const fn new(trace: u128, parent: u64) -> Self {
        Self {
            trace,
            parent,
            sampled: false,
        }
    }

    /// The trace this context belongs to.
    #[must_use]
    pub const fn trace_id(&self) -> u128 {
        self.trace
    }

    /// The span that a new span made under this context is a child of.
    #[must_use]
    pub const fn span_id(&self) -> u64 {
        self.parent
    }

    /// Whether the caller asked for this trace to be recorded.
    #[must_use]
    pub const fn sampled(&self) -> bool {
        self.sampled
    }
}

impl fmt::Display for TraceContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "00-{:032x}-{:016x}-{:02x}",
            self.trace,
            self.parent,
            u8::from(self.sampled)
        )
    }
}

impl fmt::Debug for TraceContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TraceContext({self})")
    }
}

/// One part of the header: exactly `length` lowercase hexadecimal characters.
fn hex_part(text: &str, length: usize) -> Option<u128> {
    if text.len() != length
        || !text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return None;
    }
    u128::from_str_radix(text, 16).ok()
}

#[cfg(test)]
mod tests {
    use super::TraceContext;

    const GOOD: &str = "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01";

    #[test]
    fn the_standard_form_is_read_and_written_back() {
        let context = TraceContext::parse(GOOD).unwrap();
        assert_eq!(context.trace_id(), 0x0af7_6519_16cd_43dd_8448_eb21_1c80_319c);
        assert_eq!(context.span_id(), 0xb7ad_6b71_6920_3331);
        assert!(context.sampled());
        assert_eq!(context.to_string(), GOOD);
    }

    #[test]
    fn anything_else_is_ignored() {
        let bad = [
            "",
            "00",
            "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331",
            "00-0AF7651916CD43DD8448EB211C80319C-b7ad6b7169203331-01",
            "00-00000000000000000000000000000000-b7ad6b7169203331-01",
            "00-0af7651916cd43dd8448eb211c80319c-0000000000000000-01",
            "ff-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01",
            "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01-extra",
            "00-0af7651916cd43dd8448eb211c80319-b7ad6b7169203331-01",
            "00-+af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01",
        ];
        for text in bad {
            assert!(TraceContext::parse(text).is_none(), "{text}");
        }
    }

    #[test]
    fn a_later_version_may_carry_more_parts() {
        let later = "01-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-00-more";
        let context = TraceContext::parse(later).unwrap();
        assert!(!context.sampled());
    }
}
