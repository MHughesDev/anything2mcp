//! A person's act that an agent asked for (decision 0163).
//!
//! Every caller of a served engine is an agent (decision 0158), so a
//! person's act, such as publishing a version, cannot be proved at the
//! caller's end. The dispatcher hands such an act to the person surface
//! instead: the act waits here, in memory, under a random identifier; the
//! surface shows it on a page; a press there raises the presence prompt;
//! and only a proof from that press, given for this act, carries it out,
//! once, through [`ApprovalDesk::carry_out`](crate::ApprovalDesk::carry_out),
//! as the person who proved presence. The agent is told `needs_person` with
//! the page's link; the same call again gives back the same page while the
//! act waits, and what was done once it was done.
//!
//! What waits is bounded: at most [`ACTS_WAITING`] acts, each for the
//! approval window, and what was done is kept for the same window, the
//! newest [`ACTS_KEPT`]. Nothing here is in the store: an engine that stops
//! forgets every act that waits, and nothing was done for it.

use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError, Weak};

use a2m_core::{
    DecisionId, Digest, EffectClass, Failure, FieldHasher, Hazard, PrincipalId, Timestamp,
};
use a2m_files::CompileMode;
use serde_json::{Map, Value as Json, json};

use crate::codes;
use crate::def::{ActionDef, ActionName, Handler};
use crate::dispatch::{Call, Caller, ExitClass, Next, Output, Response};
use crate::kernel::{Inner, Kernel};
use crate::person::{PresenceProof, SurfaceLink};

/// The most acts that wait for a person at once.
pub const ACTS_WAITING: usize = 16;
/// How many acts that were done, declined, or that ended unanswered are
/// kept, for the same call again.
pub const ACTS_KEPT: usize = 64;
/// The most characters of one input value a page is given.
pub const SHOWN_VALUE_CHARS: usize = 400;

/// The identifier of a person's act that waits on a page: sixteen random
/// bytes, shown as thirty-two hexadecimal characters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PersonActId([u8; 16]);

impl PersonActId {
    /// The identifier made of `bytes`.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    /// The identifier written as `text`, as [`std::fmt::Display`] writes it.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        if text.len() != 32 || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return None;
        }
        let mut bytes = [0_u8; 16];
        for (at, byte) in bytes.iter_mut().enumerate() {
            *byte = u8::from_str_radix(text.get(at * 2..at * 2 + 2)?, 16).ok()?;
        }
        Some(Self(bytes))
    }
}

impl std::fmt::Display for PersonActId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

/// One field of a person's act's input, as a page shows it: its name and
/// its value as text, redacted, and cut to [`SHOWN_VALUE_CHARS`]. A page
/// places both as text, never as markup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShownField {
    /// The field's name.
    pub name: String,
    /// Its value: a text as itself, anything else as compact JSON.
    pub value: String,
    /// Whether the value was cut.
    pub cut: bool,
}

/// One surface of a version a person is asked to publish, as the page shows
/// it: where it leads, and what its pack says a person must be told of it
/// (audit finding AC-1). Every text came from a target file and passes the
/// redactor; a page places each as text, never as markup.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ActSurface {
    /// The surface's name.
    pub name: String,
    /// Its kind, which a pack registered.
    pub kind: String,
    /// Where it leads in the target's default environment, in the form its
    /// kind's pack defines.
    pub address: String,
    /// Where it leads in each environment, in the order the plan declares
    /// them, each `(environment, address)`.
    pub addresses: Vec<(String, String)>,
    /// The notices its pack registers for what its own fields state
    /// (decision 0193).
    pub notices: Vec<crate::PreviewNotice>,
}

/// One warning the compiler gave for a version a person is asked to
/// publish (audit finding AC-1).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ActWarning {
    /// Its code, such as `kernel.unencrypted_credentials_accepted`.
    pub code: String,
    /// Its message, from the compiler, with the file it is about; it may
    /// quote a target file.
    pub message: String,
}

/// What a publish shows beside the tools, before it is redacted.
#[derive(Default)]
struct PublishShown {
    surfaces: Vec<ActSurface>,
    warnings: Vec<ActWarning>,
}

/// A person's act that an agent asked for, as the person surface is given
/// it and as [`ApprovalDesk::person_act`](crate::ApprovalDesk::person_act)
/// reads it back: exactly what will be done.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersonActAsk {
    /// The act.
    pub act: PersonActId,
    /// The person who must prove presence: the one the asking caller acts
    /// for. The act is carried out as them.
    pub person: PrincipalId,
    /// The action that will be carried out.
    pub action: ActionName,
    /// The action's summary.
    pub summary: String,
    /// The input it will be carried out with, every field, in order of
    /// name. What the engine pinned when it was asked is among them, such
    /// as the hash of the plan a publish will store.
    pub input: Vec<ShownField>,
    /// What else the person should read before they act, one line each,
    /// as text: for a publish, what each tool may do, the most dangerous
    /// first.
    pub details: Vec<String>,
    /// For a publish, every surface of the version, with where it leads
    /// and its notices; empty for every other act. They are not repeated
    /// among `details`: a page prints both (decision 0199).
    pub surfaces: Vec<ActSurface>,
    /// For a publish, every warning the compiler gave for the version;
    /// empty for every other act, and not repeated among `details`.
    pub warnings: Vec<ActWarning>,
    /// Through which door and client the act was asked for.
    pub asked_through: String,
    /// When the act stops waiting, by the kernel's clock. No answer by
    /// then is a decline.
    pub expires_at: Timestamp,
}

/// An act that waits.
struct Pending {
    ask: PersonActAsk,
    key: Digest,
    caller: Caller,
    input: Map<String, Json>,
    project: Option<PathBuf>,
    project_refused: Option<Failure>,
    link: Option<SurfaceLink>,
    /// A press is carrying it out.
    carrying: bool,
}

/// What became of an act: kept for the same call again.
struct Kept {
    key: Digest,
    until: Timestamp,
    outcome: Result<Output, Failure>,
}

#[derive(Default)]
struct State {
    pending: BTreeMap<PersonActId, Pending>,
    kept: VecDeque<Kept>,
}

/// The acts that wait for a person, and what became of the last ones.
pub(crate) struct Acts {
    kernel: OnceLock<Weak<Inner>>,
    state: Mutex<State>,
}

impl Acts {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            kernel: OnceLock::new(),
            state: Mutex::new(State::default()),
        })
    }

    /// Joins the acts to the kernel they are carried out by, once it is
    /// made. The desk holds the acts; the kernel holds the desk's surface;
    /// so the way back is weak.
    pub(crate) fn bind(&self, kernel: &Arc<Inner>) {
        let _ = self.kernel.set(Arc::downgrade(kernel));
    }

    pub(crate) fn kernel(&self) -> Result<Kernel, Failure> {
        self.kernel
            .get()
            .and_then(Weak::upgrade)
            .map(|inner| Kernel { inner })
            .ok_or_else(|| {
                codes::busy(
                    codes::ENGINE_DEGRADED,
                    "The engine is closing, so no person's act can be carried out.",
                    "Start the engine again and ask once more.",
                )
            })
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn not_waiting() -> Failure {
    codes::input(
        codes::NOT_FOUND,
        "No person's act waits under that identifier: it was carried out, declined, or its time ended.",
        "Ask for the act again, from the program that asked for it.",
    )
}

fn declined(by_person: bool) -> Failure {
    codes::policy(
        a2m_policy::APPROVAL_REFUSED,
        if by_person {
            "The person declined the act. Nothing was done."
        } else {
            "Nobody carried out the act on its page in its time, which is a decline. Nothing was done."
        },
        "Ask again if the act is still wanted; the person decides on a new page.",
    )
}

/// Ends what waits too long, and forgets what was kept too long. Called
/// with the state held.
fn sweep(state: &mut State, now: Timestamp) {
    let ended: Vec<PersonActId> = state
        .pending
        .iter()
        .filter(|(_, pending)| !pending.carrying && pending.ask.expires_at < now)
        .map(|(act, _)| *act)
        .collect();
    for act in ended {
        if let Some(pending) = state.pending.remove(&act) {
            keep(
                state,
                pending.key,
                pending.ask.expires_at,
                Err(declined(false)),
            );
        }
    }
    state.kept.retain(|kept| kept.until >= now);
}

fn keep(state: &mut State, key: Digest, until: Timestamp, outcome: Result<Output, Failure>) {
    state.kept.retain(|kept| kept.key != key);
    if state.kept.len() >= ACTS_KEPT {
        state.kept.pop_front();
    }
    state.kept.push_back(Kept {
        key,
        until,
        outcome,
    });
}

/// JSON text with every map's keys in order, so that one input always
/// gives one text, whatever order it was written in.
fn canonical(value: &Json, out: &mut String) {
    match value {
        Json::Object(map) => {
            let mut names: Vec<&String> = map.keys().collect();
            names.sort();
            out.push('{');
            for (at, name) in names.into_iter().enumerate() {
                if at > 0 {
                    out.push(',');
                }
                out.push_str(&Json::String(name.clone()).to_string());
                out.push(':');
                if let Some(inner) = map.get(name) {
                    canonical(inner, out);
                }
            }
            out.push('}');
        }
        Json::Array(items) => {
            out.push('[');
            for (at, item) in items.iter().enumerate() {
                if at > 0 {
                    out.push(',');
                }
                canonical(item, out);
            }
            out.push(']');
        }
        other => out.push_str(&other.to_string()),
    }
}

/// What makes two asks the same act: the person, the action, the input.
fn key_of(person: PrincipalId, action: &ActionName, input: &Map<String, Json>) -> Digest {
    let mut text = String::new();
    canonical(&Json::Object(input.clone()), &mut text);
    let mut hasher = FieldHasher::new();
    hasher.field("purpose", b"anything2mcp person act 1");
    hasher.field("person", person.to_string().as_bytes());
    hasher.field("action", action.as_str().as_bytes());
    hasher.field("input", text.as_bytes());
    hasher.finish()
}

/// The result of an act that waits for its person.
fn needs_person(ask: &PersonActAsk, link: &SurfaceLink, asked: &Map<String, Json>) -> Output {
    let mut result = Map::new();
    result.insert("state".to_owned(), json!("needs_person"));
    result.insert("act".to_owned(), json!(ask.act.to_string()));
    result.insert("action".to_owned(), json!(ask.action.as_str()));
    if let Some(url) = &link.url {
        result.insert("link".to_owned(), json!(url));
    }
    result.insert("person".to_owned(), json!(link.text));
    result.insert(
        "expires_at".to_owned(),
        json!(ask.expires_at.as_unix_millis()),
    );
    Output::ok(Json::Object(result))
        .exit(ExitClass::PersonNeeded)
        .then(Next::new(
            ask.action.as_str(),
            Json::Object(asked.clone()),
            "after the person has carried it out on its page; the same call then gives back what was done",
        ))
}

impl Kernel {
    /// The approval window, in milliseconds: how long a person's act waits.
    fn act_window_ms(&self) -> i64 {
        self.settings()
            .int("policy.approval_window_minutes")
            .unwrap_or(15)
            .clamp(1, 24 * 60)
            .saturating_mul(60_000)
    }

    /// Hands a person's act an agent asked for to the person surface, and
    /// gives the caller `needs_person` with the page's link. The same ask
    /// again, while the act waits, gives the same page; once it was done,
    /// declined, or ended, it gives back what became of it, for the
    /// approval window. The act's own decision is made again, for the
    /// person, when it is carried out.
    ///
    /// # Errors
    ///
    /// `policy`/`person_required` when the caller acts for no person, or
    /// the surface has no page for it; `busy`/`queue_full` when too many
    /// acts wait; what pinning the act's input refused, such as a lint
    /// failure of the drafts a publish would store.
    pub(crate) async fn ask_person(
        &self,
        def: &ActionDef,
        caller: &Caller,
        input: Map<String, Json>,
        project: (Option<PathBuf>, Option<Failure>),
    ) -> Result<Output, Failure> {
        if !self.person().shows_person_acts() {
            return Err(codes::policy(
                codes::PERSON_REQUIRED,
                format!(
                    "`{}` is a person's act, and this engine's person surface has no page on which a person can carry it out.",
                    def.name
                ),
                "Ask the person to run it themselves, at a terminal, with no engine running.",
            ));
        }
        if !matches!(def.handler, Handler::Kernel(_) | Handler::Provider(_)) {
            return Err(codes::input(
                codes::ACTION_NOT_BUILT,
                format!("`{}` is a person's act that is not built yet.", def.name),
                "Call `action.search` to see which actions are built.",
            ));
        }
        let Some(person) = caller.principal.person() else {
            return Err(codes::policy(
                codes::PERSON_REQUIRED,
                format!(
                    "`{}` is a person's act, and this caller acts for no person who could carry it out.",
                    def.name
                ),
                "Ask the person to run it themselves.",
            ));
        };
        let key = key_of(person, &def.name, &input);
        let now = self.inner.clock.now();
        {
            let mut state = self.inner.acts.state();
            sweep(&mut state, now);
            if let Some(kept) = state.kept.iter().rev().find(|kept| kept.key == key) {
                return match &kept.outcome {
                    Ok(output) => Ok(output.clone().warn(
                        "This act was answered by its person on its page; this call did nothing more. Ask again after the approval window for a new act.",
                    )),
                    Err(failure) => Err(failure.clone()),
                };
            }
            if let Some(pending) = state.pending.values().find(|pending| pending.key == key)
                && let Some(link) = &pending.link
            {
                return Ok(needs_person(&pending.ask, link, &input));
            }
            if state.pending.len() >= ACTS_WAITING {
                return Err(codes::busy(
                    a2m_sched::codes::QUEUE_FULL,
                    format!("{ACTS_WAITING} acts already wait for a person on this engine."),
                    "Let the person answer or close the pages that wait, then ask again.",
                ));
            }
        }

        // What the act rests on is pinned now, so that the person carries
        // out what they read and nothing changed since.
        let (pinned, details, publish) = self.pinned(def, caller, input.clone(), &project).await?;

        let mut bytes = [0_u8; 16];
        self.inner.random.fill(&mut bytes)?;
        let act = PersonActId::from_bytes(bytes);
        let redact = self.redact();
        let shown = |value: &Json| {
            let text = match value {
                Json::String(text) => text.clone(),
                other => other.to_string(),
            };
            let text = redact.redact(&text);
            let cut = text.chars().count() > SHOWN_VALUE_CHARS;
            let text = if cut {
                text.chars().take(SHOWN_VALUE_CHARS).collect()
            } else {
                text
            };
            (text, cut)
        };
        let mut names: Vec<&String> = pinned.keys().collect();
        names.sort();
        let fields = names
            .into_iter()
            .filter_map(|name| {
                let (value, cut) = shown(pinned.get(name)?);
                Some(ShownField {
                    name: redact.redact(name),
                    value,
                    cut,
                })
            })
            .collect();
        let ask = PersonActAsk {
            act,
            person,
            action: def.name.clone(),
            summary: def.summary.clone(),
            input: fields,
            details: details.iter().map(|line| redact.redact(line)).collect(),
            surfaces: publish
                .surfaces
                .into_iter()
                .map(|surface| ActSurface {
                    name: redact.redact(&surface.name),
                    kind: redact.redact(&surface.kind),
                    address: redact.redact(&surface.address),
                    addresses: surface
                        .addresses
                        .into_iter()
                        .map(|(environment, address)| {
                            (redact.redact(&environment), redact.redact(&address))
                        })
                        .collect(),
                    notices: surface.notices,
                })
                .collect(),
            warnings: publish
                .warnings
                .into_iter()
                .map(|warning| ActWarning {
                    code: redact.redact(&warning.code),
                    message: redact.redact(&warning.message),
                })
                .collect(),
            asked_through: redact.redact(&format!(
                "{} ({})",
                caller.door.as_str(),
                if caller.client.is_empty() {
                    "no client label"
                } else {
                    caller.client.as_str()
                }
            )),
            expires_at: Timestamp::from_unix_millis(
                now.as_unix_millis().saturating_add(self.act_window_ms()),
            ),
        };
        self.inner.acts.state().pending.insert(
            act,
            Pending {
                ask: ask.clone(),
                key,
                caller: caller.clone(),
                input: pinned,
                project: project.0,
                project_refused: project.1,
                link: None,
                carrying: false,
            },
        );
        match self.person().ask_person_act(ask.clone()).await {
            Ok(link) => {
                if let Some(pending) = self.inner.acts.state().pending.get_mut(&act) {
                    pending.link = Some(link.clone());
                }
                Ok(needs_person(&ask, &link, &input))
            }
            Err(failure) => {
                self.inner.acts.state().pending.remove(&act);
                Err(failure)
            }
        }
    }

    /// The input of an act with what it rests on pinned, and the lines a
    /// person reads beside it. A publish compiles the drafts now, at the
    /// version it will be published as, and pins that version and the
    /// plan's hash: the person publishes the plan they were shown, and
    /// drafts changed since are refused when they press.
    async fn pinned(
        &self,
        def: &ActionDef,
        caller: &Caller,
        mut input: Map<String, Json>,
        project: &(Option<PathBuf>, Option<Failure>),
    ) -> Result<(Map<String, Json>, Vec<String>, PublishShown), Failure> {
        if def.name.as_str() != "version.publish" {
            return Ok((input, Vec::new(), PublishShown::default()));
        }
        let folder = match project {
            (Some(folder), _) => folder.clone(),
            (None, Some(refused)) => return Err(refused.clone()),
            (None, None) => {
                return Err(codes::input(
                    codes::PATH_NOT_ALLOWED,
                    "A publish works on a project folder, and the call names none.",
                    "Run it inside a project folder, or pass `--project` with the folder's full path.",
                ));
            }
        };
        let target =
            a2m_core::TargetName::new(input.get("target").and_then(Json::as_str).unwrap_or(""))?;
        let version = crate::handlers::build::chosen_version(
            self,
            caller.principal.workspace,
            &target,
            input.get("version").and_then(Json::as_str),
        )
        .await?;
        let compiled = crate::handlers::build::compile_in(
            self,
            folder.clone(),
            &target,
            CompileMode::Publish,
            version,
        )
        .await?
        .map_err(|problems| crate::offline::publish_refusal(&target, problems))?;
        // The publish gates run now, as they will when the person presses:
        // a publish they would refuse is not put to a person, and the page
        // says which gates ran and which are not built (decision 0180).
        let gates = crate::offline::run_gates(
            self,
            folder,
            &target,
            &compiled,
            crate::offline::OfflineWorld {
                workspace: caller.principal.workspace,
                principal: caller.principal.principal.id,
                vault: self.inner.vault.clone(),
            },
            None,
        )
        .await?;
        if !gates.passed {
            return Err(gates.refusal(&target));
        }
        input.insert("version".to_owned(), json!(version.to_string()));
        input.insert(
            "plan_hash".to_owned(),
            json!(compiled.encoded.hash.to_string()),
        );
        // The bindings that may act for the first time in this version, by
        // operation and mode: a first binding promoted from `draft` acts
        // first here, where a person decides (decision 0189).
        let first_to_act = self.first_to_act(&target, &compiled.plan).await?;
        let details = compiled
            .review
            .items
            .iter()
            .map(|item| {
                let surfaces: Vec<String> = item.surfaces.iter().map(ToString::to_string).collect();
                let mut line = format!(
                    "{}: {}, hazard {}, {} commit points, surfaces {}",
                    item.tool,
                    item.effect,
                    item.hazard,
                    item.commit_points,
                    if surfaces.is_empty() {
                        "none".to_owned()
                    } else {
                        surfaces.join(", ")
                    },
                );
                if !item.destinations.is_empty() {
                    line.push_str(&format!("; also reaches {}", item.destinations.join(", ")));
                }
                if !item.uploads.is_empty() {
                    line.push_str(&format!("; sends files {}", item.uploads.join(", ")));
                }
                if !item.downloads.is_empty() {
                    line.push_str(&format!("; receives files {}", item.downloads.join(", ")));
                }
                line
            })
            .chain(first_to_act)
            .collect::<Vec<String>>();
        // Where each surface leads, its notices, and the warnings are
        // handed apart, in `PersonActAsk::surfaces` and `warnings`, and not
        // also as lines: a page prints them once (decision 0199).
        let shown = publish_shown(self, &compiled);
        let mut details = details;
        details.extend(gates.lines());
        Ok((input, details, shown))
    }

    /// One line for each verified binding of `plan` that the target's
    /// current version does not hold verified for the same operation and
    /// mode: what may act for the first time once `plan` is published.
    pub(crate) async fn first_to_act(
        &self,
        target: &a2m_core::TargetName,
        plan: &a2m_core::Plan,
    ) -> Result<Vec<String>, Failure> {
        let (plans, _) = self.current_plans().await?;
        let current = plans
            .into_iter()
            .find(|(name, _, _)| name == target)
            .map(|(_, _, plan)| plan);
        let acted_before = |operation: &a2m_core::OperationName, mode: &a2m_core::Mode| {
            current.as_ref().is_some_and(|current| {
                current.operations.iter().any(|earlier| {
                    earlier.name == *operation
                        && earlier.bindings.iter().any(|binding| {
                            binding.mode == *mode
                                && binding.status == a2m_core::BindingStatus::Verified
                        })
                })
            })
        };
        let mut lines = Vec::new();
        for operation in &plan.operations {
            for binding in &operation.bindings {
                if binding.status == a2m_core::BindingStatus::Verified
                    && !acted_before(&operation.name, &binding.mode)
                {
                    lines.push(format!(
                        "First to act in this version: {} ({}), through the mode {}.",
                        operation.name, operation.effect, binding.mode
                    ));
                }
            }
        }
        Ok(lines)
    }

    /// The act that waits under `act`, as its page shows it.
    pub(crate) fn waiting_act(&self, act: PersonActId) -> Result<PersonActAsk, Failure> {
        let mut state = self.inner.acts.state();
        sweep(&mut state, self.inner.clock.now());
        match state.pending.get(&act) {
            Some(pending) if !pending.carrying => Ok(pending.ask.clone()),
            _ => Err(not_waiting()),
        }
    }

    /// Records that the person declined the act, or that its page closed
    /// unanswered (`by_person` false). An act no longer waiting is left as
    /// it is.
    pub(crate) fn decline_act(&self, act: PersonActId, by_person: bool) {
        let mut state = self.inner.acts.state();
        let waiting = state
            .pending
            .get(&act)
            .is_some_and(|pending| !pending.carrying);
        if waiting && let Some(pending) = state.pending.remove(&act) {
            let until = Timestamp::from_unix_millis(
                self.inner
                    .clock
                    .now()
                    .as_unix_millis()
                    .saturating_add(self.act_window_ms()),
            );
            keep(&mut state, pending.key, until, Err(declined(by_person)));
        }
    }

    /// Carries out the act that waits under `act`, once, with a proof of
    /// presence given for it.
    ///
    /// # Errors
    ///
    /// `input`/`not_found` when nothing waits under `act`, a press already
    /// carries it out, or its time ended; `policy`/`presence_required`
    /// when the proof was given for another act, is another person's, or
    /// is not of now. In each case nothing is done, and an act that still
    /// waits goes on waiting.
    pub(crate) async fn carry_out_act(
        &self,
        act: PersonActId,
        proof: PresenceProof,
    ) -> Result<Response, Failure> {
        let now = self.inner.clock.now();
        let refuse = |why: &str| {
            codes::policy(
                a2m_policy::PRESENCE_REQUIRED,
                format!("The act was not carried out: the proof of presence {why}."),
                "Open the act's page again and press there yourself.",
            )
        };
        let (def_name, caller, input, project, project_refused) = {
            let mut state = self.inner.acts.state();
            sweep(&mut state, now);
            let Some(pending) = state.pending.get_mut(&act) else {
                return Err(not_waiting());
            };
            if pending.carrying {
                return Err(not_waiting());
            }
            if proof.act() != Some(act) {
                return Err(refuse("was given for another ask"));
            }
            proof
                .checked(pending.ask.person, now)
                .map_err(|why| refuse(&format!("was not taken: {why}")))?;
            pending.carrying = true;
            (
                pending.ask.action.clone(),
                pending.caller.clone(),
                pending.input.clone(),
                pending.project.clone(),
                pending.project_refused.clone(),
            )
        };
        let outcome = self
            .perform_act(&def_name, caller, input, project, project_refused, proof)
            .await;
        let kept = match &outcome {
            Ok((output, _)) => Ok(output.clone()),
            Err(performed) => Err(performed.0.clone()),
        };
        {
            let mut state = self.inner.acts.state();
            if let Some(pending) = state.pending.remove(&act) {
                let until = Timestamp::from_unix_millis(
                    self.inner
                        .clock
                        .now()
                        .as_unix_millis()
                        .saturating_add(self.act_window_ms()),
                );
                keep(&mut state, pending.key, until, kept);
            }
        }
        let asked = self.redact().redact(def_name.as_str());
        Ok(self.shaped(
            asked,
            outcome.map_err(|performed| {
                let Performed(failure, effect, decision) = *performed;
                crate::dispatch::Refused::after(failure, effect, decision)
            }),
        ))
    }

    /// The act itself, as the person: health, a decision of its own for
    /// the person, the project folder looked at again, the handler with
    /// the proof, and the cap.
    async fn perform_act(
        &self,
        name: &ActionName,
        asker: Caller,
        input: Map<String, Json>,
        project: Option<PathBuf>,
        project_refused: Option<Failure>,
        proof: PresenceProof,
    ) -> Result<(Output, DecisionId), Box<Performed>> {
        let none = DecisionId::from_bytes([0; 16]);
        let Some(def) = self.actions().get(name).cloned() else {
            return Err(Box::new(Performed(not_waiting(), EffectClass::Read, none)));
        };
        let effect = def.fixed_effect().unwrap_or(EffectClass::Write);
        let health = self.health();
        if !health.allows(def.needs, effect) {
            return Err(Box::new(Performed(
                codes::busy(
                    codes::ENGINE_DEGRADED,
                    format!(
                        "The engine is {}: {}.",
                        health.state.as_str(),
                        health.cause()
                    ),
                    "Call `doctor.run` for the fix, then ask again.",
                ),
                effect,
                none,
            )));
        }
        // The person carries it out: a caller of the same principal that
        // is no agent, through the same door, for the record.
        let mut person = asker;
        person.is_agent = false;
        person.client = "person page".to_owned();
        person.toolset = None;
        let decided = self
            .decide_action(&def, &person, effect, Hazard::None)
            .await;
        if let Some(refusal) = decided.refusal {
            return Err(Box::new(Performed(refusal, effect, decided.id)));
        }
        if !decided.allowed {
            return Err(Box::new(Performed(
                codes::policy(
                    a2m_policy::NOT_GRANTED,
                    "Policy did not allow this act for the person.",
                    "Ask an administrator for the role the action needs.",
                ),
                effect,
                decided.id,
            )));
        }
        // The folder is looked at again: what was checked when the act was
        // asked for may have changed while it waited.
        let (project, project_refused) = match project {
            Some(folder) => match self.checked_project(&folder).await {
                Ok(folder) => (Some(folder), None),
                Err(refused) => (None, Some(refused)),
            },
            None => (None, project_refused),
        };
        let call = Call {
            input,
            caller: person,
            decision: decided.id,
            idempotency_key: None,
            project,
            project_refused,
            presence: Some(proof),
            decided: Some((effect, Hazard::None)),
        };
        match self.run_handler(&def, call).await {
            Ok(output) => Ok((self.capped(&def, output), decided.id)),
            Err(failure) => Err(Box::new(Performed(failure, effect, decided.id))),
        }
    }
}

/// An act that was refused or failed: the failure, the effect it was
/// decided for, and the decision.
pub(crate) struct Performed(
    pub(crate) Failure,
    pub(crate) EffectClass,
    pub(crate) DecisionId,
);

/// Each surface of a compiled version with where it leads in each
/// environment and the notices its pack registers, and every warning of the
/// compile: what a person who publishes is told (audit finding AC-1).
fn publish_shown(kernel: &Kernel, compiled: &a2m_files::Compiled) -> PublishShown {
    let plan = &compiled.plan;
    let set = kernel.providers().compiler_set();
    let default = plan
        .default_environment()
        .map(|environment| environment.name.clone());
    let surfaces = plan
        .surfaces
        .iter()
        .enumerate()
        .map(|(index, surface)| {
            let at = |environment: &a2m_core::EnvironmentPlan| {
                environment
                    .addresses
                    .iter()
                    .find(|given| given.surface.get() == index)
                    .map_or(surface.address.clone(), |given| given.address.clone())
            };
            let addresses: Vec<(String, String)> = plan
                .environments
                .iter()
                .map(|environment| (environment.name.to_string(), at(environment)))
                .collect();
            let address = plan
                .environments
                .iter()
                .find(|environment| Some(&environment.name) == default.as_ref())
                .map_or(surface.address.clone(), at);
            ActSurface {
                name: surface.name.to_string(),
                kind: surface.kind.to_string(),
                address,
                addresses,
                notices: crate::person::notices_of(set, surface),
            }
        })
        .collect();
    let warnings = compiled
        .warnings
        .iter()
        .map(|warning| ActWarning {
            code: warning.code.to_string(),
            message: format!("{}: {}", warning.at.file.as_str(), warning.message),
        })
        .collect();
    PublishShown { surfaces, warnings }
}
