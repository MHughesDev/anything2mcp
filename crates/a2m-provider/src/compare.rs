//! Described against observed (spec section 218, decision 0486): what a
//! system's own description says, set against what an observation of it
//! saw.
//!
//! The comparison is a pure function over findings. It makes no finding
//! and raises no level: what it says of an endpoint is at most the level
//! an observation already earned. The side of a finding is its source;
//! its key is given by the pack that owns its kind, through a
//! [`KeyRule`], because the form of what a finding holds is that pack's.

use std::collections::{BTreeMap, BTreeSet};

use a2m_core::{ComparisonResult, Coverage, Digest, EvidenceLevel, Finding, NotCompared, Source};

/// One segment of a key's path.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum KeySegment {
    /// A segment as it is written.
    Literal(String),
    /// A segment that stands for a value, whatever its name.
    Placeholder,
}

/// What a finding is compared by: the place it is at, the verb, and the
/// path. For the request access kind the place is an origin, the verb a
/// method, and the path an address's path without its query.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ComparisonKey {
    place: String,
    verb: String,
    path: Vec<KeySegment>,
}

impl ComparisonKey {
    /// A key from its three parts. The place is compared as given, in
    /// lower case; the verb in upper case. The path is cut at `/`, empty
    /// segments are dropped, a query (from `?`) is left out, and a segment
    /// that is wholly `{…}` is a placeholder.
    ///
    /// Gives `None` when the place or the verb is empty, or the path does
    /// not start with `/`.
    #[must_use]
    pub fn parse(place: &str, verb: &str, path: &str) -> Option<Self> {
        if place.is_empty() || verb.is_empty() || !path.starts_with('/') {
            return None;
        }
        let path = path.split_once('?').map_or(path, |(path, _)| path);
        let segments = path
            .split('/')
            .filter(|segment| !segment.is_empty())
            .map(|segment| {
                if segment.len() >= 2 && segment.starts_with('{') && segment.ends_with('}') {
                    KeySegment::Placeholder
                } else {
                    KeySegment::Literal(segment.to_owned())
                }
            })
            .collect();
        Some(Self {
            place: place.to_ascii_lowercase(),
            verb: verb.to_ascii_uppercase(),
            path: segments,
        })
    }

    /// The place.
    #[must_use]
    pub fn place(&self) -> &str {
        &self.place
    }

    /// The verb.
    #[must_use]
    pub fn verb(&self) -> &str {
        &self.verb
    }

    /// The path's segments.
    #[must_use]
    pub fn segments(&self) -> &[KeySegment] {
        &self.path
    }

    /// The path as text, each placeholder written `{}`.
    #[must_use]
    pub fn path(&self) -> String {
        if self.path.is_empty() {
            return "/".to_owned();
        }
        self.path
            .iter()
            .map(|segment| match segment {
                KeySegment::Literal(text) => format!("/{text}"),
                KeySegment::Placeholder => "/{}".to_owned(),
            })
            .collect()
    }

    /// How many segments are literal.
    fn literals(&self) -> usize {
        self.path
            .iter()
            .filter(|segment| matches!(segment, KeySegment::Literal(_)))
            .count()
    }

    /// Whether this observed key fits the described key `described`.
    fn fits(&self, described: &Self) -> bool {
        self.place == described.place
            && self.verb == described.verb
            && self.path.len() == described.path.len()
            && self
                .path
                .iter()
                .zip(&described.path)
                .all(|(seen, said)| match (seen, said) {
                    (_, KeySegment::Placeholder) => true,
                    (KeySegment::Literal(seen), KeySegment::Literal(said)) => seen == said,
                    (KeySegment::Placeholder, KeySegment::Literal(_)) => false,
                })
    }
}

/// What a pack implements to give the key of a finding of a kind it owns,
/// read from that finding's `found` in the form the pack defines. Pure: no
/// input or output. A public contract.
pub trait KeyRule: Send + Sync {
    /// The finding's key; `None` when the finding holds none that can be
    /// read, which the comparison reports as `unknown`, `no_key`.
    fn key(&self, finding: &Finding) -> Option<ComparisonKey>;
}

/// What the comparison says of one key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Compared {
    /// The result.
    pub result: ComparisonResult,
    /// The key; `None` for a finding its rule gave no key for.
    pub key: Option<ComparisonKey>,
    /// The level what is said rests on: the strongest observed level for
    /// `matched` and `observed_only`, `inferred` otherwise.
    pub level: EvidenceLevel,
    /// The described findings it rests on, in order of identifier.
    pub described: Vec<Digest>,
    /// The observed findings it rests on, in order of identifier.
    pub observed: Vec<Digest>,
    /// For `unknown`, why.
    pub reason: Option<NotCompared>,
    /// How much the observation of the place covered: for `matched` and
    /// `described_only`, the coverage the observation stated.
    pub coverage: Coverage,
}

/// What the comparison found, entry by entry, in a fixed order: by
/// place, path, verb, and result.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Comparison {
    /// One entry for each described key, and one for each observed key
    /// that no described key took.
    pub entries: Vec<Compared>,
}

impl Comparison {
    /// How many entries have the result.
    #[must_use]
    pub fn count(&self, result: ComparisonResult) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.result == result)
            .count()
    }
}

/// Which side a finding is on, by its source: `None` for a source that is
/// on neither.
fn side(source: Source) -> Option<bool> {
    match source {
        Source::LiveSystem => Some(false),
        Source::PublishedDescription | Source::Artifact => Some(true),
        Source::ThirdParty | Source::Reasoning => None,
    }
}

fn stronger(a: Coverage, b: Coverage) -> Coverage {
    match (a, b) {
        (Coverage::Complete, _) | (_, Coverage::Complete) => Coverage::Complete,
        (Coverage::Partial, _) | (_, Coverage::Partial) => Coverage::Partial,
        _ => Coverage::Unknown,
    }
}

/// Compares the findings given, each with the key its kind's rule gave
/// (`None` when it gave none). A finding whose source is on neither side
/// is left out.
#[must_use]
pub fn compare_findings<'a>(
    keyed: impl IntoIterator<Item = (Option<ComparisonKey>, &'a Finding)>,
) -> Comparison {
    let mut described: BTreeMap<ComparisonKey, BTreeSet<Digest>> = BTreeMap::new();
    let mut observed: BTreeMap<ComparisonKey, (BTreeSet<Digest>, EvidenceLevel)> = BTreeMap::new();
    let mut covered: BTreeMap<String, Coverage> = BTreeMap::new();
    let mut entries = Vec::new();
    for (key, finding) in keyed {
        let Some(is_described) = side(finding.source()) else {
            continue;
        };
        let Some(key) = key else {
            let ids = vec![finding.id()];
            entries.push(Compared {
                result: ComparisonResult::Unknown,
                key: None,
                level: EvidenceLevel::Inferred,
                described: if is_described {
                    ids.clone()
                } else {
                    Vec::new()
                },
                observed: if is_described { Vec::new() } else { ids },
                reason: Some(NotCompared::NoKey),
                coverage: Coverage::Unknown,
            });
            continue;
        };
        if is_described {
            described.entry(key).or_default().insert(finding.id());
        } else {
            let place = covered
                .entry(key.place.clone())
                .or_insert(Coverage::Unknown);
            *place = stronger(*place, finding.coverage());
            let seen = observed
                .entry(key)
                .or_insert_with(|| (BTreeSet::new(), EvidenceLevel::Inferred));
            seen.0.insert(finding.id());
            seen.1 = seen.1.max(finding.level().min(EvidenceLevel::Discovered));
        }
    }
    // Each observed key belongs to the described keys it fits that have the
    // most literal segments.
    let mut taken: BTreeMap<&ComparisonKey, Vec<&ComparisonKey>> = BTreeMap::new();
    let mut untaken: Vec<&ComparisonKey> = Vec::new();
    for seen in observed.keys() {
        let fitting: Vec<&ComparisonKey> =
            described.keys().filter(|said| seen.fits(said)).collect();
        let most = fitting.iter().map(|said| said.literals()).max();
        let best: Vec<&ComparisonKey> = fitting
            .into_iter()
            .filter(|said| Some(said.literals()) == most)
            .collect();
        if best.is_empty() {
            untaken.push(seen);
        }
        for said in best {
            taken.entry(said).or_default().push(seen);
        }
    }
    for (said, ids) in &described {
        let coverage = covered
            .get(&said.place)
            .copied()
            .unwrap_or(Coverage::Unknown);
        let entry = match taken.get(said) {
            Some(seen) => Compared {
                result: ComparisonResult::Matched,
                key: Some(said.clone()),
                level: seen
                    .iter()
                    .map(|key| observed[*key].1)
                    .max()
                    .unwrap_or(EvidenceLevel::Inferred),
                described: ids.iter().copied().collect(),
                observed: seen
                    .iter()
                    .flat_map(|key| observed[*key].0.iter().copied())
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect(),
                reason: None,
                coverage,
            },
            None => {
                let reason = match covered.get(&said.place) {
                    None => Some(NotCompared::NotObserved),
                    Some(Coverage::Unknown) => Some(NotCompared::CoverageUnknown),
                    Some(_) => None,
                };
                Compared {
                    result: if reason.is_some() {
                        ComparisonResult::Unknown
                    } else {
                        ComparisonResult::DescribedOnly
                    },
                    key: Some(said.clone()),
                    level: EvidenceLevel::Inferred,
                    described: ids.iter().copied().collect(),
                    observed: Vec::new(),
                    reason,
                    coverage,
                }
            }
        };
        entries.push(entry);
    }
    for seen in untaken {
        let (ids, level) = &observed[seen];
        entries.push(Compared {
            result: ComparisonResult::ObservedOnly,
            key: Some(seen.clone()),
            level: *level,
            described: Vec::new(),
            observed: ids.iter().copied().collect(),
            reason: None,
            coverage: covered
                .get(&seen.place)
                .copied()
                .unwrap_or(Coverage::Unknown),
        });
    }
    entries.sort_by(|a, b| {
        let order = |entry: &Compared| {
            (
                entry
                    .key
                    .as_ref()
                    .map(|key| (key.place.clone(), key.path(), key.verb.clone())),
                entry.result.as_str(),
                entry.described.first().or(entry.observed.first()).copied(),
            )
        };
        order(a).cmp(&order(b))
    });
    Comparison { entries }
}
