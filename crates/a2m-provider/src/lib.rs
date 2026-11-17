//! The provider contract for Anything2MCP: the one doorway between the
//! kernel and everything that knows a kind of system.
//!
//! It holds the manifest and its checks ([`Manifest`]), the registry that
//! packs fill ([`RegistryBuilder`], [`Registry`]), the extension points a
//! pack implements ([`Pack`], [`Driver`], [`Discoverer`], [`Describer`],
//! [`ProviderAction`], and, from the crates below, `AuthProvider`,
//! `SourceReader`, and the compiler's registration traits), the handle
//! through which alone the engine reaches a driver ([`DriverHandle`]), file
//! access for packs ([`Roots`], [`CheckedPath`]), and the supervisor that
//! starts and stops every outside process ([`Supervisor`],
//! [`ScopedSupervisor`]). It knows no kind of system itself.
//!
//! Every async method of every trait here returns a [`BoxFuture`], so that
//! each trait can be used as a trait object; an implementation writes
//! `Box::pin(async move { ... })`.
//!
//! Not built yet: the plug-in host and protocol, the framed link with its
//! launch handshake, ownership records in the store, and the check of a
//! plug-in's signature. A plug-in is listed as present and not enabled.
//!
//! The crates this one may depend on are set by the layering table in
//! `docs/reference/technical-reference.md`: layers 1 to 4.

mod action;
pub mod codes;
mod discover;
mod driver;
mod manifest;
mod name;
mod pack;
mod path;
mod registry;
mod replay;
mod supervisor;

/// The boxed future every async trait method of this crate returns. It is
/// the same type as `a2m-auth`'s and the vault's.
pub use a2m_auth::BoxFuture;

pub use action::{ActionContext, ProviderAction};
pub use discover::{
    DescribeForm, Describer, Description, Discoverer, DiscovererHandle, DiscoveryReport,
    DiscoveryScope, FindingSink, SandboxPort, SurfaceDescription,
};
pub use driver::{
    Authority, Capture, ChoiceReason, Deadline, Destinations, Driver, DriverHandle, DriverHealth,
    Observed, Posture, PrepareContext, Prepared, ProviderChoice, ReadOnlyPort, Sent, StepFailure,
    StepInput, StepOutcome, SurfaceRef,
};
pub use manifest::{
    ActionDecl, Bounds, CONTRACT, DiscovererDecl, DraftingRuleDecl, Effects, EvidenceDecl,
    Exclusive, FailureCodeDecl, FileKindDecl, FindingKindDecl, KindOf, LintRuleDecl,
    MANIFEST_FILE_NAME, MANIFEST_FORMAT, Manifest, ModeDecl, ModeDefaults, NetworkEffect, Points,
    ProgramDecl, ProviderKind, ProviderSection, ReadDecl, Requirement, RunSection, SettingDecl,
    SettingScope, SettingType, SurfaceKindDecl,
};
pub use name::{ActionName, DiscovererName, ProviderName, RootName, SubsystemKind};
pub use pack::{CompilerRegistrations, Pack, PackContext, Registrar};
pub use path::{Access, CheckedPath, CheckedPaths, EntryKind, FileIdentity, Roots, Temporary};
pub use registry::{
    FoundManifest, HostServices, ProviderListing, ProviderSettings, RefusedProvider, Registry,
    RegistryBuilder, RegistryProblem, SurfaceKindEntry, current_platform,
};
pub use replay::{Recorded, RecordedAnswer, Replay, answer_name};
pub use supervisor::{
    Cleanup, ConnFailure, EnvAllowlist, Exit, Finished, ForeignLink, ForeignPipe, LaunchSpec,
    Leftover, LinkKind, OWNER_MARK_VARIABLE, OneShot, ProgramRef, Ran, ScopedSupervisor,
    SubsystemId, SubsystemListing, Supervisor, SupervisorLimits, is_engine_variable,
};
