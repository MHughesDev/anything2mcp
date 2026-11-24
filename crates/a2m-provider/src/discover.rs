//! Discovery: what a pack implements to observe a system, and the only
//! route it is given to reach one.
//!
//! A discoverer cannot write. It is handed a [`SandboxPort`], a scope, and
//! a sink, and nothing else. The port's one method takes no authority, so
//! there is nowhere to pass an intent token; a [`DriverHandle`] is given
//! only by the registry, which no pack receives; and an intent token has no
//! public constructor.
//!
//! A discoverer a person drives is called by [`Discoverer::watch`] instead,
//! and is handed no port at all: it reads a recording of its pack's
//! traffic capture, and returns when the kernel sets its [`CaptureEnd`]
//! (decision 0395). Its manifest says which of the two it is, and the
//! [`DiscovererHandle`] calls it by that method alone.
//!
//! [`DriverHandle`]: crate::DriverHandle

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use a2m_core::{
    Archetype, Coverage, EvidenceLevel, Failure, Finding, FindingKind, Redact, Step, SurfaceKind,
    Values,
};

use crate::BoxFuture;
use crate::codes;
use crate::driver::{Deadline, StepFailure, StepInput, StepOutcome, SurfaceRef};
use crate::manifest::{Bounds, DiscovererDecl, DrivenBy};
use crate::name::{DiscovererName, ProviderName};
use crate::path::CheckedPaths;

/// What is known of a surface before anything is observed: enough to ask
/// which discoverers apply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceDescription {
    /// The surface kind.
    pub kind: SurfaceKind,
    /// The kind's archetype.
    pub archetype: Archetype,
    /// Where it is reached, in the form its kind defines.
    pub address: String,
    /// What else is known, by name.
    pub facts: Values,
}

/// What one discovery job covers.
pub struct DiscoveryScope<'a> {
    /// The surface observed.
    pub surface: &'a SurfaceRef,
    /// How far the discoverer may go, from its manifest or tighter.
    pub bounds: Bounds,
    /// When it must be done.
    pub deadline: Deadline,
    /// The files it may read, already checked.
    pub paths: &'a CheckedPaths,
    /// The redactor. Text a finding quotes passes it first.
    pub redact: &'a dyn Redact,
    /// What the developer asked to look at, by name; empty for everything.
    pub focus: &'a Values,
}

impl fmt::Debug for DiscoveryScope<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DiscoveryScope")
            .field("surface", &self.surface.surface.name)
            .field("bounds", &self.bounds)
            .finish_non_exhaustive()
    }
}

/// Where findings go as they are made.
pub trait FindingSink: Send {
    /// Takes one finding.
    ///
    /// # Errors
    ///
    /// `unknown_variant`/`finding_not_declared` for a kind or a level the
    /// discoverer's manifest does not allow; the store's failures.
    fn found(&mut self, finding: Finding) -> Result<(), Failure>;
}

/// How a discovery ended.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DiscoveryReport {
    /// How much of the scope was looked at.
    pub coverage: Coverage,
    /// The bounds it stopped at, by name: `seconds`, `bytes`, `depth`.
    pub bounds_hit: Vec<String>,
    /// How many things it saw and made no finding of.
    pub dropped: u64,
}

/// The engine's sandbox, as a discoverer sees it: one step at a time, and a
/// write cannot pass through it.
pub trait SandboxPort: Send + Sync {
    /// Carries out one step that reads. A step marked `commits` is refused
    /// unsent.
    fn act<'a>(
        &'a self,
        step: &'a Step,
        input: &'a StepInput<'a>,
    ) -> BoxFuture<'a, Result<StepOutcome, StepFailure>>;
}

/// The end of a capture, as a discoverer a person drives sees it: set once,
/// by the kernel, when the person stops, when the capture's window closes,
/// or at the discoverer's bound. A discoverer waits on it or asks it; it
/// cannot set it. It is made only with its [`CaptureStop`], which comes
/// from the registry, and no pack is given a registry.
#[derive(Clone)]
pub struct CaptureEnd {
    set: Arc<tokio::sync::watch::Sender<bool>>,
}

impl CaptureEnd {
    /// Whether the capture has ended.
    #[must_use]
    pub fn is_set(&self) -> bool {
        *self.set.borrow()
    }

    /// Returns when the capture has ended; at once if it has.
    pub async fn ended(&self) {
        let mut seen = self.set.subscribe();
        // The sender is held by `self`, so the wait ends only when the
        // value is set.
        let _ = seen.wait_for(|set| *set).await;
    }

    /// Sets it. Only the contract does this, for its own bound; the
    /// kernel does it through [`CaptureStop::stop`].
    fn set(&self) {
        self.set.send_replace(true);
    }
}

impl fmt::Debug for CaptureEnd {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CaptureEnd")
            .field("set", &self.is_set())
            .finish()
    }
}

/// The kernel's half of a [`CaptureEnd`]: what sets it. Given only by
/// [`crate::Registry::capture_stop`]. Clones stop the same capture.
#[derive(Clone, Debug)]
pub struct CaptureStop {
    end: CaptureEnd,
}

impl CaptureStop {
    /// A capture's stop, not yet set.
    pub(crate) fn new() -> Self {
        let (set, _) = tokio::sync::watch::channel(false);
        Self {
            end: CaptureEnd { set: Arc::new(set) },
        }
    }

    /// Ends the capture. Setting it again changes nothing.
    pub fn stop(&self) {
        self.end.set();
    }

    /// The end the discoverer is handed.
    #[must_use]
    pub fn end(&self) -> &CaptureEnd {
        &self.end
    }
}

/// How long a discoverer a person drives is given to return once its
/// capture has ended, before the handle gives up on it as `timeout`. It
/// has only to hand over what it already holds.
pub const CAPTURE_END_GRACE: Duration = Duration::from_secs(5);

/// What a pack implements to observe one kind of system. A public contract.
pub trait Discoverer: Send + Sync {
    /// The discoverer's name, as the manifest declares it.
    fn name(&self) -> DiscovererName;

    /// From the surface description alone: does it apply here.
    fn suits(&self, surface: &SurfaceDescription) -> bool;

    /// Observes within the scope's bounds, reaching the system only through
    /// `port`. Findings go to `out` as they are made.
    fn discover<'a>(
        &'a self,
        port: &'a dyn SandboxPort,
        scope: &'a DiscoveryScope<'a>,
        out: &'a mut dyn FindingSink,
    ) -> BoxFuture<'a, Result<DiscoveryReport, Failure>>;

    /// Watches what a person does, for a discoverer whose manifest says
    /// `driven_by = "person"`: reads the recording of its pack's traffic
    /// capture until `end` is set, and returns. It is handed no port, so it
    /// reaches the system through nothing of the kernel's. Findings go to
    /// `out` as they are made.
    ///
    /// # Errors
    ///
    /// The default, for every discoverer the engine drives:
    /// `unknown_variant`/`discoverer_cannot_watch`.
    fn watch<'a>(
        &'a self,
        scope: &'a DiscoveryScope<'a>,
        end: &'a CaptureEnd,
        out: &'a mut dyn FindingSink,
    ) -> BoxFuture<'a, Result<DiscoveryReport, Failure>> {
        let _ = (scope, end, out);
        Box::pin(async {
            Err(codes::failure(
                &codes::row::DISCOVERER_CANNOT_WATCH,
                "This discoverer does not watch a capture.",
                "Report this to the provider's author; a discoverer driven by a person implements `watch`.",
            ))
        })
    }
}

/// How much a description says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DescribeForm {
    /// A few lines: what was found and how many.
    Summary,
    /// Everything the findings hold.
    Full,
}

/// Findings, put so that an agent can read them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Description {
    /// The description as text the provider wrote. It quotes nothing a
    /// system said.
    pub text: String,
    /// The same as structure, by name.
    pub data: Values,
    /// Text a system supplied, kept apart and marked, never mixed into
    /// `text`.
    pub untrusted: Vec<String>,
}

/// Turns findings into something an agent can read. Pure: no input or
/// output.
pub trait Describer: Send + Sync {
    /// Describes the findings.
    fn describe(&self, findings: &[Finding], form: DescribeForm) -> Description;
}

/// A registered discoverer with its describer and what its manifest allows.
#[derive(Clone)]
pub struct DiscovererHandle {
    pub(crate) provider: ProviderName,
    pub(crate) decl: DiscovererDecl,
    pub(crate) discoverer: Arc<dyn Discoverer>,
    pub(crate) describer: Arc<dyn Describer>,
}

impl fmt::Debug for DiscovererHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DiscovererHandle")
            .field("provider", &self.provider)
            .field("name", &self.decl.name)
            .finish_non_exhaustive()
    }
}

/// Passes on only the findings the manifest allows.
struct DeclaredOnly<'a> {
    kinds: &'a [FindingKind],
    max_level: EvidenceLevel,
    out: &'a mut dyn FindingSink,
}

impl FindingSink for DeclaredOnly<'_> {
    fn found(&mut self, finding: Finding) -> Result<(), Failure> {
        if !self.kinds.contains(finding.kind()) || finding.level() > self.max_level {
            return Err(codes::failure(
                &codes::row::FINDING_NOT_DECLARED,
                format!(
                    "A discoverer made a `{}` finding at the level `{}`, which its manifest does not allow.",
                    finding.kind().as_str(),
                    finding.level()
                ),
                "Report this to the provider's author; a discoverer returns only the kinds and levels it declares.",
            ));
        }
        self.out.found(finding)
    }
}

impl DiscovererHandle {
    /// The provider that brought it.
    #[must_use]
    pub fn provider(&self) -> &ProviderName {
        &self.provider
    }

    /// What its manifest declares: its name, kinds, level, and bounds.
    #[must_use]
    pub fn declared(&self) -> &DiscovererDecl {
        &self.decl
    }

    /// Whether it applies to the surface.
    #[must_use]
    pub fn suits(&self, surface: &SurfaceDescription) -> bool {
        surface.archetype == self.decl.archetype
            && (self.decl.surface_kinds.is_empty()
                || self.decl.surface_kinds.contains(&surface.kind))
            && self.discoverer.suits(surface)
    }

    /// Runs the discoverer. Each finding is checked against the manifest's
    /// kinds and level as it arrives.
    ///
    /// # Errors
    ///
    /// `policy`/`effect_not_declared`, before the discoverer is asked, when
    /// its manifest says a person drives it; the discoverer's failure;
    /// `unknown_variant`/`finding_not_declared` when it makes a finding its
    /// manifest does not allow.
    pub async fn discover(
        &self,
        port: &dyn SandboxPort,
        scope: &DiscoveryScope<'_>,
        out: &mut dyn FindingSink,
    ) -> Result<DiscoveryReport, Failure> {
        self.driven(DrivenBy::Engine)?;
        let mut checked = DeclaredOnly {
            kinds: &self.decl.kinds,
            max_level: self.decl.max_level,
            out,
        };
        self.discoverer.discover(port, scope, &mut checked).await
    }

    /// Runs a discoverer a person drives over a capture, until `end` is set
    /// or its bound passes, whichever is first. The bound is the earlier of
    /// the scope's deadline and its `seconds`, held to the manifest's: at
    /// the bound the handle sets the end itself and the report names
    /// `seconds`. Once the end is set the discoverer is given
    /// [`CAPTURE_END_GRACE`] to return. Each finding is checked against the
    /// manifest's kinds and level as it arrives, as in `discover`.
    ///
    /// # Errors
    ///
    /// `policy`/`effect_not_declared`, before the discoverer is asked, when
    /// its manifest does not say a person drives it; the discoverer's
    /// failure; `unknown_variant`/`finding_not_declared` when it makes a
    /// finding its manifest does not allow; `transient`/`timeout` when it
    /// has not returned within the grace after the end.
    pub async fn watch(
        &self,
        scope: &DiscoveryScope<'_>,
        end: &CaptureEnd,
        out: &mut dyn FindingSink,
    ) -> Result<DiscoveryReport, Failure> {
        self.driven(DrivenBy::Person)?;
        let seconds = scope.bounds.seconds.min(self.decl.bounds.seconds);
        let bound = Deadline::after(Duration::from_secs(u64::from(seconds)))
            .at()
            .min(scope.deadline.at());
        let mut checked = DeclaredOnly {
            kinds: &self.decl.kinds,
            max_level: self.decl.max_level,
            out,
        };
        let mut watching = self.discoverer.watch(scope, end, &mut checked);
        let mut bound_hit = false;
        tokio::select! {
            done = &mut watching => return done,
            () = end.ended() => {}
            () = tokio::time::sleep_until(bound) => {
                bound_hit = !end.is_set();
                end.set();
            }
        }
        match tokio::time::timeout(CAPTURE_END_GRACE, watching).await {
            Ok(Ok(mut report)) => {
                if bound_hit && !report.bounds_hit.iter().any(|hit| hit == "seconds") {
                    report.bounds_hit.push("seconds".to_owned());
                }
                Ok(report)
            }
            Ok(Err(failure)) => Err(failure),
            Err(_) => Err(codes::failure(
                &codes::row::TIMEOUT,
                format!(
                    "The discoverer `{}` did not return within {} seconds of its capture's end.",
                    self.decl.name,
                    CAPTURE_END_GRACE.as_secs()
                ),
                "Report this to the provider's author; a discoverer driven by a person returns once its capture ends.",
            )),
        }
    }

    /// Refuses a call by the method its manifest does not name.
    fn driven(&self, by: DrivenBy) -> Result<(), Failure> {
        if self.decl.driven_by == by {
            return Ok(());
        }
        let (asked, named) = match by {
            DrivenBy::Engine => ("discover", "watch"),
            DrivenBy::Person => ("watch", "discover"),
        };
        Err(codes::failure(
            &codes::row::EFFECT_NOT_DECLARED,
            format!(
                "The discoverer `{}` of `{}` is driven by the {}, as its manifest says, and is called by `{named}`, not by `{asked}`.",
                self.decl.name,
                self.provider,
                self.decl.driven_by.as_str()
            ),
            "Call a discoverer by the method its manifest's `driven_by` names: `discover` for the engine, a capture job's `watch` for a person.",
        ))
    }

    /// Describes findings with the discoverer's describer.
    #[must_use]
    pub fn describe(&self, findings: &[Finding], form: DescribeForm) -> Description {
        self.describer.describe(findings, form)
    }
}
