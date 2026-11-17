//! Discovery: what a pack implements to observe a system, and the only
//! route it is given to reach one.
//!
//! A discoverer cannot write. It is handed a [`SandboxPort`], a scope, and
//! a sink, and nothing else. The port's one method takes no authority, so
//! there is nowhere to pass an intent token; a [`DriverHandle`] is given
//! only by the registry, which no pack receives; and an intent token has no
//! public constructor.
//!
//! [`DriverHandle`]: crate::DriverHandle

use std::fmt;
use std::sync::Arc;

use a2m_core::{
    Archetype, Coverage, EvidenceLevel, Failure, Finding, FindingKind, Redact, Step, SurfaceKind,
    Values,
};

use crate::BoxFuture;
use crate::codes;
use crate::driver::{Deadline, StepFailure, StepInput, StepOutcome, SurfaceRef};
use crate::manifest::{Bounds, DiscovererDecl};
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
            return Err(codes::unknown_variant(
                codes::FINDING_NOT_DECLARED,
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
    /// The discoverer's failure; `unknown_variant`/`finding_not_declared`
    /// when it makes a finding its manifest does not allow.
    pub async fn discover(
        &self,
        port: &dyn SandboxPort,
        scope: &DiscoveryScope<'_>,
        out: &mut dyn FindingSink,
    ) -> Result<DiscoveryReport, Failure> {
        let mut checked = DeclaredOnly {
            kinds: &self.decl.kinds,
            max_level: self.decl.max_level,
            out,
        };
        self.discoverer.discover(port, scope, &mut checked).await
    }

    /// Describes findings with the discoverer's describer.
    #[must_use]
    pub fn describe(&self, findings: &[Finding], form: DescribeForm) -> Description {
        self.describer.describe(findings, form)
    }
}
