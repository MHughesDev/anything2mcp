//! The trace signature: a digest over what a run did, the same for two runs
//! that did the same things (spec section 217; decision 0076).
//!
//! Each record gives one digest, over the parts of it that should not vary.
//! The signature chains those digests in the order the records were made and
//! closes the chain with their count. Timings, identifiers, versions, scores,
//! measurements, capture references, the capture level, and values the
//! compiled target declares volatile never enter.

use a2m_core::Digest;

/// What starts every record's bytes, so that a record's digest can never be
/// taken for a digest of anything else.
const RECORD_DOMAIN: &[u8] = b"anything2mcp trace record 1";

/// What the chain starts from.
const CHAIN_DOMAIN: &[u8] = b"anything2mcp trace signature 1";

/// What closes the chain.
const END_DOMAIN: &[u8] = b"anything2mcp trace end 1";

/// Builds the bytes one record contributes. Every entry is written as its
/// name's length, its name, its value's length, and its value, so no two
/// different records can give the same bytes.
pub(crate) struct Canon(Vec<u8>);

impl Canon {
    pub(crate) fn record() -> Self {
        let mut canon = Self(Vec::with_capacity(256));
        canon.bytes("domain", RECORD_DOMAIN);
        canon
    }

    pub(crate) fn bytes(&mut self, name: &str, value: &[u8]) {
        self.0.extend_from_slice(&(name.len() as u64).to_le_bytes());
        self.0.extend_from_slice(name.as_bytes());
        self.0
            .extend_from_slice(&(value.len() as u64).to_le_bytes());
        self.0.extend_from_slice(value);
    }

    pub(crate) fn text(&mut self, name: &str, value: &str) {
        self.bytes(name, value.as_bytes());
    }

    pub(crate) fn number(&mut self, name: &str, value: u64) {
        self.bytes(name, &value.to_le_bytes());
    }

    /// An entry that may be absent. Absent and present-but-empty differ.
    pub(crate) fn optional(&mut self, name: &str, value: Option<&[u8]>) {
        match value {
            None => self.bytes(name, &[0]),
            Some(value) => {
                let mut tagged = Vec::with_capacity(value.len() + 1);
                tagged.push(1);
                tagged.extend_from_slice(value);
                self.bytes(name, &tagged);
            }
        }
    }

    pub(crate) fn finish(self) -> Digest {
        Digest::of(&self.0)
    }
}

/// The chain of record digests.
#[derive(Debug, Clone)]
pub(crate) struct Chain {
    state: Digest,
    records: u32,
}

impl Chain {
    pub(crate) fn new() -> Self {
        Self {
            state: Digest::of(CHAIN_DOMAIN),
            records: 0,
        }
    }

    /// Adds the next record's digest.
    pub(crate) fn push(&mut self, record: &Digest) {
        let mut bytes = [0u8; Digest::LENGTH * 2];
        bytes[..Digest::LENGTH].copy_from_slice(self.state.as_bytes());
        bytes[Digest::LENGTH..].copy_from_slice(record.as_bytes());
        self.state = Digest::of(&bytes);
        self.records = self.records.saturating_add(1);
    }

    pub(crate) const fn records(&self) -> u32 {
        self.records
    }

    /// The signature of the records pushed so far.
    pub(crate) fn signature(&self) -> Digest {
        let mut bytes = Vec::with_capacity(END_DOMAIN.len() + Digest::LENGTH + 8);
        bytes.extend_from_slice(END_DOMAIN);
        bytes.extend_from_slice(self.state.as_bytes());
        bytes.extend_from_slice(&u64::from(self.records).to_le_bytes());
        Digest::of(&bytes)
    }
}

#[cfg(test)]
mod tests {
    use a2m_core::Digest;

    use super::{Canon, Chain};

    #[test]
    fn entries_cannot_be_confused_with_each_other() {
        let digest = |entries: &[(&str, &str)]| {
            let mut canon = Canon::record();
            for (name, value) in entries {
                canon.text(name, value);
            }
            canon.finish()
        };
        assert_ne!(digest(&[("a", "bc")]), digest(&[("ab", "c")]));
        assert_ne!(
            digest(&[("a", "b"), ("c", "d")]),
            digest(&[("a", "bc"), ("", "d")])
        );
        assert_eq!(digest(&[("a", "b")]), digest(&[("a", "b")]));
    }

    #[test]
    fn absent_is_not_empty() {
        let digest = |value: Option<&[u8]>| {
            let mut canon = Canon::record();
            canon.optional("value", value);
            canon.finish()
        };
        assert_ne!(digest(None), digest(Some(b"")));
        assert_ne!(digest(Some(b"")), digest(Some(&[0])));
    }

    #[test]
    fn the_chain_depends_on_order_and_on_count() {
        let (a, b) = (Digest::of(b"a"), Digest::of(b"b"));
        let chain = |records: &[Digest]| {
            let mut chain = Chain::new();
            for record in records {
                chain.push(record);
            }
            chain.signature()
        };
        assert_eq!(chain(&[a, b]), chain(&[a, b]));
        assert_ne!(chain(&[a, b]), chain(&[b, a]));
        assert_ne!(chain(&[a]), chain(&[a, a]));
        assert_ne!(chain(&[]), chain(&[a]));
    }
}
