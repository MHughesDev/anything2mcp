//! The registry: one immutable snapshot of every provider, its modes with
//! their archetypes, and the objects each supplied.
//!
//! The product crate builds it once at start, with one call per pack, and
//! builds a new one when a provider is enabled or disabled. It is never
//! changed in place. The order in which providers are added changes
//! nothing: not the digest, not a choice, not a problem.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use a2m_auth::AuthProvider;
use a2m_core::{
    Archetype, Availability, Digest, EffectClass, Failure, FailureClass, FailureCode, Mode, Redact,
    SurfaceKind, Values, Version,
};
use a2m_files::CompilerSet;
use a2m_sources::{ReaderName, SourceReader};
use a2m_telemetry::Telemetry;

use crate::action::ProviderAction;
use crate::codes;
use crate::discover::{DiscovererHandle, SurfaceDescription};
use crate::driver::{ChoiceReason, Driver, DriverHandle, HandleFacts, ProviderChoice};
use crate::manifest::{KindOf, Manifest, ModeDecl, ModeDefaults, ProviderKind};
use crate::name::{ActionName, ProviderName};
use crate::pack::{Collected, Pack, PackContext, Registrar};
use crate::path::Roots;
use crate::supervisor::{Allowed, ScopedSupervisor, Supervisor};

/// What the product crate gives the registry to hand on to packs.
#[derive(Clone)]
pub struct HostServices {
    /// The supervisor. Each pack receives it scoped to its own manifest.
    pub supervisor: Supervisor,
    /// Every named root. Each pack receives the subset its manifest
    /// declares.
    pub roots: Roots,
    /// The redactor.
    pub redact: Arc<dyn Redact>,
    /// Telemetry.
    pub telemetry: Telemetry,
}

impl fmt::Debug for HostServices {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HostServices").finish_non_exhaustive()
    }
}

/// The settings the registry is built from. The product crate reads them;
/// this crate never opens a settings file.
#[derive(Debug, Clone, Default)]
pub struct ProviderSettings {
    /// The providers switched off. A pack that is compiled in is enabled
    /// unless it is listed here.
    pub disabled: BTreeSet<ProviderName>,
    /// The provider that drives a mode, where more than one could.
    pub mode_providers: BTreeMap<Mode, ProviderName>,
    /// Each pack's own settings group, `packs.<pack>`, as it was written.
    pub packs: BTreeMap<ProviderName, Values>,
}

/// A plug-in found on the machine: its manifest's text and its folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundManifest {
    /// The text of `anything2mcp-provider.toml`.
    pub text: String,
    /// The folder that holds it.
    pub folder: PathBuf,
}

/// Something wrong with one provider, found while the registry was built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryProblem {
    /// The provider, when its manifest could be read far enough to name it.
    pub provider: Option<ProviderName>,
    /// What is wrong.
    pub failure: Failure,
    /// Whether the provider is registered all the same. False for every
    /// problem but a kind it reads whose owner is missing.
    pub registered: bool,
}

/// One provider, as `provider.list` shows it: what it declares, whether it
/// can work here now, and what is wrong with it.
#[derive(Debug, Clone)]
pub struct ProviderListing {
    /// Its name.
    pub name: ProviderName,
    /// Its version.
    pub version: Version,
    /// A pack or a plug-in.
    pub kind: ProviderKind,
    /// What it declares.
    pub manifest: Arc<Manifest>,
    /// Whether it can do its work here, now. Computed by looking at the
    /// machine; nothing was started.
    pub availability: Availability,
    /// What to do when it is not ready.
    pub fix: String,
    /// Whether its objects are in the registry.
    pub registered: bool,
    /// What is wrong with it.
    pub problems: Vec<Failure>,
}

/// A registered surface kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceKindEntry {
    /// Its archetype.
    pub archetype: Archetype,
    /// Whether the keeper holds a session on a surface of this kind.
    pub session: bool,
    /// The provider that serves it.
    pub provider: ProviderName,
}

struct Pending {
    manifest: Arc<Manifest>,
    collected: Option<Collected>,
    availability: Availability,
    fix: String,
    problems: Vec<Failure>,
    supervisor: Option<ScopedSupervisor>,
    roots: Roots,
    settings: Values,
}

/// Gathers providers, then builds the registry.
pub struct RegistryBuilder {
    settings: ProviderSettings,
    host: HostServices,
    pending: Vec<Pending>,
    unnamed: Vec<RefusedProvider>,
}

impl fmt::Debug for RegistryBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RegistryBuilder")
            .field("providers", &self.pending.len())
            .finish_non_exhaustive()
    }
}

/// The host as a manifest's `platforms` writes it, such as `linux-x86_64`.
#[must_use]
pub fn current_platform() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

fn file_digest(path: &Path) -> Option<Digest> {
    crate::supervisor::digest_of(path).ok()
}

/// The availability check of spec section 203. It looks at the machine and
/// starts nothing: this function has no way to reach the supervisor.
fn availability(manifest: &Manifest) -> (Availability, String) {
    if !manifest.platforms.is_empty() && !manifest.platforms.contains(&current_platform()) {
        return (
            Availability::UnsupportedHost,
            format!(
                "`{}` supports: {}.",
                manifest.provider.name,
                manifest.platforms.join(", ")
            ),
        );
    }
    for needed in &manifest.requirements {
        // A place written for another operating system is not an absolute
        // path here; it would be looked for in the engine's own working
        // folder, and is passed over.
        let found = needed
            .paths
            .iter()
            .map(Path::new)
            .find(|path| path.is_absolute() && path.is_file());
        let Some(found) = found else {
            return (Availability::ComponentMissing, needed.missing.clone());
        };
        if !needed.known_digests.is_empty()
            && !file_digest(found).is_some_and(|digest| needed.known_digests.contains(&digest))
        {
            return (
                Availability::ComponentUnverified,
                format!(
                    "`{}` was found and its digest is not a known one. Review the program, then allow it in the settings.",
                    needed.name
                ),
            );
        }
    }
    (Availability::Ready, String::new())
}

impl RegistryBuilder {
    /// A builder with no provider yet.
    #[must_use]
    pub fn new(settings: ProviderSettings, host: HostServices) -> Self {
        Self {
            settings,
            host,
            pending: Vec::new(),
            unnamed: Vec::new(),
        }
    }

    /// Adds a first-party pack: reads its manifest, checks it, and, when
    /// the pack is enabled, has it register its objects. The product crate
    /// makes no other call per pack. This call reads the files of the
    /// pack's requirements to take their digests; make it before the
    /// engine serves, or on a thread that may block.
    pub fn add_pack(&mut self, pack: &dyn Pack) {
        let manifest = match Manifest::parse(pack.manifest()) {
            Ok(manifest) => Arc::new(manifest),
            Err(failure) => {
                self.unnamed.push(RefusedProvider::of(
                    pack.manifest(),
                    ProviderKind::Pack,
                    failure,
                ));
                return;
            }
        };
        let name = manifest.provider.name.clone();
        let mut pending = Pending {
            manifest: Arc::clone(&manifest),
            collected: None,
            availability: Availability::Unknown,
            fix: String::new(),
            problems: Vec::new(),
            supervisor: None,
            roots: Roots::none(),
            settings: Values::new(),
        };
        if manifest.provider.kind != ProviderKind::Pack {
            pending.problems.push(codes::failure(
                &codes::row::MANIFEST_INVALID,
                format!("`{name}` is compiled in and its manifest says it is a plug-in."),
                "Write `kind = \"pack\"` in the pack's manifest.",
            ));
            self.pending.push(pending);
            return;
        }
        if self.settings.disabled.contains(&name) {
            pending.availability = Availability::NotEnabled;
            pending.fix = format!("Enable it with `anything2mcp provider enable {name}`.");
            self.pending.push(pending);
            return;
        }
        let given = self.settings.packs.get(&name).cloned().unwrap_or_default();
        let settings = match manifest.validate_settings(&given) {
            Ok(settings) => settings,
            Err(failure) => {
                pending.availability = Availability::Misconfigured;
                pending.fix = failure.next_step().to_owned();
                pending.problems.push(failure);
                self.pending.push(pending);
                return;
            }
        };
        let roots = match self
            .host
            .roots
            .narrowed(&manifest.effects.read_roots, &manifest.effects.write_roots)
        {
            Ok(roots) => roots,
            Err(failure) => {
                pending.availability = Availability::Misconfigured;
                pending.fix = failure.next_step().to_owned();
                pending.problems.push(failure);
                self.pending.push(pending);
                return;
            }
        };
        (pending.availability, pending.fix) = availability(&manifest);

        // What the pack may start: the programs its manifest names, and the
        // places it looks for the programs it requires, each with the
        // digests declared for it. They are checked at every start.
        let mut declared: Vec<Allowed> = manifest
            .effects
            .processes
            .iter()
            .map(|program| Allowed {
                path: PathBuf::from(&program.path),
                digests: program.digest.into_iter().collect(),
            })
            .collect();
        for needed in &manifest.requirements {
            declared.extend(needed.paths.iter().map(|path| Allowed {
                path: PathBuf::from(path),
                digests: needed.known_digests.clone(),
            }));
        }
        let supervisor = ScopedSupervisor::new(
            self.host.supervisor.clone(),
            name.clone(),
            declared,
            manifest.effects.plan_programs,
        );
        let context = PackContext {
            provider: name,
            supervisor: supervisor.clone(),
            roots: roots.clone(),
            redact: Arc::clone(&self.host.redact),
            telemetry: self.host.telemetry.clone(),
            settings: settings.clone(),
        };
        let mut registrar = Registrar::new(&manifest);
        match pack.register(&context, &mut registrar) {
            Ok(()) => pending.collected = Some(registrar.collected),
            Err(failure) => pending
                .problems
                .push(self.host.redact.redact_failure(&failure)),
        }
        pending.supervisor = Some(supervisor);
        pending.roots = roots;
        pending.settings = settings;
        self.pending.push(pending);
    }

    /// Adds a plug-in found on the machine. Its manifest is read and
    /// checked, and it is listed as present and not enabled: the plug-in
    /// host that would start it is not built, so it registers nothing.
    pub fn add_plugin(&mut self, found: &FoundManifest) {
        match Manifest::parse(&found.text) {
            Err(failure) => self.unnamed.push(RefusedProvider::of(
                &found.text,
                ProviderKind::Plugin,
                failure,
            )),
            Ok(manifest) => {
                let mut problems = Vec::new();
                if manifest.provider.kind != ProviderKind::Plugin {
                    problems.push(codes::failure(
                        &codes::row::MANIFEST_INVALID,
                        format!(
                            "`{}` was found as a plug-in and its manifest says it is a pack.",
                            manifest.provider.name
                        ),
                        "Write `kind = \"plugin\"` in the plug-in's manifest.",
                    ));
                }
                self.pending.push(Pending {
                    manifest: Arc::new(manifest),
                    collected: None,
                    availability: Availability::NotEnabled,
                    fix: "Plug-ins cannot be enabled in this version.".to_owned(),
                    problems,
                    supervisor: None,
                    roots: Roots::none(),
                    settings: Values::new(),
                });
            }
        }
    }

    /// Builds the registry, and reports every problem at once. A provider
    /// with a problem is listed with it and registers nothing, except a
    /// provider that reads a kind whose owner is missing: it stays, and
    /// only a binding that needs the kind does not compile.
    #[must_use]
    #[allow(clippy::too_many_lines)] // the checks run in one fixed order; splitting them hides it
    pub fn build(self) -> (Registry, Vec<RegistryProblem>) {
        let Self {
            settings,
            host,
            mut pending,
            mut unnamed,
        } = self;
        // A refused manifest's name is its own unchecked word. One that
        // says the name of a provider that is listed, or of a refused one
        // named before it, is kept unnamed, so that its problem is never
        // listed under a name another provider holds (decision 0193).
        let mut taken: BTreeSet<String> = pending
            .iter()
            .map(|one| one.manifest.provider.name.as_str().to_owned())
            .collect();
        for refused in &mut unnamed {
            if let Some(name) = refused.name.take() {
                if taken.insert(name.clone()) {
                    refused.name = Some(name);
                } else {
                    refused.version = None;
                }
            }
        }
        // Name order, so that the order of the calls changes nothing.
        pending.sort_by(|a, b| {
            (&a.manifest.provider.name, &a.manifest.digest)
                .cmp(&(&b.manifest.provider.name, &b.manifest.digest))
        });
        let mut soft: Vec<Vec<Failure>> = pending.iter().map(|_| Vec::new()).collect();

        // One name, one provider.
        for index in 0..pending.len() {
            let name = &pending[index].manifest.provider.name;
            let twice = pending
                .iter()
                .enumerate()
                .any(|(other, p)| other != index && p.manifest.provider.name == *name);
            if twice {
                let failure = codes::failure(
                    &codes::row::PROVIDER_DUPLICATE,
                    format!("Two providers carry the name `{name}`."),
                    "Remove one of the two.",
                );
                pending[index].problems.push(failure);
            }
        }

        let usable = |p: &Pending| p.collected.is_some() && p.problems.is_empty();

        // Modes and surface kinds agree on their archetype; a kind has one
        // owner for a surface kind.
        let mut conflicts: Vec<(usize, Failure)> = Vec::new();
        for (index, one) in pending.iter().enumerate().filter(|(_, p)| usable(p)) {
            for (other_index, other) in pending.iter().enumerate().filter(|(_, p)| usable(p)) {
                if index == other_index {
                    continue;
                }
                let other_name = &other.manifest.provider.name;
                for mode in &one.manifest.modes {
                    let differs = other
                        .manifest
                        .mode(&mode.name)
                        .is_some_and(|theirs| theirs.archetype != mode.archetype);
                    if differs {
                        conflicts.push((
                            index,
                            codes::failure(
                                &codes::row::MODE_CONFLICT,
                                format!(
                                    "The mode `{}` is registered with two archetypes, here and by `{other_name}`.",
                                    mode.name
                                ),
                                "Disable one of the two providers.",
                            ),
                        ));
                    }
                }
                for kind in &one.manifest.surface_kinds {
                    if other
                        .manifest
                        .surface_kinds
                        .iter()
                        .any(|theirs| theirs.name == kind.name)
                    {
                        conflicts.push((
                            index,
                            codes::failure(
                                &codes::row::MODE_CONFLICT,
                                format!(
                                    "The surface kind `{}` is registered here and by `{other_name}`.",
                                    kind.name
                                ),
                                "Disable one of the two providers.",
                            ),
                        ));
                    }
                }
                let owned = one
                    .manifest
                    .file_kinds
                    .iter()
                    .map(|kind| (KindOf::FileKinds, kind.name.as_str(), &kind.surface_kinds))
                    .chain(one.manifest.finding_kinds.iter().map(|kind| {
                        (
                            KindOf::FindingKinds,
                            kind.name.as_str(),
                            &kind.surface_kinds,
                        )
                    }));
                for (of, kind, surface_kinds) in owned {
                    for surface_kind in surface_kinds {
                        if other.manifest.owns(of, kind, surface_kind) {
                            conflicts.push((
                                index,
                                codes::failure(
                                    &codes::row::KIND_OWNER_CONFLICT,
                                    format!(
                                        "The kind `{kind}` of `{surface_kind}` surfaces is owned here and by `{other_name}`."
                                    ),
                                    "Disable one of the two providers; a kind has one owner for a surface kind.",
                                ),
                            ));
                        }
                    }
                }
            }
        }
        for (index, failure) in conflicts {
            pending[index].problems.push(failure);
        }

        // The compiler's set, and the same again without each provider it
        // refuses, until none is refused.
        let compiler = loop {
            let mut builder = CompilerSet::builder();
            let mut refused = None;
            'providers: for (index, one) in pending.iter().enumerate().filter(|(_, p)| usable(p)) {
                let Some(collected) = &one.collected else {
                    continue;
                };
                let name = one.manifest.provider.name.as_str();
                let given = &collected.compiler;
                let mut steps: Vec<Result<(), Failure>> = Vec::new();
                for spec in &given.surface_kinds {
                    steps.push(builder.surface_kind(name, spec.clone()).map(|_| ()));
                }
                for control in &given.controls {
                    steps.push(builder.control(name, control.clone()).map(|_| ()));
                }
                for kind in &given.file_kinds {
                    steps.push(builder.file_kind(name, Arc::clone(kind)).map(|_| ()));
                }
                for lowering in &given.lowerings {
                    steps.push(builder.lowering(name, Arc::clone(lowering)).map(|_| ()));
                }
                for rule in &given.lint_rules {
                    steps.push(builder.lint_rule(name, Arc::clone(rule)).map(|_| ()));
                }
                for rule in &given.drafting_rules {
                    steps.push(builder.drafting_rule(name, Arc::clone(rule)).map(|_| ()));
                }
                if let Some(failure) = steps.into_iter().find_map(Result::err) {
                    refused = Some((index, failure));
                    break 'providers;
                }
            }
            match refused {
                Some((index, failure)) => pending[index].problems.push(failure),
                None => break builder.build(),
            }
        };

        // A kind a provider reads has an enabled owner.
        for index in 0..pending.len() {
            if !usable(&pending[index]) {
                continue;
            }
            for read in &pending[index].manifest.reads {
                let owner = |p: &Pending| p.manifest.owns(read.of, &read.kind, &read.surface_kind);
                if pending.iter().any(|p| usable(p) && owner(p)) {
                    continue;
                }
                let could = pending
                    .iter()
                    .find(|p| owner(p))
                    .map(|p| p.manifest.provider.name.to_string());
                soft[index].push(codes::failure(
                    &codes::row::KIND_OWNER_MISSING,
                    format!(
                        "`{}` reads the kind `{}` of `{}` surfaces, and no enabled provider owns it.",
                        pending[index].manifest.provider.name,
                        codes::shown(&read.kind),
                        read.surface_kind
                    ),
                    match could {
                        Some(owner) => format!("Enable the provider `{owner}`, which owns it."),
                        None => "Install and enable the provider that owns the kind.".to_owned(),
                    },
                ));
            }
        }

        // The snapshot.
        let mut providers = BTreeMap::new();
        let mut modes: BTreeMap<Mode, ModeEntry> = BTreeMap::new();
        let mut surface_kinds = BTreeMap::new();
        let mut readers: BTreeMap<String, Arc<dyn SourceReader>> = BTreeMap::new();
        let mut auth_providers = Vec::new();
        let mut actions = BTreeMap::new();
        let mut discoverers = Vec::new();
        let mut problems: Vec<RegistryProblem> = unnamed
            .iter()
            .map(|refused| RegistryProblem {
                provider: refused
                    .name
                    .as_deref()
                    .and_then(|name| ProviderName::new(name).ok()),
                failure: refused.problem.clone(),
                registered: false,
            })
            .collect();
        let mut described = Vec::new();

        for (index, mut one) in pending.into_iter().enumerate() {
            let name = one.manifest.provider.name.clone();
            let registered = usable(&one);
            let mut all_problems = one.problems.clone();
            all_problems.extend(soft[index].iter().cloned());
            for failure in &one.problems {
                problems.push(RegistryProblem {
                    provider: Some(name.clone()),
                    failure: failure.clone(),
                    registered: false,
                });
            }
            for failure in &soft[index] {
                problems.push(RegistryProblem {
                    provider: Some(name.clone()),
                    failure: failure.clone(),
                    registered,
                });
            }
            let listing = ProviderListing {
                name: name.clone(),
                version: one.manifest.provider.version,
                kind: one.manifest.provider.kind,
                manifest: Arc::clone(&one.manifest),
                availability: one.availability,
                fix: one.fix.clone(),
                registered,
                problems: all_problems,
            };
            // A duplicate name keeps the first listing; both carry the problem.
            providers.entry(name.clone()).or_insert(ProviderEntry {
                listing,
                roots: one.roots.clone(),
                settings: one.settings.clone(),
            });
            let (true, Some(collected), Some(supervisor)) =
                (registered, one.collected.take(), one.supervisor.take())
            else {
                continue;
            };
            let mut line = format!(
                "provider {name} {} {} {}",
                one.manifest.provider.version,
                one.manifest.digest,
                one.availability.as_str()
            );
            for kind in &one.manifest.surface_kinds {
                surface_kinds.insert(
                    kind.name.clone(),
                    SurfaceKindEntry {
                        archetype: kind.archetype,
                        session: kind.session,
                        provider: name.clone(),
                    },
                );
            }
            let codes: BTreeMap<FailureCode, FailureClass> = one
                .manifest
                .failure_codes
                .iter()
                .map(|code| (code.code.clone(), code.class))
                .collect();
            let mut drivers: BTreeMap<Mode, Arc<dyn Driver>> =
                collected.drivers.into_iter().collect();
            for decl in &one.manifest.modes {
                let driver = drivers.remove(&decl.name);
                if driver.is_some() {
                    line.push_str(&format!(" driver:{}", decl.name));
                }
                modes
                    .entry(decl.name.clone())
                    .or_insert_with(|| ModeEntry {
                        archetype: decl.archetype,
                        candidates: Vec::new(),
                    })
                    .candidates
                    .push(Candidate {
                        provider: name.clone(),
                        version: one.manifest.provider.version,
                        digest: one.manifest.digest,
                        availability: one.availability,
                        fix: one.fix.clone(),
                        decl: decl.clone(),
                        codes: codes.clone(),
                        driver,
                        supervisor: supervisor.clone(),
                    });
            }
            let mut names: Vec<String> = Vec::new();
            for (discoverer, describer) in collected.discoverers {
                let found = discoverer.name();
                if let Some(decl) = one.manifest.discoverers.iter().find(|d| d.name == found) {
                    names.push(format!("discoverer:{found}"));
                    discoverers.push(DiscovererHandle {
                        provider: name.clone(),
                        decl: decl.clone(),
                        discoverer,
                        describer,
                    });
                }
            }
            for provider in collected.auth_providers {
                names.push(format!("auth:{}", provider.name().as_str()));
                auth_providers.push(provider);
            }
            for reader in collected.source_readers {
                let reader_name = reader.name().as_str().to_owned();
                if readers.contains_key(&reader_name) {
                    problems.push(RegistryProblem {
                        provider: Some(name.clone()),
                        failure: codes::failure(
                            &codes::row::KIND_OWNER_CONFLICT,
                            format!(
                                "The source reader `{reader_name}` is registered by two providers."
                            ),
                            "Disable one of the two providers.",
                        ),
                        registered: true,
                    });
                    continue;
                }
                names.push(format!("reader:{reader_name}"));
                readers.insert(reader_name, reader);
            }
            for action in collected.actions {
                let action_name = action.name();
                let effect = one
                    .manifest
                    .actions
                    .iter()
                    .find(|own| own.name == action_name)
                    .map_or(EffectClass::Irreversible, |own| own.effect);
                names.push(format!("action:{action_name}"));
                actions.insert(action_name, (action, effect));
            }
            let given = &collected.compiler;
            names.extend(
                given
                    .surface_kinds
                    .iter()
                    .map(|s| format!("surface_kind:{}", s.kind)),
            );
            names.extend(given.controls.iter().map(|c| format!("control:{c}")));
            names.extend(
                given
                    .file_kinds
                    .iter()
                    .map(|k| format!("file_kind:{}", k.name())),
            );
            names.extend(given.lowerings.iter().map(|l| {
                let verbs: Vec<&str> = l.verbs().iter().map(a2m_core::Verb::as_str).collect();
                format!("lowering:{}:{}", l.mode(), verbs.join("+"))
            }));
            names.extend(
                given
                    .lint_rules
                    .iter()
                    .map(|r| format!("lint:{}", r.code())),
            );
            names.extend(
                given
                    .drafting_rules
                    .iter()
                    .map(|r| format!("draft:{}:{}", r.finding_kind().as_str(), r.file_kind())),
            );
            names.sort();
            for registered_name in names {
                line.push(' ');
                line.push_str(&registered_name);
            }
            let mut settings_text = Vec::new();
            one.settings.write_canonical(&mut settings_text);
            line.push_str(&format!(" settings:{}", Digest::of(&settings_text)));
            described.push(line);
        }
        discoverers.sort_by(|a: &DiscovererHandle, b: &DiscovererHandle| {
            (&a.provider, &a.decl.name).cmp(&(&b.provider, &b.decl.name))
        });
        for (mode, provider) in &settings.mode_providers {
            described.push(format!("choice {mode} {provider}"));
        }
        let digest = Digest::of(described.join("\n").as_bytes());

        let registry = Registry {
            inner: Arc::new(Inner {
                digest,
                providers,
                modes,
                surface_kinds,
                readers,
                auth_providers,
                actions,
                discoverers,
                compiler,
                choices: settings.mode_providers,
                host,
                refused: unnamed,
            }),
        };
        (registry, problems)
    }
}

struct ProviderEntry {
    listing: ProviderListing,
    roots: Roots,
    settings: Values,
}

struct Candidate {
    provider: ProviderName,
    version: Version,
    digest: Digest,
    availability: Availability,
    fix: String,
    decl: ModeDecl,
    codes: BTreeMap<FailureCode, FailureClass>,
    driver: Option<Arc<dyn Driver>>,
    supervisor: ScopedSupervisor,
}

struct ModeEntry {
    archetype: Archetype,
    candidates: Vec<Candidate>,
}

struct Inner {
    digest: Digest,
    providers: BTreeMap<ProviderName, ProviderEntry>,
    modes: BTreeMap<Mode, ModeEntry>,
    surface_kinds: BTreeMap<SurfaceKind, SurfaceKindEntry>,
    readers: BTreeMap<String, Arc<dyn SourceReader>>,
    auth_providers: Vec<Arc<dyn AuthProvider>>,
    actions: BTreeMap<ActionName, (Arc<dyn ProviderAction>, EffectClass)>,
    discoverers: Vec<DiscovererHandle>,
    compiler: CompilerSet,
    choices: BTreeMap<Mode, ProviderName>,
    host: HostServices,
    refused: Vec<RefusedProvider>,
}

/// A provider whose manifest was refused, so that nothing of it is
/// registered and it has no listing: kept so that `provider.list` names it
/// with its problem, never leaving it out unseen. Its name and version are
/// read from the manifest's text as far as they can be, and are the
/// manifest's own words, unchecked. A name that another provider holds,
/// listed or refused before it, is not kept: such a provider is unnamed,
/// and its problem is never listed under another's name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefusedProvider {
    /// The name its manifest gives, when the text says one.
    pub name: Option<String>,
    /// The version its manifest gives, when the text says one.
    pub version: Option<String>,
    /// A pack compiled in, or a plug-in found on the machine.
    pub kind: ProviderKind,
    /// Why its manifest was refused.
    pub problem: Failure,
}

impl RefusedProvider {
    fn of(text: &str, kind: ProviderKind, problem: Failure) -> Self {
        let table = toml::from_str::<toml::Table>(text).ok();
        let said = |field: &str| {
            table
                .as_ref()
                .and_then(|table| table.get("provider"))
                .and_then(|provider| provider.get(field))
                .and_then(toml::Value::as_str)
                .filter(|text| text.len() <= 64 && text.chars().all(|c| c.is_ascii_graphic()))
                .map(str::to_owned)
        };
        Self {
            name: said("name"),
            version: said("version"),
            kind,
            problem,
        }
    }
}

/// One immutable snapshot of everything that is registered. Cheap to
/// clone; replaced whole, never changed.
#[derive(Clone)]
pub struct Registry {
    inner: Arc<Inner>,
}

impl fmt::Debug for Registry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Registry")
            .field("digest", &self.inner.digest)
            .field(
                "providers",
                &self.inner.providers.keys().collect::<Vec<_>>(),
            )
            .finish_non_exhaustive()
    }
}

impl Registry {
    /// The digest of what is registered: every registered provider with
    /// its version, manifest, availability, objects, and settings, and
    /// every choice a setting makes. It keys the tool-list cache and is
    /// recorded in every trace.
    #[must_use]
    pub fn digest(&self) -> Digest {
        self.inner.digest
    }

    /// The archetype of a registered mode.
    #[must_use]
    pub fn archetype(&self, mode: &Mode) -> Option<Archetype> {
        self.inner.modes.get(mode).map(|entry| entry.archetype)
    }

    /// Every registered mode, in order of name.
    pub fn modes(&self) -> impl Iterator<Item = &Mode> {
        self.inner.modes.keys()
    }

    /// A registered surface kind.
    #[must_use]
    pub fn surface_kind(&self, kind: &SurfaceKind) -> Option<&SurfaceKindEntry> {
        self.inner.surface_kinds.get(kind)
    }

    /// The candidate that drives a mode, with the reason it is the one.
    fn chosen(&self, mode: &Mode) -> Result<(&ModeEntry, &Candidate, ChoiceReason), Failure> {
        let entry = self.inner.modes.get(mode).ok_or_else(|| {
            codes::failure(
                &codes::row::MODE_UNKNOWN,
                format!("No enabled provider registered the mode `{mode}`."),
                "Enable the provider that brings the mode; `anything2mcp provider list` shows them.",
            )
        })?;
        let drivers: Vec<&Candidate> = entry
            .candidates
            .iter()
            .filter(|candidate| candidate.driver.is_some())
            .collect();
        let unavailable = |candidate: &Candidate| {
            codes::failure(
                &codes::row::PROVIDER_UNAVAILABLE,
                format!(
                    "The provider `{}`, which drives the mode `{mode}`, is `{}` here.",
                    candidate.provider,
                    candidate.availability.as_str()
                ),
                if candidate.fix.is_empty() {
                    "Run `anything2mcp doctor` to see what the provider needs.".to_owned()
                } else {
                    candidate.fix.clone()
                },
            )
        };
        if let Some(named) = self.inner.choices.get(mode) {
            let candidate = drivers
                .iter()
                .find(|candidate| candidate.provider == *named)
                .ok_or_else(|| {
                    codes::failure(
                        &codes::row::PROVIDER_UNAVAILABLE,
                        format!(
                            "The settings name `{named}` for the mode `{mode}`, and it registered no driver for it."
                        ),
                        "Enable that provider, or name another for the mode.",
                    )
                })?;
            return if candidate.availability.is_ready() {
                Ok((entry, candidate, ChoiceReason::NamedBySetting))
            } else {
                // Never the other one: there is no silent fallback.
                Err(unavailable(candidate))
            };
        }
        let ready: Vec<&&Candidate> = drivers
            .iter()
            .filter(|candidate| candidate.availability.is_ready())
            .collect();
        match (ready.as_slice(), drivers.as_slice()) {
            ([one], _) => Ok((entry, one, ChoiceReason::OnlyProvider)),
            ([], []) => Err(codes::failure(
                &codes::row::MODE_UNKNOWN,
                format!("The mode `{mode}` is registered and no enabled provider drives it."),
                "Enable a provider that drives the mode.",
            )),
            ([], [first, ..]) => Err(unavailable(first)),
            _ => {
                let listed: Vec<String> = drivers
                    .iter()
                    .map(|candidate| {
                        format!(
                            "{} ({})",
                            candidate.provider,
                            candidate.availability.as_str()
                        )
                    })
                    .collect();
                Err(codes::failure(
                    &codes::row::PROVIDER_AMBIGUOUS,
                    format!(
                        "More than one ready provider drives the mode `{mode}`: {}.",
                        listed.join(", ")
                    ),
                    "Name the provider for the mode in the settings.",
                ))
            }
        }
    }

    /// The defaults of a mode, from the provider that drives it; when that
    /// choice cannot be made, from the first provider by name that
    /// declares the mode.
    #[must_use]
    pub fn mode_defaults(&self, mode: &Mode) -> Option<ModeDefaults> {
        match self.chosen(mode) {
            Ok((_, candidate, _)) => Some(candidate.decl.defaults()),
            Err(_) => self
                .inner
                .modes
                .get(mode)
                .and_then(|entry| entry.candidates.first())
                .map(|candidate| candidate.decl.defaults()),
        }
    }

    /// The driver for a mode, with the provider choice already made and
    /// its reason attached. There is no priority and no silent fallback.
    ///
    /// # Errors
    ///
    /// `input`/`mode_unknown` when no enabled provider drives the mode;
    /// `input`/`provider_ambiguous` when more than one ready provider does
    /// and no setting names one; `busy`/`provider_unavailable` when the one
    /// that is named, or the only one, is not ready.
    pub fn driver(&self, mode: &Mode) -> Result<DriverHandle, Failure> {
        let (entry, candidate, reason) = self.chosen(mode)?;
        let Some(driver) = &candidate.driver else {
            return Err(codes::failure(
                &codes::row::MODE_UNKNOWN,
                format!("No enabled provider drives the mode `{mode}`."),
                "Enable a provider that drives the mode.",
            ));
        };
        let facts = HandleFacts {
            mode: mode.clone(),
            archetype: entry.archetype,
            steps: candidate.decl.steps.iter().cloned().collect(),
            commit_steps: candidate.decl.commit_steps.iter().cloned().collect(),
            absence_steps: candidate.decl.absence_steps.iter().cloned().collect(),
            codes: candidate.codes.clone(),
            choice: ProviderChoice {
                provider: candidate.provider.clone(),
                version: candidate.version,
                digest: candidate.digest,
                reason,
            },
            supervisor: candidate.supervisor.clone(),
        };
        Ok(DriverHandle::new(
            Arc::clone(driver),
            Arc::new(facts),
            Arc::clone(&self.inner.host.redact),
        ))
    }

    /// A handle on `driver` in place of the driver of the provider chosen
    /// for `mode`, with that provider's facts: its step kinds, the kinds that
    /// commit, its failure codes, and its choice. The handle checks every
    /// step against them as it would the provider's own driver. For an
    /// offline test, whose driver is a [`crate::Replay`] of recorded
    /// answers: nothing of the provider's own driver is used.
    ///
    /// # Errors
    ///
    /// As [`Registry::driver`]: `input`/`mode_unknown`,
    /// `input`/`provider_ambiguous`.
    pub fn replay_driver(
        &self,
        mode: &Mode,
        driver: Arc<dyn Driver>,
    ) -> Result<DriverHandle, Failure> {
        let (entry, candidate, reason) = self.chosen(mode)?;
        let facts = HandleFacts {
            mode: mode.clone(),
            archetype: entry.archetype,
            steps: candidate.decl.steps.iter().cloned().collect(),
            commit_steps: candidate.decl.commit_steps.iter().cloned().collect(),
            absence_steps: candidate.decl.absence_steps.iter().cloned().collect(),
            codes: candidate.codes.clone(),
            choice: ProviderChoice {
                provider: candidate.provider.clone(),
                version: candidate.version,
                digest: candidate.digest,
                reason,
            },
            supervisor: candidate.supervisor.clone(),
        };
        Ok(DriverHandle::new(
            driver,
            Arc::new(facts),
            Arc::clone(&self.inner.host.redact),
        ))
    }

    /// The reader a provider registered under a name. The built-in readers
    /// are `a2m-sources`'s own and are not here.
    ///
    /// # Errors
    ///
    /// `input`/`not_registered`.
    pub fn source_reader(&self, name: &ReaderName) -> Result<Arc<dyn SourceReader>, Failure> {
        self.inner
            .readers
            .get(name.as_str())
            .cloned()
            .ok_or_else(|| {
                codes::failure(
                    &codes::row::NOT_REGISTERED,
                    format!(
                        "No enabled provider registered the source reader `{}`.",
                        name.as_str()
                    ),
                    "Enable the provider that brings the reader.",
                )
            })
    }

    /// Every source reader providers registered, in order of name.
    #[must_use]
    pub fn source_readers(&self) -> Vec<Arc<dyn SourceReader>> {
        self.inner.readers.values().cloned().collect()
    }

    /// The discoverers that apply to a surface, in order of provider and
    /// name.
    #[must_use]
    pub fn discoverers_for(&self, surface: &SurfaceDescription) -> Vec<DiscovererHandle> {
        self.inner
            .discoverers
            .iter()
            .filter(|handle| handle.suits(surface))
            .cloned()
            .collect()
    }

    /// The names under which the discoverers that apply to a surface read
    /// files a person hands a discovery, each once: what a discovery of the
    /// surface may be handed, and nothing else (decision 0263). Empty when
    /// no discoverer of the surface reads a file.
    #[must_use]
    pub fn files_taken(&self, surface: &SurfaceDescription) -> BTreeSet<String> {
        self.inner
            .discoverers
            .iter()
            .filter(|handle| handle.suits(surface))
            .flat_map(|handle| handle.decl.files.iter().cloned())
            .collect()
    }

    /// The auth providers packs registered, for the keeper.
    #[must_use]
    pub fn auth_providers(&self) -> Vec<Arc<dyn AuthProvider>> {
        self.inner.auth_providers.clone()
    }

    /// Everything packs registered with the compiler.
    #[must_use]
    pub fn compiler_set(&self) -> &CompilerSet {
        &self.inner.compiler
    }

    /// An action a provider added, with the effect class its manifest
    /// declares, for the dispatcher.
    #[must_use]
    pub fn action(&self, name: &ActionName) -> Option<(Arc<dyn ProviderAction>, EffectClass)> {
        self.inner.actions.get(name).cloned()
    }

    /// Every action providers added, in order of name.
    pub fn actions(&self) -> impl Iterator<Item = &ActionName> {
        self.inner.actions.keys()
    }

    /// The provider that owns a kind for a surface kind, among those
    /// registered.
    #[must_use]
    pub fn kind_owner(
        &self,
        of: KindOf,
        kind: &str,
        surface_kind: &SurfaceKind,
    ) -> Option<&ProviderName> {
        self.inner
            .providers
            .values()
            .find(|entry| {
                entry.listing.registered && entry.listing.manifest.owns(of, kind, surface_kind)
            })
            .map(|entry| &entry.listing.name)
    }

    /// Every provider whose manifest was refused, with why, in the order
    /// they were added. Nothing of them is registered.
    #[must_use]
    pub fn refused(&self) -> &[RefusedProvider] {
        &self.inner.refused
    }

    /// Every provider: what it declares, whether it can work here, and
    /// what is wrong with it. In order of name. A provider whose manifest
    /// was refused has no listing; see [`Registry::refused`].
    #[must_use]
    pub fn listing(&self) -> Vec<ProviderListing> {
        self.inner
            .providers
            .values()
            .map(|entry| entry.listing.clone())
            .collect()
    }

    /// The roots a provider's manifest declares, for the dispatcher of its
    /// actions.
    #[must_use]
    pub fn provider_roots(&self, provider: &ProviderName) -> Option<&Roots> {
        self.inner.providers.get(provider).map(|entry| &entry.roots)
    }

    /// A pack's validated settings group, for `config.show`.
    #[must_use]
    pub fn provider_settings(&self, provider: &ProviderName) -> Option<&Values> {
        self.inner
            .providers
            .get(provider)
            .map(|entry| &entry.settings)
    }

    /// The supervisor, for `doctor.run` and for stopping everything.
    #[must_use]
    pub fn supervisor(&self) -> &Supervisor {
        &self.inner.host.supervisor
    }

    /// Every named root, for the kernel's own reads.
    #[must_use]
    pub fn roots(&self) -> &Roots {
        &self.inner.host.roots
    }
}
