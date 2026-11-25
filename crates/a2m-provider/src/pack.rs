//! What a pack is to the kernel, what it is handed when it registers, and
//! what it hands over.

use std::collections::BTreeSet;
use std::fmt;
use std::sync::Arc;

use a2m_auth::AuthProvider;
use a2m_core::{ControlName, Failure, FindingKind, Mode, Redact, Values};
use a2m_files::{DraftingRule, FileKind, LintRule, Lowering, SurfaceKindSpec};
use a2m_logs::LogsAndTraces;
use a2m_sources::SourceReader;

use crate::action::ProviderAction;
use crate::capture::CaptureSource;
use crate::codes;
use crate::compare::KeyRule;
use crate::discover::{Describer, Discoverer};
use crate::driver::Driver;
use crate::manifest::{KindOf, Manifest};
use crate::name::{ProviderName, RootName};
use crate::path::{Access, CheckedPath, Roots};
use crate::supervisor::ScopedSupervisor;

/// A first-party pack. The product crate makes one call per enabled pack,
/// [`RegistryBuilder::add_pack`](crate::RegistryBuilder::add_pack), and no
/// other.
pub trait Pack: Send + Sync {
    /// The manifest's TOML text, embedded with `include_str!`.
    fn manifest(&self) -> &'static str;

    /// Hands the pack's objects to the registrar. Each call is checked
    /// against the manifest as it is made. A pack keeps what it needs of
    /// `cx` inside the objects it registers.
    ///
    /// # Errors
    ///
    /// Any failure: the pack is then listed with it and registers nothing.
    fn register(&self, cx: &PackContext, to: &mut Registrar<'_>) -> Result<(), Failure>;
}

/// What a pack is handed when it registers: everything it may use that is
/// not the system itself.
#[derive(Clone)]
pub struct PackContext {
    pub(crate) provider: ProviderName,
    pub(crate) supervisor: ScopedSupervisor,
    pub(crate) roots: Roots,
    pub(crate) redact: Arc<dyn Redact>,
    pub(crate) logs_and_traces: LogsAndTraces,
    pub(crate) settings: Values,
}

impl fmt::Debug for PackContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PackContext")
            .field("provider", &self.provider)
            .finish_non_exhaustive()
    }
}

impl PackContext {
    /// The pack's own name.
    #[must_use]
    pub fn provider(&self) -> &ProviderName {
        &self.provider
    }

    /// A supervisor that starts only the programs the pack's manifest
    /// declares under `effects.processes`.
    #[must_use]
    pub fn supervisor(&self) -> &ScopedSupervisor {
        &self.supervisor
    }

    /// The roots the pack's manifest declares, and nothing else.
    #[must_use]
    pub fn roots(&self) -> &Roots {
        &self.roots
    }

    /// One of the pack's folders: a declared root, as a checked path.
    ///
    /// # Errors
    ///
    /// `policy`/`effect_not_declared` for a root the manifest does not
    /// declare, or declares for reading when `access` is `Write`.
    pub fn folder(&self, root: &RootName, access: Access) -> Result<CheckedPath, Failure> {
        self.roots.folder(root, access)
    }

    /// The redactor. Register nothing with it here; pass every text that
    /// leaves the pack through it.
    #[must_use]
    pub fn redact(&self) -> &Arc<dyn Redact> {
        &self.redact
    }

    /// LogsAndTraces: spans, metrics, and logs.
    #[must_use]
    pub fn logs_and_traces(&self) -> &LogsAndTraces {
        &self.logs_and_traces
    }

    /// The pack's own settings group, `packs.<pack>`, checked against the
    /// schema its manifest declares, with every default filled in.
    #[must_use]
    pub fn settings(&self) -> &Values {
        &self.settings
    }
}

/// What a pack brings to the compiler: the objects behind the surface
/// kinds, controls, file kinds, lint rules, and drafting rules its manifest
/// declares, and the lowerings of its modes.
#[derive(Clone, Default)]
pub struct CompilerRegistrations {
    /// The surface kinds it serves.
    pub surface_kinds: Vec<SurfaceKindSpec>,
    /// The controls it can operate.
    pub controls: Vec<ControlName>,
    /// The file kinds it owns.
    pub file_kinds: Vec<Arc<dyn FileKind>>,
    /// The lowerings of its modes.
    pub lowerings: Vec<Arc<dyn Lowering>>,
    /// Its lint rules.
    pub lint_rules: Vec<Arc<dyn LintRule>>,
    /// Its drafting rules.
    pub drafting_rules: Vec<Arc<dyn DraftingRule>>,
}

impl fmt::Debug for CompilerRegistrations {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CompilerRegistrations")
            .field("surface_kinds", &self.surface_kinds.len())
            .field("controls", &self.controls.len())
            .field("file_kinds", &self.file_kinds.len())
            .field("lowerings", &self.lowerings.len())
            .field("lint_rules", &self.lint_rules.len())
            .field("drafting_rules", &self.drafting_rules.len())
            .finish()
    }
}

impl CompilerRegistrations {
    /// Nothing yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a surface kind.
    #[must_use]
    pub fn surface_kind(mut self, spec: SurfaceKindSpec) -> Self {
        self.surface_kinds.push(spec);
        self
    }

    /// Adds a control.
    #[must_use]
    pub fn control(mut self, control: ControlName) -> Self {
        self.controls.push(control);
        self
    }

    /// Adds a file kind.
    #[must_use]
    pub fn file_kind(mut self, kind: Arc<dyn FileKind>) -> Self {
        self.file_kinds.push(kind);
        self
    }

    /// Adds a lowering.
    #[must_use]
    pub fn lowering(mut self, lowering: Arc<dyn Lowering>) -> Self {
        self.lowerings.push(lowering);
        self
    }

    /// Adds a lint rule.
    #[must_use]
    pub fn lint_rule(mut self, rule: Arc<dyn LintRule>) -> Self {
        self.lint_rules.push(rule);
        self
    }

    /// Adds a drafting rule.
    #[must_use]
    pub fn drafting_rule(mut self, rule: Arc<dyn DraftingRule>) -> Self {
        self.drafting_rules.push(rule);
        self
    }
}

/// Everything one provider handed over.
#[derive(Default)]
pub(crate) struct Collected {
    pub drivers: Vec<(Mode, Arc<dyn Driver>)>,
    pub discoverers: Vec<(Arc<dyn Discoverer>, Arc<dyn Describer>)>,
    pub auth_providers: Vec<Arc<dyn AuthProvider>>,
    pub source_readers: Vec<Arc<dyn SourceReader>>,
    pub compiler: CompilerRegistrations,
    pub actions: Vec<Arc<dyn ProviderAction>>,
    pub capture: Option<Arc<dyn CaptureSource>>,
    pub key_rules: Vec<(FindingKind, Arc<dyn KeyRule>)>,
}

/// What a pack hands its objects to. Each call is checked against the
/// manifest as it is made: an object the manifest does not declare is
/// refused with `policy`/`effect_not_declared`, and so is one handed over
/// twice.
pub struct Registrar<'a> {
    pub(crate) manifest: &'a Manifest,
    pub(crate) collected: Collected,
    seen: BTreeSet<String>,
}

impl fmt::Debug for Registrar<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Registrar")
            .field("provider", &self.manifest.provider.name)
            .finish_non_exhaustive()
    }
}

impl<'a> Registrar<'a> {
    pub(crate) fn new(manifest: &'a Manifest) -> Self {
        Self {
            manifest,
            collected: Collected::default(),
            seen: BTreeSet::new(),
        }
    }

    fn undeclared(&self, what: &str) -> Failure {
        codes::failure(
            &codes::row::EFFECT_NOT_DECLARED,
            format!(
                "The provider `{}` registers {what}, which its manifest does not declare.",
                self.manifest.provider.name
            ),
            "Declare it in the provider's manifest, where a person can review it.",
        )
    }

    fn once(&mut self, what: String) -> Result<(), Failure> {
        if self.seen.insert(what.clone()) {
            Ok(())
        } else {
            Err(codes::failure(
                &codes::row::EFFECT_NOT_DECLARED,
                format!(
                    "The provider `{}` registers {what} twice.",
                    self.manifest.provider.name
                ),
                "Register each object once.",
            ))
        }
    }

    /// Registers the driver of a mode the manifest lists under
    /// `points.driver`.
    ///
    /// # Errors
    ///
    /// `policy`/`effect_not_declared`.
    pub fn driver(&mut self, mode: Mode, driver: Arc<dyn Driver>) -> Result<(), Failure> {
        if !self.manifest.points.driver.contains(&mode) {
            return Err(self.undeclared(&format!("a driver for the mode `{mode}`")));
        }
        self.once(format!("the driver of `{mode}`"))?;
        self.collected.drivers.push((mode, driver));
        Ok(())
    }

    /// Registers a discoverer the manifest lists under `points.discoverer`,
    /// with its describer.
    ///
    /// # Errors
    ///
    /// `policy`/`effect_not_declared`.
    pub fn discoverer(
        &mut self,
        discoverer: Arc<dyn Discoverer>,
        describer: Arc<dyn Describer>,
    ) -> Result<(), Failure> {
        let name = discoverer.name();
        if !self.manifest.points.discoverer.contains(&name) {
            return Err(self.undeclared(&format!("the discoverer `{name}`")));
        }
        self.once(format!("the discoverer `{name}`"))?;
        self.collected.discoverers.push((discoverer, describer));
        Ok(())
    }

    /// Registers the pack's one capture source, which offers traffic
    /// capture of the surface kinds the manifest lists under
    /// `points.traffic_capture` (decision 0391).
    ///
    /// # Errors
    ///
    /// `policy`/`effect_not_declared` when the manifest lists none, or for
    /// a second source.
    pub fn traffic_capture(&mut self, source: Arc<dyn CaptureSource>) -> Result<(), Failure> {
        if self.manifest.points.traffic_capture.is_empty() {
            return Err(self.undeclared("a traffic capture source"));
        }
        self.once("a traffic capture source".to_owned())?;
        self.collected.capture = Some(source);
        Ok(())
    }

    /// Registers the key rule of a finding kind the manifest owns, under
    /// `finding_kinds`: what the comparison of what is described with what
    /// is observed reads a finding of that kind by (decision 0486).
    ///
    /// # Errors
    ///
    /// `policy`/`effect_not_declared` for a kind the manifest does not
    /// own, or a second rule for one kind.
    pub fn key_rule(&mut self, kind: FindingKind, rule: Arc<dyn KeyRule>) -> Result<(), Failure> {
        if !self
            .manifest
            .finding_kinds
            .iter()
            .any(|owned| owned.name == kind)
        {
            return Err(self.undeclared(&format!(
                "a key rule for the finding kind `{}`, which it does not own",
                kind.as_str()
            )));
        }
        self.once(format!("the key rule of `{}`", kind.as_str()))?;
        self.collected.key_rules.push((kind, rule));
        Ok(())
    }

    /// Registers an auth provider the manifest lists under
    /// `points.auth_provider`. Only a provider that needs a surface lives in
    /// a pack; the others are `a2m-auth`'s own.
    ///
    /// # Errors
    ///
    /// `policy`/`effect_not_declared`.
    pub fn auth_provider(&mut self, provider: Arc<dyn AuthProvider>) -> Result<(), Failure> {
        let name = provider.name();
        if !self.manifest.points.auth_provider.contains(name.as_str()) {
            return Err(self.undeclared(&format!("the auth provider `{}`", name.as_str())));
        }
        self.once(format!("the auth provider `{}`", name.as_str()))?;
        self.collected.auth_providers.push(provider);
        Ok(())
    }

    /// Registers a source reader the manifest lists under
    /// `points.source_reader`.
    ///
    /// # Errors
    ///
    /// `policy`/`effect_not_declared`.
    pub fn source_reader(&mut self, reader: Arc<dyn SourceReader>) -> Result<(), Failure> {
        let name = reader.name().as_str().to_owned();
        if !self.manifest.points.source_reader.contains(&name) {
            return Err(self.undeclared(&format!("the source reader `{name}`")));
        }
        self.once(format!("the source reader `{name}`"))?;
        self.collected.source_readers.push(reader);
        Ok(())
    }

    /// Registers what the pack brings to the compiler. May be called more
    /// than once; each object is checked against the manifest: a surface
    /// kind, control, file kind, lint rule, or drafting rule must be
    /// declared, a lowering must be of a declared mode and verbs with
    /// declared step kinds, and a kind a lowering reads must be declared
    /// under `reads` or owned.
    ///
    /// # Errors
    ///
    /// `policy`/`effect_not_declared`.
    pub fn compiler(&mut self, registrations: CompilerRegistrations) -> Result<(), Failure> {
        let manifest = self.manifest;
        for spec in &registrations.surface_kinds {
            if !manifest
                .surface_kinds
                .iter()
                .any(|kind| kind.name == spec.kind)
            {
                return Err(self.undeclared(&format!("the surface kind `{}`", spec.kind)));
            }
        }
        for control in &registrations.controls {
            if !manifest.controls.contains(control) {
                return Err(self.undeclared(&format!("the control `{control}`")));
            }
        }
        for kind in &registrations.file_kinds {
            let name = kind.name();
            let declared = manifest.file_kinds.iter().find(|own| own.name == name);
            let fits = declared.is_some_and(|own| {
                own.directory.trim_end_matches('/') == kind.folder()
                    && kind
                        .surface_kinds()
                        .iter()
                        .all(|surface_kind| own.surface_kinds.contains(surface_kind))
            });
            if !fits {
                return Err(self.undeclared(&format!(
                    "the file kind `{name}` in the folder `{}` for its surface kinds",
                    kind.folder()
                )));
            }
        }
        for lowering in &registrations.lowerings {
            let mode = lowering.mode();
            let Some(declared) = manifest.mode(&mode) else {
                return Err(self.undeclared(&format!("a lowering for the mode `{mode}`")));
            };
            if let Some(verb) = lowering
                .verbs()
                .iter()
                .find(|verb| !declared.verbs.contains(verb))
            {
                return Err(self.undeclared(&format!("the verb `{verb}` of the mode `{mode}`")));
            }
            if let Some(step) = lowering
                .step_kinds()
                .iter()
                .find(|step| !declared.steps.contains(step))
            {
                return Err(
                    self.undeclared(&format!("the step kind `{step}` of the mode `{mode}`"))
                );
            }
            for (kind, surface_kind) in lowering.reads() {
                let read = manifest.reads.iter().any(|read| {
                    read.of == KindOf::FileKinds
                        && read.kind == kind.as_str()
                        && read.surface_kind == *surface_kind
                });
                if !read && !manifest.owns(KindOf::FileKinds, kind.as_str(), surface_kind) {
                    return Err(self.undeclared(&format!(
                        "a lowering that reads the file kind `{kind}` of `{surface_kind}` surfaces"
                    )));
                }
            }
        }
        for rule in &registrations.lint_rules {
            let code = rule.code();
            if !manifest.lint_rules.iter().any(|own| own.code == code) {
                return Err(self.undeclared(&format!("the lint rule `{code}`")));
            }
        }
        for rule in &registrations.drafting_rules {
            let (finding_kind, file_kind) = (rule.finding_kind(), rule.file_kind());
            let declared = manifest
                .drafting_rules
                .iter()
                .any(|own| own.finding_kind == finding_kind && own.file_kind == file_kind);
            if !declared {
                return Err(self.undeclared(&format!(
                    "a drafting rule from `{}` findings to `{file_kind}` files",
                    finding_kind.as_str()
                )));
            }
        }
        let into = &mut self.collected.compiler;
        into.surface_kinds.extend(registrations.surface_kinds);
        into.controls.extend(registrations.controls);
        into.file_kinds.extend(registrations.file_kinds);
        into.lowerings.extend(registrations.lowerings);
        into.lint_rules.extend(registrations.lint_rules);
        into.drafting_rules.extend(registrations.drafting_rules);
        Ok(())
    }

    /// Registers an action the manifest lists under `actions`.
    ///
    /// # Errors
    ///
    /// `policy`/`effect_not_declared`.
    pub fn action(&mut self, action: Arc<dyn ProviderAction>) -> Result<(), Failure> {
        let name = action.name();
        if !self.manifest.actions.iter().any(|own| own.name == name) {
            return Err(self.undeclared(&format!("the action `{name}`")));
        }
        self.once(format!("the action `{name}`"))?;
        self.collected.actions.push(action);
        Ok(())
    }
}
