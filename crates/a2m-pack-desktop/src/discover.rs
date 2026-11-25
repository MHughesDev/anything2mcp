//! The discoverer `desktop_snapshot` and its describer (spec section 305,
//! "Discovery and drafting"). It sends one step, `snapshot`, which only
//! looks, and makes one `window` finding for each window of the declared
//! application: its role and title, and the shape of its tree. A finding
//! holds roles, the names that are the window's shape (a control's label,
//! a table's name, a column's heading), and structure; never a value the
//! window shows, and of a control that holds a secret only that it is
//! there. Windows of one shape whose titles differ in one word are one
//! finding, its title a pattern with one placeholder (decision 0462).

use std::sync::Arc;

use a2m_core::{
    Address, Coverage, EvidenceLevel, FailsAs, Failure, Finding, FindingKind, LimitationCode,
    ProducedBy, Source, Step, Subject, UnknownReason, Value, Values,
};
use a2m_logs::Clock;
use a2m_provider::{
    BoxFuture, DescribeForm, Describer, Description, Discoverer, DiscovererName, DiscoveryReport,
    DiscoveryScope, FindingSink, SandboxPort, StepInput, StepOutcome, SurfaceDescription,
};
use serde_json::{Value as Json, json};

use crate::codes;
use crate::driver::SNAPSHOT_FORMAT;
use crate::lower::SNAPSHOT;
use crate::screens::{DESKTOP_SURFACE, PACK};

/// The finding kind: one window's shape.
pub(crate) const WINDOW_FINDING: &str = "window";
/// The format of a `window` finding.
pub(crate) const FORMAT: &str = "1";

/// How long the one step of a discovery may take, in milliseconds.
const STEP_TIMEOUT_MS: u32 = 20_000;

/// The discoverer `desktop_snapshot`.
pub(crate) struct DesktopSnapshot {
    pub(crate) name: DiscovererName,
    pub(crate) kind: FindingKind,
    pub(crate) version: String,
    pub(crate) clock: Arc<dyn Clock>,
}

fn invalid(why: &str) -> Failure {
    codes::failure(
        &codes::row::STEP_NOT_USABLE,
        format!("The desktop driver's snapshot is not in the form this discoverer reads: {why}."),
        "Run the discovery again with the version of the desktop pack that is enabled.",
    )
}

impl Discoverer for DesktopSnapshot {
    fn name(&self) -> DiscovererName {
        self.name.clone()
    }

    fn suits(&self, surface: &SurfaceDescription) -> bool {
        surface.kind == DESKTOP_SURFACE
    }

    fn discover<'a>(
        &'a self,
        port: &'a dyn SandboxPort,
        scope: &'a DiscoveryScope<'a>,
        out: &'a mut dyn FindingSink,
    ) -> BoxFuture<'a, Result<DiscoveryReport, Failure>> {
        Box::pin(async move {
            let step = Step {
                kind: SNAPSHOT,
                refers_to: None,
                args: Vec::new(),
                timeout_ms: STEP_TIMEOUT_MS,
                fails_as: FailsAs::from(&codes::row::UNEXPECTED_PAGE),
                commits: false,
                lookup: false,
                idempotent: true,
                when: None,
                payload: a2m_core::PackPayload::none(),
            };
            let none = Values::new();
            let input = StepInput::new(&none, scope.deadline);
            let outcome = port
                .act(&step, &input)
                .await
                .map_err(|failed| failed.failure)?;
            let (StepOutcome::Done(observed) | StepOutcome::Absent(observed)) = outcome;
            let shape = observed
                .values
                .get("shape")
                .and_then(Value::as_text)
                .ok_or_else(|| invalid("it holds no shape"))?;
            let shape: Json = serde_json::from_str(shape).map_err(|_| invalid("not JSON"))?;
            if shape.get("format").and_then(Json::as_str) != Some(SNAPSHOT_FORMAT) {
                return Err(invalid("another format"));
            }
            let windows = shape
                .get("windows")
                .and_then(Json::as_array)
                .ok_or_else(|| invalid("no windows"))?;
            let surface = &scope.surface.surface;
            let now = self.clock.now();
            let mut report = DiscoveryReport {
                coverage: Coverage::Complete,
                ..DiscoveryReport::default()
            };
            let mut bytes = 0_u64;
            // Windows of one shape whose titles differ in one word are one
            // screen, its title a pattern: one finding for them all.
            let groups = matched(windows);
            for (at, window) in windows.iter().enumerate() {
                let group = groups.iter().find(|group| group.windows.contains(&at));
                if group.is_some_and(|group| group.windows.first() != Some(&at)) {
                    continue;
                }
                let cut = window.get("cut").and_then(Json::as_str);
                let mut found = json!({
                    "format": FORMAT,
                    "program": surface.address,
                    "window": {
                        "role": window.get("role").cloned().unwrap_or(Json::Null),
                        "title": window.get("title").cloned().unwrap_or(Json::Null),
                    },
                    "cut": cut.is_some(),
                    "elements": window.get("elements").cloned().unwrap_or(Json::Array(Vec::new())),
                });
                if let Some(group) = group {
                    let title = window
                        .get("title")
                        .and_then(Json::as_str)
                        .unwrap_or_default();
                    found["window"]["title"] = json!(group.title);
                    found["elements"] = shape_of(window, title, &group.title);
                    found["windows"] = json!(group.windows.len());
                }
                let found = found.to_string();
                bytes += u64::try_from(found.len()).unwrap_or(u64::MAX);
                if bytes > scope.bounds.bytes {
                    report.coverage = Coverage::Partial;
                    report.bounds_hit.push("bytes".to_owned());
                    report.dropped += u64::try_from(windows.len() - at).unwrap_or(u64::MAX);
                    break;
                }
                let subject = Address::new(
                    &format!("desktop://{}", surface.name.as_str()),
                    &format!("/window/{at}"),
                )
                .map_or(
                    Subject::Unknown {
                        reason: UnknownReason::NotSupported,
                    },
                    Subject::Address,
                );
                let mut finding = Finding::about(
                    subject,
                    self.kind.clone(),
                    scope.redact.redact(&found),
                    Source::LiveSystem,
                    ProducedBy {
                        provider: PACK.to_owned(),
                        version: self.version.clone(),
                        component: None,
                    },
                    now,
                )
                .level(EvidenceLevel::Discovered)
                .limitation(
                    LimitationCode::NotExercised,
                    "The tree was read and nothing was pressed, expanded, or focused: what a collapsed or unopened part of the window holds is not in it.",
                );
                if let Some(bound) = cut {
                    finding = finding.coverage(Coverage::Partial).limitation(
                        LimitationCode::Truncated,
                        format!("The walk of the tree stopped at its bound on {bound}."),
                    );
                    report.coverage = Coverage::Partial;
                    report.bounds_hit.push(bound.to_owned());
                } else {
                    finding = finding.coverage(Coverage::Complete);
                }
                out.found(finding.build()?)?;
            }
            Ok(report)
        })
    }
}

/// Windows of one shape whose titles differ in one word.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Matched {
    /// The title, with the word that differs as one placeholder.
    pub(crate) title: String,
    /// The windows, by their place in the snapshot, in order.
    pub(crate) windows: Vec<usize>,
}

/// Whether one word of a title may stand for a record's key: letters,
/// digits, and `-`, `.`, `_`, as a path segment the web probe takes for
/// one.
fn plain_word(word: &str) -> bool {
    !word.is_empty()
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_'))
}

/// The name of the placeholder at `at`: the plain word before it, as one
/// record of it, with `_id`; `key` when there is none. The web probe's
/// rule for a path's segment.
fn placeholder(words: &[&str], at: usize) -> String {
    let word: String = at
        .checked_sub(1)
        .and_then(|before| words.get(before))
        .map(|word| {
            word.chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() {
                        c.to_ascii_lowercase()
                    } else {
                        '_'
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    let word = word.trim_matches('_');
    let word = match word.strip_suffix('s') {
        Some(one) if !one.is_empty() && !one.ends_with('s') => one,
        _ => word,
    };
    let fits = word.starts_with(|c: char| c.is_ascii_lowercase())
        && !word.contains("__")
        && word.len() <= 40;
    if fits {
        format!("{word}_id")
    } else {
        "key".to_owned()
    }
}

/// A window's elements, the window's own element, which is named by its
/// title, named `pattern` instead.
fn shape_of(window: &Json, title: &str, pattern: &str) -> Json {
    let mut elements = window
        .get("elements")
        .cloned()
        .unwrap_or(Json::Array(Vec::new()));
    if let Some(root) = elements.get_mut(0)
        && root.get("depth").and_then(Json::as_u64) == Some(0)
        && root.get("name").and_then(Json::as_str) == Some(title)
    {
        root["name"] = json!(pattern);
    }
    elements
}

/// Screen matching over the windows of one snapshot, as the web probe
/// matches pages (decision 0462): windows of one role and one shape, each
/// walked whole, whose titles have the same number of words and differ in
/// exactly one, which is a plain word in each and not the same in all. A
/// window two such groups would hold is in neither; a group of fewer than
/// two windows is none, so one title never becomes a pattern. A title
/// that holds a brace, or a word with no letter or digit beside the
/// differing one, is left as it is.
pub(crate) fn matched(windows: &[Json]) -> Vec<Matched> {
    use std::collections::BTreeMap;
    let mut groups: BTreeMap<(String, usize, Vec<String>, String), Vec<usize>> = BTreeMap::new();
    for (at, window) in windows.iter().enumerate() {
        let text = |key: &str| window.get(key).and_then(Json::as_str);
        let (Some(role), Some(title)) = (text("role"), text("title")) else {
            continue;
        };
        if !window.get("cut").is_some_and(Json::is_null)
            || title.contains(['{', '}'])
            || title.chars().any(char::is_control)
        {
            continue;
        }
        let shape = shape_of(window, title, "").to_string();
        let words: Vec<&str> = title.split(' ').collect();
        for hole in 0..words.len() {
            if !plain_word(words[hole]) {
                continue;
            }
            let mut others: Vec<String> = words.iter().map(|word| (*word).to_owned()).collect();
            others[hole] = String::new();
            if others.iter().all(|word| word.trim().is_empty()) {
                continue;
            }
            groups
                .entry((role.to_owned(), hole, others, shape.clone()))
                .or_default()
                .push(at);
        }
    }
    let title_of = |at: usize| {
        windows[at]
            .get("title")
            .and_then(Json::as_str)
            .unwrap_or_default()
    };
    groups.retain(|(_, hole, _, _), members| {
        let mut seen: Vec<&str> = members
            .iter()
            .map(|at| title_of(*at).split(' ').nth(*hole).unwrap_or_default())
            .collect();
        seen.sort_unstable();
        seen.dedup();
        seen.len() >= 2
    });
    let mut held: BTreeMap<usize, u32> = BTreeMap::new();
    for members in groups.values() {
        for at in members {
            *held.entry(*at).or_default() += 1;
        }
    }
    let mut out: Vec<Matched> = groups
        .into_iter()
        .filter_map(|((_, hole, _, _), members)| {
            if members.iter().any(|at| held.get(at) != Some(&1)) {
                return None;
            }
            let words: Vec<&str> = title_of(members[0]).split(' ').collect();
            let name = placeholder(&words, hole);
            let title: Vec<String> = words
                .iter()
                .enumerate()
                .map(|(at, word)| {
                    if at == hole {
                        format!("{{{name}}}")
                    } else {
                        (*word).to_owned()
                    }
                })
                .collect();
            Some(Matched {
                title: title.join(" "),
                windows: members,
            })
        })
        .collect();
    out.sort_by_key(|group| group.windows.first().copied());
    out
}

/// Puts the discoverer's findings so that an agent can read them.
pub(crate) struct DesktopDescriber {
    pub(crate) kind: FindingKind,
}

impl Describer for DesktopDescriber {
    fn describe(&self, findings: &[Finding], form: DescribeForm) -> Description {
        let (mut windows, mut elements, mut unnamed, mut secret) = (0_i64, 0_i64, 0_i64, 0_i64);
        let mut untrusted = Vec::new();
        for finding in findings
            .iter()
            .filter(|finding| *finding.kind() == self.kind)
        {
            let found: Json = serde_json::from_str(finding.found()).unwrap_or(Json::Null);
            windows += 1;
            for one in found
                .get("elements")
                .and_then(Json::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default()
            {
                elements += 1;
                if one.get("unnamed").and_then(Json::as_bool) == Some(true) {
                    unnamed += 1;
                }
                if one.get("secret").and_then(Json::as_bool) == Some(true) {
                    secret += 1;
                }
            }
            if form == DescribeForm::Full
                && let Some(title) = found
                    .get("window")
                    .and_then(|window| window.get("title"))
                    .and_then(Json::as_str)
            {
                untrusted.push(title.to_owned());
            }
        }
        Description {
            text: format!(
                "The application shows {windows} windows, with {elements} elements in all; {unnamed} have no accessible name, and {secret} hold a secret, whose value was not read."
            ),
            data: Values::new()
                .with("windows", windows)
                .with("elements", elements)
                .with("unnamed", unnamed)
                .with("secret", secret),
            untrusted,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_are_one_screen_only_when_one_word_of_their_titles_differs_and_nothing_else() {
        let window = |title: &str, label: &str| {
            json!({"role": "frame", "title": title, "cut": null, "elements": [
                {"depth": 0, "role": "frame", "name": title},
                {"depth": 1, "role": "label", "name": label},
            ]})
        };
        let windows = [
            window("Order 1001", "Customer"),
            window("Order 1002", "Customer"),
            window("Order 1003", "City"),
            window("Example order", "Customer"),
            window("Notes", "Customer"),
        ];
        assert_eq!(
            matched(&windows),
            [Matched {
                title: "Order {order_id}".to_owned(),
                windows: vec![0, 1],
            }]
        );
        // One title is never a pattern, nor two windows of one title.
        assert!(matched(&windows[..1]).is_empty());
        assert!(
            matched(&[
                window("Order 1001", "Customer"),
                window("Order 1001", "Customer")
            ])
            .is_empty()
        );
        // Two words that differ are not one screen.
        assert!(
            matched(&[
                window("Order 1 open", "Customer"),
                window("Order 2 held", "Customer")
            ])
            .is_empty()
        );
        // A window walked only in part is not matched.
        let mut cut = window("Order 1004", "Customer");
        cut["cut"] = json!("nodes");
        assert!(matched(&[window("Order 1001", "Customer"), cut]).is_empty());
        // A window two groups would hold is in neither.
        let groups = matched(&[
            window("Order 1 of 2", "Customer"),
            window("Order 1 of 3", "Customer"),
            window("Order 2 of 2", "Customer"),
        ]);
        assert!(groups.is_empty(), "{groups:?}");
        // With no plain word before it, the placeholder is `key`.
        assert_eq!(
            matched(&[
                window("1001 - Orders", "Customer"),
                window("1002 - Orders", "Customer")
            ])[0]
                .title,
            "{key} - Orders"
        );
        // A title of one word that differs would match any window.
        assert!(matched(&[window("1001", "Customer"), window("1002", "Customer")]).is_empty());
    }
}
