//! The discoverer `desktop_snapshot` and its describer (spec section 305,
//! "Discovery and drafting"). It sends one step, `snapshot`, which only
//! looks, and makes one `window` finding for each window of the declared
//! application: its role and title, and the shape of its tree. A finding
//! holds roles, the names that are the window's shape (a control's label,
//! a table's name, a column's heading), and structure; never a value the
//! window shows, and of a control that holds a secret only that it is
//! there.

use std::sync::Arc;

use a2m_core::{
    Address, Coverage, EvidenceLevel, FailsAs, Failure, Finding, FindingKind, LimitationCode,
    ProducedBy, Source, Step, Subject, UnknownReason, Value, Values,
};
use a2m_provider::{
    BoxFuture, DescribeForm, Describer, Description, Discoverer, DiscovererName, DiscoveryReport,
    DiscoveryScope, FindingSink, SandboxPort, StepInput, StepOutcome, SurfaceDescription,
};
use a2m_telemetry::Clock;
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
            for (at, window) in windows.iter().enumerate() {
                let cut = window.get("cut").and_then(Json::as_str);
                let found = json!({
                    "format": FORMAT,
                    "program": surface.address,
                    "window": {
                        "role": window.get("role").cloned().unwrap_or(Json::Null),
                        "title": window.get("title").cloned().unwrap_or(Json::Null),
                    },
                    "cut": cut.is_some(),
                    "elements": window.get("elements").cloned().unwrap_or(Json::Array(Vec::new())),
                })
                .to_string();
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
