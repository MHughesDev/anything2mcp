//! The publish review summary: what each tool of a version may do, what it
//! starts, what it sets in a program's environment, where it sends requests,
//! and how each surface is signed in to, for the person who publishes it
//! (spec sections 403 and 502; decision 0252).
//!
//! Every field is typed, so a page renders it and never prints a sentence
//! built from a system's or an agent's words. The only sentences are this
//! module's constants. What a pack does not state is said in so many words,
//! and never left as an empty list that would read as "nothing".

use std::collections::{BTreeMap, BTreeSet};

use a2m_core::{
    AddressScope, AuthProviderName, CredentialRef, DeclaredProgram, Digest, EffectClass,
    Environment, Hazard, Mode, OperationName, Plan, ProbePlan, ScopedHost, SignInMode,
    SurfaceIndex, SurfaceName,
};
use serde::Serialize;

/// What the summary says of a binding whose pack does not state what it
/// starts, sets, or sends to.
pub const NOT_STATED: &str = "this kind of tool does not state what it runs";
/// What the summary says beside a script a target keeps (decision 0216).
pub const SCRIPT_NOTICE: &str =
    "runs a script this target keeps: the digest pins its bytes; it does not make it safe";
/// What the summary says of a tool's reach when a binding's pack does not
/// state where it sends requests.
pub const REACH_NOT_STATED: &str = "where this kind of tool sends requests is not stated: only the addresses shown are known, and it may reach others";
/// What the summary says of a sign-in's destinations before the session
/// keeper has stated them.
pub const SENT_TO_NOT_STATED: &str =
    "where a credential or a token of this sign-in is sent has not been stated";
/// What the summary says of a sign-in whose provider this engine does not
/// hold.
pub const PROVIDER_NOT_HELD: &str = "the provider is not held by this engine, so where it would send is not known; it signs in nowhere until a pack that provides it is enabled";
/// What the summary says of a sign-in whose probe is named and not yet
/// drafted.
pub const PROBE_PENDING: &str = "the read that checks this sign-in is not drafted yet: a session it makes is not confirmed until that read exists and passes, and no version can be published with it";

// ---- What a pack states about one binding ----

/// What a lowering states about one binding for the review summary
/// (`Lowering::statement`). A lowering that does not answer says nothing,
/// and the summary says so.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Statement {
    /// The pack does not state what the binding starts or where it sends.
    #[default]
    NotStated,
    /// What the binding starts, sets, and sends to, all of it.
    Stated(BindingStatement),
}

/// Everything one binding starts, sets in a program's environment, and
/// sends requests to, as the target's files declare it. A list left empty
/// says "none": a pack that cannot say gives [`Statement::NotStated`].
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct BindingStatement {
    /// Each program the binding starts.
    pub runs: Vec<ProgramRun>,
    /// Each variable it sets or passes to a program it starts.
    pub environment: Vec<EnvironmentVariable>,
    /// Where it sends requests. The default is that it does not say, which
    /// is the honest answer for a program the pack starts: what a program
    /// reaches is the program's.
    pub sends: Sends,
}

/// Where a binding sends requests.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
#[serde(tag = "sends", rename_all = "snake_case")]
pub enum Sends {
    /// The pack cannot say: what the binding starts may reach any address.
    #[default]
    NotStated,
    /// Only to these, and to no other address.
    Only {
        /// Whether to its surface's address, in each environment of the
        /// target.
        surface: bool,
        /// The other addresses, as the target's files name them: a URL, a
        /// host and port, or a host.
        addresses: Vec<String>,
    },
}

/// A program a binding starts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProgramRun {
    /// A program, by its absolute path and the digests it is pinned to.
    Program {
        /// Its absolute path, as the target's file wrote it.
        path: String,
        /// The digests its bytes must have; empty where the file pins none.
        digests: Vec<Digest>,
    },
    /// A fixed pair: an interpreter, its fixed arguments, and a script the
    /// target keeps (decision 0216).
    Script {
        /// The interpreter's absolute path.
        interpreter: String,
        /// The digests the interpreter is pinned to; empty where none.
        interpreter_digests: Vec<Digest>,
        /// The arguments it is always given before the script's path.
        arguments: Vec<String>,
        /// The script's path inside the target's folder.
        script: String,
        /// The digest of the script's bytes.
        script_digest: Digest,
    },
}

/// One variable a binding sets or passes to a program it starts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EnvironmentVariable {
    /// Its name.
    pub name: String,
    /// Its value, or why it is not shown.
    pub value: VariableValue,
}

/// The value of a variable, as the review shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum VariableValue {
    /// Set to this value, which the target's file states.
    Set(String),
    /// Set to a value marked secret, which is never shown.
    Secret,
    /// Copied from the engine's own environment when it is set there.
    Passed,
}

// ---- The summary ----

/// Every tool of a version, the most dangerous first, and every sign-in.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct ReviewSummary {
    /// The tools, ordered by risk: by hazard, then effect class, then what
    /// they expose (an unknown reach, then a reach off the machine, then
    /// what they start), then name.
    pub items: Vec<ReviewItem>,
    /// Each entry of `auth.yaml`: a probe not yet drafted first, then in the
    /// file's order. A page shows these before the tools: a credential sent
    /// to an address is a risk to every tool of the surfaces it covers.
    pub sign_ins: Vec<SignInReview>,
}

/// What one tool may do, for the person who reviews a version before it is
/// published.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReviewItem {
    /// The tool.
    pub tool: OperationName,
    /// Its effect class.
    pub effect: EffectClass,
    /// What it could damage beyond the record it names.
    pub hazard: Hazard,
    /// How many times the system saves in one call; none for a pipeline.
    pub commit_points: u32,
    /// The surfaces it may reach.
    pub surfaces: Vec<SurfaceName>,
    /// Addresses beyond its surfaces that it may reach.
    pub destinations: Vec<String>,
    /// What it may send as a file.
    pub uploads: Vec<String>,
    /// What it may receive as a file.
    pub downloads: Vec<String>,
    /// The one fixed place its record is, where it has one.
    pub place: Option<PlaceReview>,
    /// Each of its bindings: what it starts and sets, as its pack states it.
    /// A pipeline's are those of the operations its steps call.
    pub bindings: Vec<BindingReview>,
    /// Every address it can send a request to, by the kind of place.
    pub reach: Reach,
}

/// The fixed place a tool's record is (decision 0252).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlaceReview {
    /// The surface it is on.
    pub surface: SurfaceName,
    /// The file kind that declares it.
    pub kind: a2m_core::FileKindName,
    /// Its name in the target's files.
    pub name: String,
}

/// One binding of a tool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BindingReview {
    /// The operation it carries out.
    pub operation: OperationName,
    /// Its mode.
    pub mode: Mode,
    /// The surface it acts on.
    pub surface: SurfaceName,
    /// The programs the surface declares, which its driver may start, as
    /// the kernel reads them from the target's files.
    pub surface_programs: Vec<DeclaredProgram>,
    /// What its pack states.
    pub states: BindingStates,
}

/// What a pack states about one binding, as the summary shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "stated", rename_all = "snake_case")]
pub enum BindingStates {
    /// The pack states nothing; `says` is [`NOT_STATED`].
    NotStated {
        /// The fixed sentence.
        says: &'static str,
    },
    /// What the binding starts and what it sets.
    Stated {
        /// Each program it starts; empty means it starts none.
        runs: Vec<RunReview>,
        /// Each variable it sets or passes; empty means none.
        environment: Vec<EnvironmentVariable>,
    },
}

/// A program a binding starts, with the fixed notice a script carries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RunReview {
    /// The program.
    pub run: ProgramRun,
    /// [`SCRIPT_NOTICE`] for a script the target keeps; none for a program.
    pub notice: Option<&'static str>,
}

/// The addresses of one kind of place.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReachGroup {
    /// What kind of place the addresses' literals say they are.
    pub scope: AddressScope,
    /// The addresses, sorted, each once.
    pub addresses: Vec<String>,
}

/// Every address a tool can send a request to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "reach", rename_all = "snake_case")]
pub enum Reach {
    /// Every address, stated by every binding's pack and the tool's
    /// permissions. Empty groups mean it sends no request to any address.
    Stated {
        /// By kind of place, the riskiest first.
        groups: Vec<ReachGroup>,
        /// Whether any of them may leave this machine.
        leaves_this_machine: bool,
    },
    /// A binding's pack does not state where it sends; `says` is
    /// [`REACH_NOT_STATED`].
    Partly {
        /// The addresses that are known, by kind of place.
        known: Vec<ReachGroup>,
        /// The fixed sentence.
        says: &'static str,
    },
}

/// One entry of `auth.yaml`, for the person who publishes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SignInReview {
    /// The entry's place in the plan's `auth`, which
    /// [`ReviewSummary::state_sent_to`] names.
    pub entry: usize,
    /// The surfaces it signs in to.
    pub surfaces: Vec<SurfaceName>,
    /// The auth provider.
    pub provider: AuthProviderName,
    /// How the sign-in comes to exist.
    pub mode: SignInMode,
    /// Each stored credential, by role and reference. Never a value.
    pub credentials: Vec<CredentialShown>,
    /// The read that checks the session.
    pub probe: ProbeReview,
    /// For each environment of the target: every address a credential or a
    /// token of it is sent to.
    pub sent_to: Vec<SentToReview>,
}

/// A stored credential, by name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CredentialShown {
    /// The role the provider gives it, such as `password`.
    pub role: String,
    /// The vault reference.
    pub reference: CredentialRef,
}

/// The read that checks a session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "probe", rename_all = "snake_case")]
pub enum ProbeReview {
    /// A probe that compiles.
    Declared {
        /// Its name: an operation, or an entry of a pack's file.
        name: String,
    },
    /// A probe named and not drafted yet, in a draft only.
    Pending {
        /// The name the entry gives it.
        name: String,
        /// [`PROBE_PENDING`].
        says: &'static str,
    },
}

/// Where a sign-in sends in one environment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SentToReview {
    /// The environment.
    pub environment: Environment,
    /// Where.
    pub sent_to: SentTo,
}

/// Where a credential or a token of a sign-in is sent, in one environment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "sent_to", rename_all = "snake_case")]
pub enum SentTo {
    /// Not stated yet: the compiler cannot know where a provider sends.
    /// `says` is [`SENT_TO_NOT_STATED`].
    NotStated {
        /// The fixed sentence.
        says: &'static str,
    },
    /// The provider is not held; `says` is [`PROVIDER_NOT_HELD`].
    ProviderNotHeld {
        /// The fixed sentence.
        says: &'static str,
    },
    /// Every address, and no other.
    Addresses {
        /// By kind of place, the riskiest first.
        groups: Vec<SentGroup>,
        /// Whether any of them may leave this machine.
        leaves_this_machine: bool,
    },
}

/// The addresses of one kind of place that a sign-in sends to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SentGroup {
    /// What kind of place.
    pub scope: AddressScope,
    /// The addresses, sorted by address.
    pub addresses: Vec<SentAddress>,
}

/// One address a sign-in sends to, and what is sent there.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct SentAddress {
    /// The address.
    pub address: String,
    /// What goes there, in the provider's fixed words, such as "the
    /// sign-in form".
    pub what: String,
}

impl ReviewSummary {
    /// The summary with every text it holds passed through `f`: each
    /// address, path, argument, variable's name and value, place, file
    /// name, credential's role, probe's name, and what a sign-in sends
    /// where. A caller passes the redactor, so that no secret a target's
    /// file happens to quote reaches a page. The fixed sentences are this
    /// module's constants and stay as they are; checked names (tools,
    /// surfaces, modes, providers, environments, references) and digests
    /// are not text a target can fill with anything, and stay too.
    #[must_use]
    pub fn map_text(&self, f: &dyn Fn(&str) -> String) -> Self {
        let all = |texts: &[String]| -> Vec<String> { texts.iter().map(|text| f(text)).collect() };
        let variable = |variable: &EnvironmentVariable| EnvironmentVariable {
            name: f(&variable.name),
            value: match &variable.value {
                VariableValue::Set(value) => VariableValue::Set(f(value)),
                VariableValue::Secret => VariableValue::Secret,
                VariableValue::Passed => VariableValue::Passed,
            },
        };
        let reach_groups = |groups: &[ReachGroup]| -> Vec<ReachGroup> {
            groups
                .iter()
                .map(|group| ReachGroup {
                    scope: group.scope,
                    addresses: all(&group.addresses),
                })
                .collect()
        };
        let run = |run: &ProgramRun| match run {
            ProgramRun::Program { path, digests } => ProgramRun::Program {
                path: f(path),
                digests: digests.clone(),
            },
            ProgramRun::Script {
                interpreter,
                interpreter_digests,
                arguments,
                script,
                script_digest,
            } => ProgramRun::Script {
                interpreter: f(interpreter),
                interpreter_digests: interpreter_digests.clone(),
                arguments: all(arguments),
                script: f(script),
                script_digest: *script_digest,
            },
        };
        let items = self
            .items
            .iter()
            .map(|item| ReviewItem {
                tool: item.tool.clone(),
                effect: item.effect,
                hazard: item.hazard,
                commit_points: item.commit_points,
                surfaces: item.surfaces.clone(),
                destinations: all(&item.destinations),
                uploads: all(&item.uploads),
                downloads: all(&item.downloads),
                place: item.place.as_ref().map(|place| PlaceReview {
                    surface: place.surface.clone(),
                    kind: place.kind.clone(),
                    name: f(&place.name),
                }),
                bindings: item
                    .bindings
                    .iter()
                    .map(|binding| BindingReview {
                        operation: binding.operation.clone(),
                        mode: binding.mode.clone(),
                        surface: binding.surface.clone(),
                        surface_programs: binding
                            .surface_programs
                            .iter()
                            .map(|program| DeclaredProgram {
                                path: f(&program.path),
                                digest: program.digest,
                            })
                            .collect(),
                        states: match &binding.states {
                            BindingStates::NotStated { says } => BindingStates::NotStated { says },
                            BindingStates::Stated { runs, environment } => BindingStates::Stated {
                                runs: runs
                                    .iter()
                                    .map(|one| RunReview {
                                        run: run(&one.run),
                                        notice: one.notice,
                                    })
                                    .collect(),
                                environment: environment.iter().map(variable).collect(),
                            },
                        },
                    })
                    .collect(),
                reach: match &item.reach {
                    Reach::Stated {
                        groups,
                        leaves_this_machine,
                    } => Reach::Stated {
                        groups: reach_groups(groups),
                        leaves_this_machine: *leaves_this_machine,
                    },
                    Reach::Partly { known, says } => Reach::Partly {
                        known: reach_groups(known),
                        says,
                    },
                },
            })
            .collect();
        let sign_ins = self
            .sign_ins
            .iter()
            .map(|sign_in| SignInReview {
                entry: sign_in.entry,
                surfaces: sign_in.surfaces.clone(),
                provider: sign_in.provider.clone(),
                mode: sign_in.mode,
                credentials: sign_in
                    .credentials
                    .iter()
                    .map(|credential| CredentialShown {
                        role: f(&credential.role),
                        reference: credential.reference.clone(),
                    })
                    .collect(),
                probe: match &sign_in.probe {
                    ProbeReview::Declared { name } => ProbeReview::Declared { name: f(name) },
                    ProbeReview::Pending { name, says } => ProbeReview::Pending {
                        name: f(name),
                        says,
                    },
                },
                sent_to: sign_in
                    .sent_to
                    .iter()
                    .map(|sent_to| SentToReview {
                        environment: sent_to.environment.clone(),
                        sent_to: match &sent_to.sent_to {
                            SentTo::Addresses {
                                groups,
                                leaves_this_machine,
                            } => SentTo::Addresses {
                                groups: groups
                                    .iter()
                                    .map(|group| SentGroup {
                                        scope: group.scope,
                                        addresses: group
                                            .addresses
                                            .iter()
                                            .map(|sent| SentAddress {
                                                address: f(&sent.address),
                                                what: f(&sent.what),
                                            })
                                            .collect(),
                                    })
                                    .collect(),
                                leaves_this_machine: *leaves_this_machine,
                            },
                            other => other.clone(),
                        },
                    })
                    .collect(),
            })
            .collect();
        Self { items, sign_ins }
    }

    /// States where a sign-in sends in one environment: the addresses the
    /// session keeper gives, or `None` when the provider is not held. The
    /// caller is `a2m-actions`, which asks `a2m-auth`; the compiler cannot
    /// know. Returns false, and changes nothing, when the entry or the
    /// environment is not one of the summary's.
    pub fn state_sent_to(
        &mut self,
        entry: usize,
        environment: &Environment,
        sent: Option<Vec<SentAddress>>,
    ) -> bool {
        let Some(slot) = self
            .sign_ins
            .iter_mut()
            .find(|sign_in| sign_in.entry == entry)
            .and_then(|sign_in| {
                sign_in
                    .sent_to
                    .iter_mut()
                    .find(|sent_to| &sent_to.environment == environment)
            })
        else {
            return false;
        };
        slot.sent_to = match sent {
            None => SentTo::ProviderNotHeld {
                says: PROVIDER_NOT_HELD,
            },
            Some(addresses) => {
                let leaves = addresses.iter().any(|sent| leaves(&sent.address));
                let mut grouped: BTreeMap<u8, (AddressScope, BTreeSet<SentAddress>)> =
                    BTreeMap::new();
                for sent in addresses {
                    let scope = ScopedHost::of_destination(&sent.address).scope();
                    grouped
                        .entry(scope_rank(scope))
                        .or_insert_with(|| (scope, BTreeSet::new()))
                        .1
                        .insert(sent);
                }
                SentTo::Addresses {
                    groups: grouped
                        .into_values()
                        .map(|(scope, addresses)| SentGroup {
                            scope,
                            addresses: addresses.into_iter().collect(),
                        })
                        .collect(),
                    leaves_this_machine: leaves,
                }
            }
        };
        true
    }
}

/// The order of kinds of place in the summary, the riskiest first: one
/// that cannot be read, then one anyone can reach, then a name, which may
/// lead anywhere (`localhost` included, decision 0188), then the ranges
/// that stay nearer, and this machine last.
fn scope_rank(scope: AddressScope) -> u8 {
    match scope {
        AddressScope::Unreadable => 0,
        AddressScope::Public => 1,
        AddressScope::Name => 2,
        AddressScope::Unspecified => 3,
        AddressScope::Multicast => 4,
        AddressScope::CarrierGradeNat => 5,
        AddressScope::PrivateUse => 6,
        AddressScope::UniqueLocal => 7,
        AddressScope::LinkLocal => 8,
        AddressScope::Reserved => 9,
        AddressScope::Documentation => 10,
        AddressScope::ThisMachine => 11,
    }
}

fn leaves(address: &str) -> bool {
    ScopedHost::of_destination(address).may_leave_this_machine()
}

fn groups(addresses: &BTreeSet<String>) -> Vec<ReachGroup> {
    let mut grouped: BTreeMap<u8, (AddressScope, Vec<String>)> = BTreeMap::new();
    for address in addresses {
        let scope = ScopedHost::of_destination(address).scope();
        grouped
            .entry(scope_rank(scope))
            .or_insert_with(|| (scope, Vec::new()))
            .1
            .push(address.clone());
    }
    grouped
        .into_values()
        .map(|(scope, addresses)| ReachGroup { scope, addresses })
        .collect()
}

/// What the compiler learned of one operation while lowering it, beyond
/// its plan: the fixed place, and what each binding's pack stated.
#[derive(Debug, Clone, Default)]
pub(crate) struct OpReview {
    pub place: Option<PlaceReview>,
    pub bindings: Vec<(Mode, SurfaceIndex, Statement)>,
}

/// The summary of a compiled plan.
pub(crate) fn review_of(plan: &Plan, reviews: &BTreeMap<OperationName, OpReview>) -> ReviewSummary {
    let surface_names = |indices: &[SurfaceIndex]| -> Vec<SurfaceName> {
        indices
            .iter()
            .filter_map(|index| plan.surface(*index).map(|surface| surface.name.clone()))
            .collect()
    };
    // A surface's address in every environment.
    let surface_addresses = |index: SurfaceIndex| -> Vec<String> {
        let mut addresses: Vec<String> = plan
            .surface(index)
            .map(|surface| surface.address.clone())
            .into_iter()
            .collect();
        for environment in &plan.environments {
            for address in &environment.addresses {
                if address.surface == index {
                    addresses.push(address.address.clone());
                }
            }
        }
        addresses
    };
    // The bindings and the reach of a list of operations, with their
    // permissions' destinations.
    let reviewed = |operations: &[&a2m_core::OperationPlan]| -> (Vec<BindingReview>, Reach) {
        let mut bindings = Vec::new();
        let mut addresses: BTreeSet<String> = BTreeSet::new();
        let mut partly = false;
        for operation in operations {
            addresses.extend(operation.manifest.destinations.iter().cloned());
            let Some(review) = reviews.get(&operation.name) else {
                partly = true;
                continue;
            };
            for (mode, surface, statement) in &review.bindings {
                let Some(plan_surface) = plan.surface(*surface) else {
                    continue;
                };
                let states = match statement {
                    Statement::NotStated => {
                        partly = true;
                        BindingStates::NotStated { says: NOT_STATED }
                    }
                    Statement::Stated(stated) => {
                        match &stated.sends {
                            Sends::NotStated => partly = true,
                            Sends::Only {
                                surface: to_surface,
                                addresses: others,
                            } => {
                                if *to_surface {
                                    addresses.extend(surface_addresses(*surface));
                                }
                                addresses.extend(others.iter().cloned());
                            }
                        }
                        BindingStates::Stated {
                            runs: stated
                                .runs
                                .iter()
                                .map(|run| RunReview {
                                    run: run.clone(),
                                    notice: matches!(run, ProgramRun::Script { .. })
                                        .then_some(SCRIPT_NOTICE),
                                })
                                .collect(),
                            environment: stated.environment.clone(),
                        }
                    }
                };
                bindings.push(BindingReview {
                    operation: operation.name.clone(),
                    mode: mode.clone(),
                    surface: plan_surface.name.clone(),
                    surface_programs: plan_surface.programs.clone(),
                    states,
                });
            }
        }
        let known = groups(&addresses);
        let reach = if partly {
            Reach::Partly {
                known,
                says: REACH_NOT_STATED,
            }
        } else {
            Reach::Stated {
                groups: known,
                leaves_this_machine: addresses.iter().any(|address| leaves(address)),
            }
        };
        (bindings, reach)
    };
    let mut items: Vec<ReviewItem> = plan
        .operations
        .iter()
        .map(|operation| {
            let (bindings, reach) = reviewed(&[operation]);
            ReviewItem {
                tool: operation.name.clone(),
                effect: operation.effect,
                hazard: operation.hazard,
                commit_points: operation.commit_points,
                surfaces: surface_names(&operation.manifest.surfaces),
                destinations: operation.manifest.destinations.clone(),
                uploads: operation.manifest.uploads.clone(),
                downloads: operation.manifest.downloads.clone(),
                place: reviews
                    .get(&operation.name)
                    .and_then(|review| review.place.clone()),
                bindings,
                reach,
            }
        })
        .collect();
    for pipeline in &plan.pipelines {
        let mut surfaces = BTreeSet::new();
        let mut destinations = BTreeSet::new();
        let mut uploads = BTreeSet::new();
        let mut downloads = BTreeSet::new();
        let mut called: Vec<&a2m_core::OperationPlan> = Vec::new();
        for step in &pipeline.steps {
            if let Some(operation) = plan.operation(step.operation) {
                surfaces.extend(operation.manifest.surfaces.iter().copied());
                destinations.extend(operation.manifest.destinations.iter().cloned());
                uploads.extend(operation.manifest.uploads.iter().cloned());
                downloads.extend(operation.manifest.downloads.iter().cloned());
                if !called.iter().any(|known| known.name == operation.name) {
                    called.push(operation);
                }
            }
        }
        let surfaces: Vec<SurfaceIndex> = surfaces.into_iter().collect();
        let (bindings, reach) = reviewed(&called);
        items.push(ReviewItem {
            tool: pipeline.name.clone(),
            effect: pipeline.effect,
            hazard: pipeline.hazard,
            commit_points: 0,
            surfaces: surface_names(&surfaces),
            destinations: destinations.into_iter().collect(),
            uploads: uploads.into_iter().collect(),
            downloads: downloads.into_iter().collect(),
            place: None,
            bindings,
            reach,
        });
    }
    items.sort_by(|a, b| {
        (b.hazard, b.effect, exposure(b))
            .cmp(&(a.hazard, a.effect, exposure(a)))
            .then_with(|| a.tool.cmp(&b.tool))
    });
    let mut sign_ins: Vec<SignInReview> = plan
        .auth
        .iter()
        .enumerate()
        .map(|(entry, auth)| SignInReview {
            entry,
            surfaces: surface_names(&auth.surfaces),
            provider: auth.provider.clone(),
            mode: auth.mode,
            credentials: auth
                .credentials
                .iter()
                .map(|credential| CredentialShown {
                    role: credential.role.clone(),
                    reference: credential.reference.clone(),
                })
                .collect(),
            probe: match &auth.probe {
                ProbePlan::Operation { operation, .. } => ProbeReview::Declared {
                    name: plan
                        .operation(*operation)
                        .map(|found| found.name.to_string())
                        .unwrap_or_default(),
                },
                ProbePlan::Entry(reference) => ProbeReview::Declared {
                    name: reference.entry.clone(),
                },
                ProbePlan::Pending { name } => ProbeReview::Pending {
                    name: name.clone(),
                    says: PROBE_PENDING,
                },
            },
            sent_to: plan
                .environments
                .iter()
                .map(|environment| SentToReview {
                    environment: environment.name.clone(),
                    sent_to: SentTo::NotStated {
                        says: SENT_TO_NOT_STATED,
                    },
                })
                .collect(),
        })
        .collect();
    // A sign-in nobody can check yet first; the rest keep the file's order.
    sign_ins.sort_by_key(|sign_in| !matches!(sign_in.probe, ProbeReview::Pending { .. }));
    ReviewSummary { items, sign_ins }
}

/// What a tool exposes beyond its effect, for the order of the summary:
/// first where it sends (unknown, then off this machine), then what it
/// starts (unknown or a script, then a program).
fn exposure(item: &ReviewItem) -> (u8, u8) {
    let reach = match &item.reach {
        Reach::Partly { .. } => 2,
        Reach::Stated {
            leaves_this_machine: true,
            ..
        } => 1,
        Reach::Stated { .. } => 0,
    };
    let runs = item
        .bindings
        .iter()
        .map(|binding| match &binding.states {
            BindingStates::NotStated { .. } => 2,
            BindingStates::Stated { runs, .. } => runs
                .iter()
                .map(|run| match run.run {
                    ProgramRun::Script { .. } => 2,
                    ProgramRun::Program { .. } => 1,
                })
                .max()
                .unwrap_or(0),
        })
        .max()
        .unwrap_or(0);
    (reach, runs)
}
