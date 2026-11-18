//! What a watch compares: the digests of what one check of a read returned,
//! the baseline of what earlier checks saw, and the pure comparison of the
//! two (decision 0247). Nothing here keeps or sees a value once its digest
//! is made.
//!
//! A record is named by the workspace's keyed digest of its key under the
//! purpose `record_key`, the name the ledger gives a key in a record row and
//! a lock, so a change and a write can be matched. A read whose output is not
//! a list of keyed records is compared whole, as one record named
//! [`WatchKey::Whole`]. Each output's value is reduced to a keyed digest of
//! its canonical bytes under the purpose [`value_purpose`], with the output's
//! name inside what is digested, so that equal values of two outputs differ.

use std::collections::{BTreeMap, BTreeSet};

use a2m_core::{
    Digest, DigestPurpose, Failure, FailureClass, FailureCode, KeyedDigest, RecordKey, Values,
    WorkspaceId,
};

/// The purpose the digests of a watch's values are made under.
///
/// `a2m-core` has no purpose of a watch's own yet, and this crate does not
/// own that list: until it has one, the nearest, `restricted_value`, is used
/// here and nowhere else in this crate, and every digest carries the prefix
/// [`VALUE_DOMAIN`] so that it never equals a digest a trace or a diff makes
/// of the same value. Adding the purpose is a change of this one line.
#[must_use]
pub const fn value_purpose() -> DigestPurpose {
    DigestPurpose::RestrictedValue
}

/// What every digest of a watch's value begins with.
pub const VALUE_DOMAIN: &[u8] = b"anything2mcp watch value 1";
/// What every digest of a watch's whole record begins with.
const RECORD_DOMAIN: &[u8] = b"anything2mcp watch record 1";

/// A record's key appears twice in one check: a named fault of that check,
/// never a merge. Class `unknown_variant`.
pub const WATCH_KEY_REPEATED: FailureCode = FailureCode::fixed("watch_key_repeated");
/// A check holds keyed records and a whole result at once, or more than one
/// whole result. Class `unknown_variant`.
pub const WATCH_SHAPES_MIXED: FailureCode = FailureCode::fixed("watch_shapes_mixed");

/// How a watch names one record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum WatchKey {
    /// The whole result of a read that is not a list of keyed records.
    Whole,
    /// A record, by the workspace's keyed digest of its key under the
    /// purpose `record_key`.
    Record(Digest),
}

impl WatchKey {
    /// The text the store keeps.
    #[must_use]
    pub fn stored(&self) -> String {
        match self {
            Self::Whole => "whole".to_owned(),
            Self::Record(digest) => digest.to_string(),
        }
    }

    pub(crate) fn from_stored(text: &str) -> Option<Self> {
        if text == "whole" {
            return Some(Self::Whole);
        }
        text.parse().ok().map(Self::Record)
    }

    /// The name of a record's key, as the ledger names it in a record row.
    #[must_use]
    pub fn of(digest: &dyn KeyedDigest, workspace: &WorkspaceId, key: &RecordKey) -> Self {
        Self::Record(digest.keyed_digest(
            workspace,
            DigestPurpose::RecordKey,
            &key.canonical_bytes(),
        ))
    }
}

/// The digests of one record: of its values together, and of each output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordDigests {
    /// The digest of all its compared outputs together.
    pub whole: Digest,
    /// The digest of each compared output's value, by the output's name.
    pub outputs: BTreeMap<String, Digest>,
}

impl RecordDigests {
    /// The digests of one record's normalized values, leaving out the
    /// outputs named in `ignore`.
    #[must_use]
    pub fn of(
        digest: &dyn KeyedDigest,
        workspace: &WorkspaceId,
        values: &Values,
        ignore: &BTreeSet<String>,
    ) -> Self {
        let mut outputs = BTreeMap::new();
        for (name, value) in values.iter() {
            if ignore.contains(name) {
                continue;
            }
            let mut bytes = framed(VALUE_DOMAIN);
            bytes.extend_from_slice(&framed(name.as_bytes()));
            bytes.extend_from_slice(&value.canonical_bytes());
            outputs.insert(
                name.to_owned(),
                digest.keyed_digest(workspace, value_purpose(), &bytes),
            );
        }
        Self::from_outputs(digest, workspace, outputs)
    }

    fn from_outputs(
        digest: &dyn KeyedDigest,
        workspace: &WorkspaceId,
        outputs: BTreeMap<String, Digest>,
    ) -> Self {
        let mut bytes = framed(RECORD_DOMAIN);
        for (name, output) in &outputs {
            bytes.extend_from_slice(&framed(name.as_bytes()));
            bytes.extend_from_slice(output.as_bytes());
        }
        Self {
            whole: digest.keyed_digest(workspace, value_purpose(), &bytes),
            outputs,
        }
    }

    /// The names of the outputs whose digests differ, or that one side
    /// holds and the other does not, in name order.
    #[must_use]
    pub fn differing(&self, other: &Self) -> Vec<String> {
        let names: BTreeSet<&String> = self.outputs.keys().chain(other.outputs.keys()).collect();
        names
            .into_iter()
            .filter(|name| self.outputs.get(*name) != other.outputs.get(*name))
            .cloned()
            .collect()
    }
}

fn framed(bytes: &[u8]) -> Vec<u8> {
    let mut out = (bytes.len() as u64).to_le_bytes().to_vec();
    out.extend_from_slice(bytes);
    out
}

/// What is wrong with a check's records, so that it is not compared.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckFault {
    /// Two records of the check have one key.
    KeyRepeated(WatchKey),
    /// The check holds keyed records and a whole result at once.
    ShapesMixed,
}

impl CheckFault {
    /// The failure the check is recorded with.
    #[must_use]
    pub fn failure(&self) -> Failure {
        match self {
            Self::KeyRepeated(_) => Failure::new(
                FailureClass::UnknownVariant,
                WATCH_KEY_REPEATED,
                "Two records the read returned have one key, so the check cannot be compared.",
                "Check the key the operation declares for its records; the watch goes on and compares the next check.",
            ),
            Self::ShapesMixed => Failure::new(
                FailureClass::UnknownVariant,
                WATCH_SHAPES_MIXED,
                "The read returned keyed records and a whole result at once, so the check cannot be compared.",
                "Check the operation's outputs; the watch goes on and compares the next check.",
            ),
        }
    }
}

/// The digests of one check, by record. Two records with one key are never
/// merged: [`CheckDigests::new`] refuses them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckDigests {
    complete: bool,
    records: BTreeMap<WatchKey, RecordDigests>,
}

impl CheckDigests {
    /// The digests of a check, in any order. `complete` says the check's run
    /// completed and the read returned every record that matches its input.
    ///
    /// # Errors
    ///
    /// [`CheckFault::KeyRepeated`] when two records have one key;
    /// [`CheckFault::ShapesMixed`] for a whole result beside keyed records.
    pub fn new(
        complete: bool,
        records: impl IntoIterator<Item = (WatchKey, RecordDigests)>,
    ) -> Result<Self, CheckFault> {
        let mut held = BTreeMap::new();
        for (key, digests) in records {
            if held.insert(key, digests).is_some() {
                return Err(CheckFault::KeyRepeated(key));
            }
        }
        if held.contains_key(&WatchKey::Whole) && held.len() > 1 {
            return Err(CheckFault::ShapesMixed);
        }
        Ok(Self {
            complete,
            records: held,
        })
    }

    /// Whether the check saw every record.
    #[must_use]
    pub const fn complete(&self) -> bool {
        self.complete
    }

    /// The records, by key.
    #[must_use]
    pub const fn records(&self) -> &BTreeMap<WatchKey, RecordDigests> {
        &self.records
    }

    /// How many records it saw.
    #[must_use]
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// Whether it saw none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}

/// One record the baseline holds: its digests, and the check that last saw
/// it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Held {
    /// The digests it had when last seen.
    pub digests: RecordDigests,
    /// The check that last saw it.
    pub last_seen: u64,
}

/// What a watch knows of the records it has seen: for each, its digests when
/// last seen. A check updates what it saw and leaves the rest in place, so a
/// record that left the view of a list cut short is not taken for a new one
/// when it comes back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Baseline {
    /// Whether a complete check is among those that made it. Only then can
    /// a record a complete check did not see be called removed.
    pub complete: bool,
    /// The last check whose digests it took.
    pub through: u64,
    /// The records, by key.
    pub records: BTreeMap<WatchKey, Held>,
}

impl Baseline {
    /// The baseline the first check of a watch makes. Nothing is reported
    /// for it: there is nothing to compare with.
    #[must_use]
    pub fn first(seq: u64, check: &CheckDigests) -> Self {
        Self {
            complete: check.complete,
            through: seq,
            records: check
                .records
                .iter()
                .map(|(key, digests)| {
                    (
                        *key,
                        Held {
                            digests: digests.clone(),
                            last_seen: seq,
                        },
                    )
                })
                .collect(),
        }
    }

    /// The baseline after check `seq`: what it saw replaces what was held,
    /// a record proven removed leaves, and every other record stays.
    #[must_use]
    pub fn after(&self, seq: u64, check: &CheckDigests) -> Self {
        let proven = removal_proven(self, check);
        let mut records: BTreeMap<WatchKey, Held> = self
            .records
            .iter()
            .filter(|(key, _)| !proven || check.records.contains_key(key))
            .map(|(key, held)| (*key, held.clone()))
            .collect();
        for (key, digests) in &check.records {
            records.insert(
                *key,
                Held {
                    digests: digests.clone(),
                    last_seen: seq,
                },
            );
        }
        Self {
            complete: self.complete || check.complete,
            through: seq,
            records,
        }
    }
}

/// A record whose digests differ: which outputs, by name, never by value,
/// and the check that last saw it before.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedRecord {
    /// The record.
    pub key: WatchKey,
    /// The outputs that differ, in name order.
    pub outputs: Vec<String>,
    /// The check that last saw it before this one.
    pub older: u64,
}

/// A record the newer check did not see, and the check that last saw it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingRecord {
    /// The record.
    pub key: WatchKey,
    /// The check that last saw it.
    pub older: u64,
}

/// What changed between a baseline and a newer check. Every list is in key
/// order. Unchanged records are counted, not listed.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Comparison {
    /// Records the watch had never seen.
    pub added: Vec<WatchKey>,
    /// Records seen before whose digests differ.
    pub changed: Vec<ChangedRecord>,
    /// Records the watch saw before and a complete check did not see, when
    /// the baseline also holds a complete check.
    pub removed: Vec<MissingRecord>,
    /// Records the last check saw and this one did not, where removal is
    /// not proven. A record is reported unseen once, when it leaves the
    /// view, not at every check after.
    pub unseen: Vec<MissingRecord>,
    /// Records seen before with the same digests.
    pub unchanged: u32,
}

impl Comparison {
    /// Whether nothing is to be reported.
    #[must_use]
    pub fn is_quiet(&self) -> bool {
        self.added.is_empty()
            && self.changed.is_empty()
            && self.removed.is_empty()
            && self.unseen.is_empty()
    }
}

fn removal_proven(older: &Baseline, newer: &CheckDigests) -> bool {
    older.complete && newer.complete
}

/// Compares a newer check with the baseline: a pure function of the two.
///
/// `added` is a key the baseline does not hold. `changed` is a key held
/// with other digests, with the names of the outputs that differ. `removed`
/// is a key held and not seen, only when both the baseline and the newer
/// check are complete; otherwise such a key is `unseen`, and only in the
/// check right after the one that last saw it.
#[must_use]
pub fn compare(older: &Baseline, newer: &CheckDigests) -> Comparison {
    let mut out = Comparison::default();
    for (key, digests) in &newer.records {
        match older.records.get(key) {
            None => out.added.push(*key),
            Some(held) if held.digests.whole == digests.whole => {
                out.unchanged = out.unchanged.saturating_add(1);
            }
            Some(held) => out.changed.push(ChangedRecord {
                key: *key,
                outputs: held.digests.differing(digests),
                older: held.last_seen,
            }),
        }
    }
    let proven = removal_proven(older, newer);
    for (key, held) in &older.records {
        if newer.records.contains_key(key) {
            continue;
        }
        let missing = MissingRecord {
            key: *key,
            older: held.last_seen,
        };
        if proven {
            out.removed.push(missing);
        } else if held.last_seen == older.through {
            out.unseen.push(missing);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A keyed digest for tests: the plain digest of the purpose, the
    /// workspace, and the bytes.
    struct Plain;

    impl KeyedDigest for Plain {
        fn keyed_digest(
            &self,
            workspace: &WorkspaceId,
            purpose: DigestPurpose,
            bytes: &[u8],
        ) -> Digest {
            let mut all = purpose.as_str().as_bytes().to_vec();
            all.extend_from_slice(workspace.as_bytes());
            all.extend_from_slice(bytes);
            Digest::of(&all)
        }
    }

    fn ws() -> WorkspaceId {
        WorkspaceId::from_bytes([3; 16])
    }

    fn key(n: u64) -> WatchKey {
        WatchKey::Record(Digest::of(&n.to_le_bytes()))
    }

    fn record(status: &str, total: i64) -> RecordDigests {
        let values: Values = [
            ("status", a2m_core::Value::from(status)),
            ("total", a2m_core::Value::from(total)),
        ]
        .into_iter()
        .collect();
        RecordDigests::of(&Plain, &ws(), &values, &BTreeSet::new())
    }

    /// A small generator with no outside crate: the same sequence on every
    /// machine.
    struct Seq(u64);

    impl Seq {
        fn next(&mut self) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            self.0 >> 33
        }
    }

    fn random_check(seq: &mut Seq) -> Vec<(WatchKey, RecordDigests)> {
        let count = seq.next() % 12;
        let mut keys = BTreeSet::new();
        while (keys.len() as u64) < count {
            keys.insert(seq.next() % 20);
        }
        keys.into_iter()
            .map(|n| {
                (
                    key(n),
                    record(["new", "shipped"][(seq.next() % 2) as usize], 1),
                )
            })
            .collect()
    }

    #[test]
    fn a_check_compared_with_itself_reports_nothing() {
        let mut seq = Seq(7);
        for _ in 0..200 {
            let records = random_check(&mut seq);
            for complete in [true, false] {
                let check = CheckDigests::new(complete, records.clone()).unwrap();
                let base = Baseline::first(0, &check);
                let compared = compare(&base, &check);
                assert!(compared.is_quiet(), "{compared:?}");
                assert_eq!(compared.unchanged as usize, check.len());
                assert_eq!(base.after(1, &check).records.len(), check.len());
            }
        }
    }

    #[test]
    fn the_order_of_records_does_not_matter() {
        let mut seq = Seq(11);
        for _ in 0..200 {
            let older = CheckDigests::new(true, random_check(&mut seq)).unwrap();
            let records = random_check(&mut seq);
            let mut shuffled = records.clone();
            // A reversal and a rotation: two orders other than the first.
            shuffled.reverse();
            if !shuffled.is_empty() {
                let by = (seq.next() as usize) % shuffled.len();
                shuffled.rotate_left(by);
            }
            let base = Baseline::first(0, &older);
            let one = compare(&base, &CheckDigests::new(true, records).unwrap());
            let two = compare(&base, &CheckDigests::new(true, shuffled).unwrap());
            assert_eq!(one, two);
        }
    }

    #[test]
    fn a_key_twice_in_one_check_is_a_named_fault_and_never_a_merge() {
        let mut seq = Seq(13);
        for _ in 0..100 {
            let mut records = random_check(&mut seq);
            if records.is_empty() {
                continue;
            }
            let at = (seq.next() as usize) % records.len();
            let again = (records[at].0, record("other", 2));
            records.push(again);
            let fault = CheckDigests::new(true, records).unwrap_err();
            assert_eq!(fault, CheckFault::KeyRepeated(again.0));
            assert_eq!(fault.failure().code(), &WATCH_KEY_REPEATED);
            assert_eq!(fault.failure().class(), FailureClass::UnknownVariant);
        }
        let mixed = CheckDigests::new(
            true,
            [(WatchKey::Whole, record("a", 1)), (key(1), record("a", 1))],
        );
        assert_eq!(mixed.unwrap_err(), CheckFault::ShapesMixed);
    }

    #[test]
    fn added_changed_and_removed_are_told_by_key_and_output_name() {
        let older = CheckDigests::new(
            true,
            [(key(1), record("new", 5)), (key(2), record("new", 7))],
        )
        .unwrap();
        let newer = CheckDigests::new(
            true,
            [(key(1), record("shipped", 5)), (key(3), record("new", 1))],
        )
        .unwrap();
        let base = Baseline::first(4, &older);
        let compared = compare(&base, &newer);
        assert_eq!(compared.added, vec![key(3)]);
        assert_eq!(
            compared.changed,
            vec![ChangedRecord {
                key: key(1),
                outputs: vec!["status".to_owned()],
                older: 4
            }]
        );
        assert_eq!(
            compared.removed,
            vec![MissingRecord {
                key: key(2),
                older: 4
            }]
        );
        assert!(compared.unseen.is_empty());
        assert_eq!(compared.unchanged, 0);
        assert!(!base.after(5, &newer).records.contains_key(&key(2)));
    }

    #[test]
    fn a_check_cut_short_never_reports_a_removal_and_tells_unseen_once() {
        let full = CheckDigests::new(
            true,
            [(key(1), record("new", 1)), (key(2), record("new", 1))],
        )
        .unwrap();
        let short = CheckDigests::new(false, [(key(1), record("new", 1))]).unwrap();
        let base = Baseline::first(0, &full);
        let first = compare(&base, &short);
        assert!(first.removed.is_empty());
        assert_eq!(
            first.unseen,
            vec![MissingRecord {
                key: key(2),
                older: 0
            }]
        );
        // The record stays known: the next short check says nothing of it,
        // and when it comes back it is not new.
        let base = base.after(1, &short);
        assert!(compare(&base, &short).is_quiet());
        let base = base.after(2, &short);
        let back = compare(&base, &full);
        assert!(back.is_quiet(), "{back:?}");
        assert_eq!(back.unchanged, 2);
        // A complete check after a complete baseline proves the removal.
        let gone = compare(&base, &short_but_complete());
        assert_eq!(
            gone.removed,
            vec![MissingRecord {
                key: key(2),
                older: 0
            }]
        );
    }

    fn short_but_complete() -> CheckDigests {
        CheckDigests::new(true, [(key(1), record("new", 1))]).unwrap()
    }

    #[test]
    fn no_complete_check_ever_means_no_removal_ever() {
        let short = CheckDigests::new(false, [(key(1), record("new", 1))]).unwrap();
        let base = Baseline::first(0, &short);
        let full_without = CheckDigests::new(true, [(key(9), record("new", 1))]).unwrap();
        let compared = compare(&base, &full_without);
        assert!(compared.removed.is_empty());
        assert_eq!(compared.unseen.len(), 1);
        // Once a complete check is in the baseline, removal can be proven.
        let base = base.after(1, &full_without);
        assert!(base.complete);
    }

    #[test]
    fn an_ignored_output_changes_no_digest_and_equal_values_of_two_outputs_differ() {
        let one: Values = [
            ("a", a2m_core::Value::from("x")),
            ("seen_at", a2m_core::Value::from(1_i64)),
        ]
        .into_iter()
        .collect();
        let two: Values = [
            ("a", a2m_core::Value::from("x")),
            ("seen_at", a2m_core::Value::from(2_i64)),
        ]
        .into_iter()
        .collect();
        let ignore: BTreeSet<String> = ["seen_at".to_owned()].into();
        assert_eq!(
            RecordDigests::of(&Plain, &ws(), &one, &ignore),
            RecordDigests::of(&Plain, &ws(), &two, &ignore)
        );
        assert_ne!(
            RecordDigests::of(&Plain, &ws(), &one, &BTreeSet::new()).whole,
            RecordDigests::of(&Plain, &ws(), &two, &BTreeSet::new()).whole
        );
        let swapped: Values = [("b", a2m_core::Value::from("x"))].into_iter().collect();
        let first: Values = [("a", a2m_core::Value::from("x"))].into_iter().collect();
        let (a, b) = (
            RecordDigests::of(&Plain, &ws(), &first, &BTreeSet::new()),
            RecordDigests::of(&Plain, &ws(), &swapped, &BTreeSet::new()),
        );
        assert_ne!(a.outputs.get("a"), b.outputs.get("b"));
        assert_eq!(a.differing(&b), vec!["a".to_owned(), "b".to_owned()]);
    }
}
