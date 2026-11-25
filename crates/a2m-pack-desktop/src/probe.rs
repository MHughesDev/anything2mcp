//! The discoverer `desktop_probe` (spec section 305, "Discovery and
//! drafting"; decision 0999): what the declared application is, and how
//! far the pack can reach it. It sends the one step `snapshot`, which only
//! looks, through the read-only port, and makes one `surface` finding: the
//! program the surface declares, the digest of its file, its version (not
//! read: no accessibility interface gives it), how many windows the
//! application shows, how many controls they hold and how many of those
//! have no name, and a support tier from a closed rule.
//!
//! The tier: `unsupported` when the application shows no window; `partial`
//! when any control has no name, since a control with no name cannot be
//! located (decision 0198), or when a walk stopped at its bound; `full`
//! otherwise. It never holds a value the window shows.

use std::sync::Arc;

use a2m_core::{
    Address, Coverage, EvidenceLevel, FailsAs, Failure, FindingKind, LimitationCode, ProducedBy,
    Source, Step, Subject, UnknownReason, Value, Values,
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

/// The finding kind: one surface, as the probe found it.
pub(crate) const SURFACE_FINDING: &str = "surface";
/// The format of a `surface` finding of this pack.
pub(crate) const FORMAT: &str = "1";

/// The tiers, a closed list.
pub(crate) const TIERS: &[&str] = &["full", "partial", "unsupported"];

/// The roles of the pack's list a person acts on or reads a value from:
/// the controls the probe counts.
pub(crate) const CONTROL_ROLES: &[&str] = &[
    "menu_item",
    "page_tab",
    "entry",
    "text",
    "password_text",
    "spin_button",
    "combo_box",
    "check_box",
    "radio_button",
    "push_button",
    "toggle_button",
    "list_box",
    "table",
];

/// How long the one step may take, in milliseconds.
const STEP_TIMEOUT_MS: u32 = 20_000;

/// The discoverer `desktop_probe`.
pub(crate) struct DesktopProbe {
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

/// What the probe counts of the windows of a snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Counted {
    pub(crate) windows: usize,
    pub(crate) controls: usize,
    pub(crate) unnamed: usize,
    pub(crate) cut: bool,
}

impl Counted {
    pub(crate) fn of(windows: &[Json]) -> Self {
        let mut counted = Self {
            windows: windows.len(),
            ..Self::default()
        };
        for window in windows {
            counted.cut |= window.get("cut").is_some_and(|cut| !cut.is_null());
            for element in window
                .get("elements")
                .and_then(Json::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default()
            {
                let role = element.get("role").and_then(Json::as_str).unwrap_or("");
                if CONTROL_ROLES.contains(&role) {
                    counted.controls += 1;
                    if element.get("unnamed").and_then(Json::as_bool) == Some(true) {
                        counted.unnamed += 1;
                    }
                }
            }
        }
        counted
    }

    /// The tier, by the closed rule.
    pub(crate) fn tier(&self) -> &'static str {
        if self.windows == 0 {
            "unsupported"
        } else if self.unnamed > 0 || self.cut {
            "partial"
        } else {
            "full"
        }
    }
}

impl Discoverer for DesktopProbe {
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
            let counted = Counted::of(windows);
            let surface = &scope.surface.surface;
            let found = json!({
                "format": FORMAT,
                "program": surface.address,
                "digest": shape.get("program_digest").cloned().unwrap_or(Json::Null),
                "version": Json::Null,
                "windows": counted.windows,
                "controls": counted.controls,
                "unnamed_controls": counted.unnamed,
                "tier": counted.tier(),
            })
            .to_string();
            let subject = Address::new(&format!("desktop://{}", surface.name.as_str()), "/")
                .map_or(
                    Subject::Unknown {
                        reason: UnknownReason::NotSupported,
                    },
                    Subject::Address,
                );
            let mut finding = a2m_core::Finding::about(
                subject,
                self.kind.clone(),
                scope.redact.redact(&found),
                Source::LiveSystem,
                ProducedBy {
                    provider: PACK.to_owned(),
                    version: self.version.clone(),
                    component: None,
                },
                self.clock.now(),
            )
            .level(EvidenceLevel::Discovered)
            .limitation(
                LimitationCode::NotExercised,
                "The windows were read and nothing was pressed, expanded, or focused: controls in a part not open now are not counted.",
            )
            .limitation(
                LimitationCode::NotExercised,
                "The program's version is not read: the accessibility interface does not give it. The digest is of the program's file at the declared path, which the running process was proven to run.",
            );
            let mut report = DiscoveryReport {
                coverage: Coverage::Complete,
                ..DiscoveryReport::default()
            };
            if counted.cut {
                finding = finding.coverage(Coverage::Partial).limitation(
                    LimitationCode::Truncated,
                    "A walk of a window stopped at its bound: the counts are of what was read.",
                );
                report.coverage = Coverage::Partial;
            } else {
                finding = finding.coverage(Coverage::Complete);
            }
            out.found(finding.build()?)?;
            Ok(report)
        })
    }
}

/// The describer of the probe's findings: the tier and the counts, which
/// are numbers and words of the pack's own; the program's path is the
/// target's.
pub(crate) struct ProbeDescriber {
    pub(crate) kind: FindingKind,
}

impl Describer for ProbeDescriber {
    fn describe(&self, findings: &[a2m_core::Finding], _form: DescribeForm) -> Description {
        let Some(found) = findings
            .iter()
            .find(|finding| *finding.kind() == self.kind)
            .and_then(|finding| serde_json::from_str::<Json>(finding.found()).ok())
        else {
            return Description {
                text: "The probe found nothing.".to_owned(),
                data: Values::new(),
                untrusted: Vec::new(),
            };
        };
        let count = |key: &str| found.get(key).and_then(Json::as_i64).unwrap_or_default();
        let tier = found
            .get("tier")
            .and_then(Json::as_str)
            .filter(|tier| TIERS.contains(tier))
            .unwrap_or("unsupported");
        let (windows, controls, unnamed) = (
            count("windows"),
            count("controls"),
            count("unnamed_controls"),
        );
        Description {
            text: format!(
                "The application shows {windows} windows with {controls} controls, of which {unnamed} have no name and cannot be located; its support tier is `{tier}`. Its version is not read."
            ),
            data: Values::new()
                .with("windows", windows)
                .with("controls", controls)
                .with("unnamed_controls", unnamed)
                .with("tier", tier),
            untrusted: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tier_follows_the_closed_rule() {
        let window = |elements: Json, cut: Json| json!({"elements": elements, "cut": cut});
        assert_eq!(Counted::of(&[]).tier(), "unsupported");
        let named = window(json!([{"role": "entry", "name": "A"}]), Json::Null);
        assert_eq!(Counted::of(std::slice::from_ref(&named)).tier(), "full");
        let unnamed = window(
            json!([{"role": "entry", "unnamed": true}, {"role": "panel", "unnamed": true}]),
            Json::Null,
        );
        let counted = Counted::of(&[named.clone(), unnamed]);
        assert_eq!((counted.controls, counted.unnamed), (2, 1));
        assert_eq!(counted.tier(), "partial");
        let cut = window(json!([]), json!("nodes"));
        assert_eq!(Counted::of(&[named, cut]).tier(), "partial");
        for tier in ["full", "partial", "unsupported"] {
            assert!(TIERS.contains(&tier));
        }
        for role in CONTROL_ROLES {
            assert!(crate::roles::known(role), "{role}");
        }
    }
}
