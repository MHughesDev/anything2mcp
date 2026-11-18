//! The `page_http` mode: a read made as a request from inside the page,
//! with the page's own session (decision 0190).
//!
//! | Verb | Act | Confirm | Read back |
//! | --- | --- | --- | --- |
//! | `read` | `goto` the page that makes the request, `request` | | |
//! | the write verbs | Not served: the compiler refuses them, and an operation declared to write, by name | | |
//!
//! The `goto` sends an observing tab to the page the entry names and waits,
//! as every `goto` does, for its document to load: that is the named
//! condition the request waits on. The `request` step makes the read from
//! inside that page through the browser engine's read, which takes no
//! method and sends GET alone, checks the status, the media type, and the
//! answer's shape against the entry, and only then gives the values the
//! operation names. Checking and extracting are inside the step, so no
//! unchecked answer is held between two steps.
//!
//! Every run of this mode has an observing tab, whatever the run's
//! posture: the browser fails every request of the page that could change
//! the system, beneath the read.

use std::collections::BTreeMap;

use a2m_auth::SessionLease;
use a2m_core::{
    EffectClass, FailsAs, FailureClass, FileKindName, IdentityStrategy, Mode, PackPayload, PackRef,
    SectionIndex, Step, StepArg, StepKind, SurfaceKind, SurfaceName, Value, ValueType, Verb,
};
use a2m_files::{
    Diagnostics, JsonSchema, LowerContext, Lowered, Lowering, Place, Possible, ResolvedBinding,
    Stage, StepCommits,
};
use a2m_pack_web_browser::{Address, Browsers, Look, ObserveTab, PagePath, SafeRequests};
use a2m_provider::{
    BoxFuture, Cleanup, Driver, DriverHealth, Observed, PrepareContext, Prepared, StepFailure,
    StepInput, StepOutcome, SurfaceRef,
};
use a2m_telemetry::Detail;
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

use crate::codes::{self, shown};
use crate::driver::{
    TabSetup, acting, browser_failure, fail, refusal, segment, tab_setup, until, within_pattern,
};
use crate::lower::{GOTO, REACH_ARG};
use crate::model::{PAYLOAD_FORMAT, ReadItem};
use crate::requests::{Kind, PageRequest, REQUEST_KIND, RequestSection, pattern};
use crate::screens::{PACK, WEB_SURFACE, problem};

/// The name of the mode.
pub const PAGE_HTTP_MODE: &str = "page_http";

/// The step that makes the read from inside the page, checks its answer,
/// and gives its values.
pub const REQUEST: StepKind = StepKind::fixed("request");

/// What the name of a `request` step's argument starts with.
pub(crate) const PATH_ARG: &str = "path.";

/// The one verb this mode serves. A write verb is not one the manifest
/// declares, so the compiler refuses it as `kernel.unknown_verb`; a read
/// verb on an operation declared to write is refused here, as
/// `web.page_http_writes`.
const READ_VERBS: &[&str] = &["read"];

const DEFAULT_TIMEOUT_MS: u32 = 30_000;

/// The payload of a `request` step.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub(crate) struct RequestStep {
    /// The values given, each from the answer's top-level name of `from`.
    pub outputs: Vec<ReadItem>,
}

/// The lowering of the `page_http` mode.
pub(crate) struct PageHttpLowering {
    mode: Mode,
    schema: JsonSchema,
    verbs: Vec<Verb>,
    step_kinds: Vec<StepKind>,
}

impl PageHttpLowering {
    pub(crate) fn new(mode: Mode) -> Self {
        Self {
            mode,
            schema: JsonSchema::object().required("endpoint", JsonSchema::name()),
            verbs: READ_VERBS.iter().map(|verb| Verb::fixed(verb)).collect(),
            step_kinds: vec![GOTO, REQUEST],
        }
    }
}

fn step(
    cx: &LowerContext<'_>,
    kind: StepKind,
    section: SectionIndex,
    entry: &str,
    fails_as: FailsAs,
) -> Step {
    Step {
        timeout_ms: cx
            .target()
            .default_timeout_ms(kind.as_str())
            .unwrap_or(DEFAULT_TIMEOUT_MS),
        kind,
        refers_to: Some(PackRef {
            section,
            entry: entry.to_owned(),
        }),
        args: Vec::new(),
        fails_as,
        commits: false,
        lookup: false,
        idempotent: false,
        when: None,
        payload: PackPayload::none(),
    }
}

/// The arguments that fill the placeholders of a path, each the
/// operation's input of the same name, which must be one segment.
fn holes_of(
    cx: &LowerContext<'_>,
    path: &[crate::model::Segment],
    prefix: &str,
    at: &Place,
    problems: &mut Diagnostics,
) -> Vec<StepArg> {
    let mut args = Vec::new();
    for segment in path {
        let crate::model::Segment::Hole(hole) = segment else {
            continue;
        };
        if cx.operation().input(hole).is_none() {
            problems.push(problem(
                "placeholder_unbound",
                Stage::Resolve,
                at,
                format!(
                    "The placeholder `{hole}` of the page request matches no input of the operation."
                ),
                "Add an input of that name.",
            ));
            continue;
        }
        match cx.expr_at(&format!("input.{hole}"), at, None) {
            Ok(index) => {
                let fits = cx.expr_type(index).is_some_and(|ty| {
                    matches!(ty, ValueType::Text | ValueType::Int | ValueType::Choice)
                });
                if !fits {
                    problems.push(problem(
                        "position_type",
                        Stage::TypeCheck,
                        at,
                        format!("The input `{hole}` fills a segment of a path and is not text or a whole number."),
                        "Give the input a field type that is one segment of an address.",
                    ));
                }
                args.push(StepArg {
                    name: format!("{prefix}{hole}"),
                    value: index,
                });
            }
            Err(wrong) => problems.push(wrong),
        }
    }
    args
}

impl Lowering for PageHttpLowering {
    fn mode(&self) -> Mode {
        self.mode.clone()
    }

    fn verbs(&self) -> &[Verb] {
        &self.verbs
    }

    fn reads(&self) -> &[(FileKindName, SurfaceKind)] {
        &[]
    }

    fn step_kinds(&self) -> &[StepKind] {
        &self.step_kinds
    }

    /// Neither step saves: a `goto` reaches a page, and a `request` is a
    /// GET, which the browser engine's read cannot send any other way.
    fn commits(&self, _kind: &StepKind) -> StepCommits {
        StepCommits::Never
    }

    fn least_effect(&self, binding: &ResolvedBinding<'_>) -> EffectClass {
        if READ_VERBS.contains(&binding.verb.as_str()) {
            EffectClass::Read
        } else {
            EffectClass::Irreversible
        }
    }

    fn binding_schema(&self) -> &JsonSchema {
        &self.schema
    }

    fn identity(&self, binding: &ResolvedBinding<'_>, strategy: IdentityStrategy) -> Possible {
        match strategy {
            IdentityStrategy::None | IdentityStrategy::Natural | IdentityStrategy::Composite
                if READ_VERBS.contains(&binding.verb.as_str()) =>
            {
                Possible::Yes
            }
            _ => Possible::No(
                "a `page_http` binding only reads in this version of the web pack.".to_owned(),
            ),
        }
    }

    fn lower(
        &self,
        binding: &ResolvedBinding<'_>,
        cx: &LowerContext<'_>,
    ) -> Result<Lowered, Diagnostics> {
        let mut problems = Diagnostics::new();
        let at = &binding.doc.at;
        // The write half needs what this version does not have: the
        // anti-forgery token the page holds, sent without being read out of
        // the page; a commit point the request marks; and a read-back that
        // tells the new state from the old. Until it is designed, a write is
        // refused by name.
        let reads = READ_VERBS.contains(&binding.verb.as_str());
        if !reads || cx.operation().effect() != EffectClass::Read {
            return Err(Diagnostics::from(problem(
                "page_http_writes",
                Stage::Analyze,
                at,
                "A `page_http` binding only reads in this version: a write from inside the page needs the anti-forgery token the page holds, a commit point, and a read-back, which are not designed yet.",
                "Bind the write in the `ui` mode, or make this a read with `effect: read` and the verb `read`.",
            )));
        }
        let named = binding.doc.get("endpoint");
        let name = named.and_then(|named| named.as_text()).unwrap_or_default();
        let name_at = named.map_or(at, |named| &named.at);
        let found = cx
            .section(&REQUEST_KIND, &WEB_SURFACE)
            .and_then(|(index, section)| {
                let decoded = if section.format == PAYLOAD_FORMAT {
                    section.payload.decode::<RequestSection>().ok()
                } else {
                    None
                };
                decoded
                    .and_then(|section| section.get(name).cloned())
                    .map(|entry| (index, entry))
            });
        let Some((section, entry)) = found else {
            return Err(Diagnostics::from(problem(
                "unknown_endpoint",
                Stage::Resolve,
                name_at,
                format!(
                    "No file under `page_requests/` defines a page request named `{}` on a surface of the kind `web`.",
                    shown(name)
                ),
                "Name a page request that `page_requests/` defines.",
            )));
        };
        let surface = SurfaceName::new(&entry.surface).map_err(|_| {
            Diagnostics::from(problem(
                "unknown_endpoint",
                Stage::Resolve,
                name_at,
                "The page request's surface has no well-formed name.",
                "Name a surface the target declares.",
            ))
        })?;
        let payload = outputs(cx, &entry, name_at, &mut problems);
        let mut goto = step(
            cx,
            GOTO,
            section,
            name,
            FailsAs {
                class: FailureClass::UnknownVariant,
                code: codes::UNEXPECTED_PAGE,
            },
        );
        goto.args = holes_of(cx, &entry.page, REACH_ARG, name_at, &mut problems);
        let mut request = step(
            cx,
            REQUEST,
            section,
            name,
            FailsAs {
                class: FailureClass::Drift,
                code: codes::RESPONSE_SCHEMA_MISMATCH,
            },
        );
        request.args = holes_of(cx, &entry.path, PATH_ARG, name_at, &mut problems);
        match PackPayload::encode(&payload) {
            Ok(encoded) => request.payload = encoded,
            Err(failure) => problems.push(problem(
                "not_encodable",
                Stage::Lower,
                at,
                failure.message().to_owned(),
                "Report this to the web pack's author.",
            )),
        }
        if !problems.is_empty() {
            return Err(problems);
        }
        Ok(Lowered {
            surface,
            act: vec![goto, request],
            confirm: Vec::new(),
            read_back: Vec::new(),
            lookup: Vec::new(),
            settle_ms: 0,
            prefer_response: false,
        })
    }
}

/// Each output of the operation, from the answer's top-level value of its
/// name, which the entry must declare, require, and give a kind the output
/// can hold exactly.
fn outputs(
    cx: &LowerContext<'_>,
    entry: &PageRequest,
    at: &Place,
    problems: &mut Diagnostics,
) -> RequestStep {
    let mut payload = RequestStep::default();
    for output in cx.operation().outputs() {
        let Some(kind) = entry.properties.get(&output.name) else {
            problems.push(problem(
                "unknown_field",
                Stage::Resolve,
                at,
                format!(
                    "The output `{}` matches no value the page request's answer declares.",
                    output.name
                ),
                "Declare it under the answer's `properties`, or rename the output.",
            ));
            continue;
        };
        if !entry.required.contains(&output.name) {
            problems.push(problem(
                "output_not_required",
                Stage::Analyze,
                at,
                format!(
                    "The output `{}` is a value the answer may leave out.",
                    output.name
                ),
                "List it under the answer's `required`, if the answer always holds it.",
            ));
        }
        let fits = match kind {
            Kind::String => output.ty == ValueType::Text,
            Kind::Integer => output.ty == ValueType::Int,
            Kind::Boolean => output.ty == ValueType::Bool,
            Kind::Number | Kind::Null | Kind::Object | Kind::Array => false,
        };
        if !fits {
            problems.push(problem(
                "output_kind",
                Stage::TypeCheck,
                at,
                format!(
                    "The output `{}` cannot hold the answer's `{}` exactly: a string is read as text, an integer as a whole number, and a boolean as a truth value. A number with a fraction, and a nested value, are not read in this version.",
                    output.name,
                    kind.word()
                ),
                "Give the output the field type that matches, or leave the value out.",
            ));
        }
        payload.outputs.push(ReadItem {
            output: output.name.clone(),
            from: output.name.clone(),
        });
    }
    payload
}

// ---- the driver ----

/// The driver of the `page_http` mode.
pub(crate) struct PageHttpDriver {
    browsers: Browsers,
    installed: bool,
}

impl PageHttpDriver {
    pub(crate) fn new(browsers: Browsers) -> Self {
        Self {
            installed: browsers.availability() == a2m_core::Availability::Ready,
            browsers,
        }
    }
}

/// What one run holds: its observing tab and the page requests of its plan.
struct Session {
    tab: ObserveTab,
    origin: String,
    sections: BTreeMap<usize, RequestSection>,
}

impl Session {
    fn entry<'a>(&'a self, step: &Step) -> Result<(&'a str, &'a PageRequest), StepFailure> {
        let named = step
            .refers_to
            .as_ref()
            .ok_or_else(|| codes::not_usable("it names no page request"))?;
        self.sections
            .get(&named.section.get())
            .and_then(|section| section.get_key_value(&named.entry))
            .map(|(name, entry)| (name.as_str(), entry))
            .ok_or_else(|| codes::not_usable("the page request it names is not in the plan"))
    }

    async fn goto(&self, step: &Step, input: &StepInput<'_>) -> Result<StepOutcome, StepFailure> {
        let (name, entry) = self.entry(step)?;
        let mut path = String::new();
        for part in &entry.page {
            path.push('/');
            match part {
                crate::model::Segment::Literal(text) => path.push_str(text),
                crate::model::Segment::Hole(hole) => {
                    let value = input.args.get(&format!("{REACH_ARG}{hole}"));
                    path.push_str(&segment(hole, value)?);
                }
            }
        }
        if path.is_empty() {
            path.push('/');
        }
        let made = format!("{}{path}", self.origin);
        if !within_pattern(&path, &entry.page) || !made.starts_with(&format!("{}/", self.origin)) {
            return Err(StepFailure::not_sent(codes::surface_not_declared(
                "The address made for the page",
            )));
        }
        let address = Address::new(&made).map_err(StepFailure::not_sent)?;
        let what = format!("the page of the request `{name}`");
        let loaded = self
            .tab
            .goto(&address, until(step, input))
            .await
            .map_err(|error| browser_failure(&error, &what))?;
        let inside = loaded
            .address
            .strip_prefix(&self.origin)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with(['/', '?', '#']));
        if !inside {
            return Err(StepFailure::answered(codes::surface_not_declared(
                "The address the page moved to",
            )));
        }
        let status = loaded
            .status
            .map_or_else(|| "none".to_owned(), |status| status.to_string());
        Ok(StepOutcome::Done(Observed {
            captures: vec![acting(
                vec![
                    Detail::new("step", "goto"),
                    Detail::new("request", name),
                    Detail::new("page", pattern(&entry.page)),
                    Detail::new("status", status),
                ],
                Vec::new(),
            )],
            ..Observed::default()
        }))
    }

    async fn request(
        &self,
        step: &Step,
        input: &StepInput<'_>,
    ) -> Result<StepOutcome, StepFailure> {
        let (name, entry) = self.entry(step)?;
        let payload: RequestStep = step
            .payload
            .decode()
            .map_err(|_| codes::not_usable("its payload does not decode"))?;
        let mut values = BTreeMap::new();
        for part in &entry.path {
            if let crate::model::Segment::Hole(hole) = part {
                let value = input.args.get(&format!("{PATH_ARG}{hole}"));
                values.insert(hole.clone(), segment(hole, value)?);
            }
        }
        // The browser engine checks the values again as it makes the path.
        let path = PagePath::fill(&pattern(&entry.path), &values).map_err(StepFailure::not_sent)?;
        let what = format!("the request `{name}`");
        let answer = self
            .tab
            .read_from_page(&path, until(step, input))
            .await
            .map_err(|error| browser_failure(&error, &what))?;
        let mut means = vec![
            Detail::new("step", "request"),
            Detail::new("request", name),
            Detail::new("path", pattern(&entry.path)),
            Detail::new("status", answer.status.to_string()),
            Detail::new("media_type", shown(&answer.media_type)),
        ];
        if answer.status != 200 {
            return Err(StepFailure::answered(fail(
                FailureClass::UnknownVariant,
                codes::UNEXPECTED_STATUS,
                format!(
                    "The request `{name}` was answered with the status {}, which it does not declare.",
                    answer.status
                ),
                "Look at what the application answers; this version reads an answer of status 200 only.",
            )));
        }
        let json = answer.media_type == "application/json" || answer.media_type.ends_with("+json");
        let body: Option<Json> = if json {
            serde_json::from_str(&answer.body).ok()
        } else {
            None
        };
        let Some(Json::Object(fields)) = body else {
            return Err(mismatch(name, "the answer is not a JSON object".to_owned()));
        };
        for required in &entry.required {
            if !fields.contains_key(required) {
                return Err(mismatch(
                    name,
                    format!(
                        "the answer lacks `{}`, which it declares as required",
                        shown(required)
                    ),
                ));
            }
        }
        for (field, value) in &fields {
            match entry.properties.get(field) {
                Some(kind) if !kind.fits(value) => {
                    return Err(mismatch(
                        name,
                        format!("the answer's `{}` is not a {}", shown(field), kind.word()),
                    ));
                }
                None if entry.closed => {
                    return Err(mismatch(
                        name,
                        "the answer holds a name it does not declare, and it declares that it holds no other".to_owned(),
                    ));
                }
                _ => {}
            }
        }
        let mut observed = Observed::default();
        for item in &payload.outputs {
            let value = match fields.get(&item.from) {
                Some(Json::String(text)) => Value::Text(text.clone()),
                Some(Json::Bool(flag)) => Value::Bool(*flag),
                Some(Json::Number(number)) => match number.as_i64() {
                    Some(whole) => Value::Int(whole),
                    None => {
                        return Err(mismatch(
                            name,
                            format!("`{}` is not a whole number", shown(&item.from)),
                        ));
                    }
                },
                _ => {
                    return Err(mismatch(
                        name,
                        format!(
                            "the answer holds no plain value for `{}`",
                            shown(&item.from)
                        ),
                    ));
                }
            };
            observed.values.insert(item.output.clone(), value);
        }
        means.push(Detail::new("outputs", payload.outputs.len().to_string()));
        observed.captures.push(acting(means, Vec::new()));
        Ok(StepOutcome::Done(observed))
    }
}

/// The answer is not the shape the entry declares: `drift`, as the HTTP
/// pack and section 301 class a changed answer. The message names where it
/// differs and never quotes a value.
fn mismatch(name: &str, why: String) -> StepFailure {
    StepFailure::answered(fail(
        FailureClass::Drift,
        codes::RESPONSE_SCHEMA_MISMATCH,
        format!("The answer to the request `{name}` is not the shape its entry declares: {why}."),
        "Look at what the application answers now, and correct the entry under `page_requests/`.",
    ))
}

impl PageHttpDriver {
    async fn prepared(
        &self,
        cx: &PrepareContext<'_>,
        lease: Option<&SessionLease>,
    ) -> Result<Session, StepFailure> {
        let TabSetup {
            origin,
            profile,
            environment,
            origins,
        } = tab_setup(cx, lease)?;
        let surface = &cx.surface.surface;
        let mut sections = BTreeMap::new();
        for (position, section) in cx.plan.packs.iter().enumerate() {
            let own = section.pack == PACK
                && section.kind == REQUEST_KIND
                && section.surface_kind == surface.kind;
            if !own {
                continue;
            }
            if section.format != PAYLOAD_FORMAT {
                return Err(codes::not_usable(format!(
                    "its page requests are in format {}, and this pack reads format {PAYLOAD_FORMAT}",
                    section.format
                )));
            }
            sections.insert(
                position,
                section
                    .payload
                    .decode::<RequestSection>()
                    .map_err(|_| codes::not_usable("its page requests do not decode"))?,
            );
        }
        // Whatever the run's posture, the tab only observes: the browser
        // fails every request of the page that could change the system,
        // and none is marked safe.
        let tab = self
            .browsers
            .observer()
            .observing_tab(&profile, &environment, &SafeRequests::default(), &origins)
            .await
            .map_err(|error| browser_failure(&error, "the opening of a tab"))?;
        Ok(Session {
            tab,
            origin,
            sections,
        })
    }
}

impl Driver for PageHttpDriver {
    fn prepare<'a>(
        &'a self,
        cx: &'a PrepareContext<'a>,
        lease: Option<&'a SessionLease>,
    ) -> BoxFuture<'a, Result<Prepared, StepFailure>> {
        Box::pin(async move { self.prepared(cx, lease).await.map(Prepared::new) })
    }

    fn act<'a>(
        &'a self,
        prepared: &'a mut Prepared,
        step: &'a Step,
        input: &'a StepInput<'a>,
    ) -> BoxFuture<'a, Result<StepOutcome, StepFailure>> {
        Box::pin(async move {
            let session = prepared
                .state::<Session>()
                .ok_or_else(|| codes::not_usable("it was prepared by another driver"))?;
            let what = format!("the start of a `{}` step", step.kind);
            if let Some(failure) = refusal(&session.tab.blocked(), &what) {
                return Err(StepFailure::not_sent(failure));
            }
            let done = if step.kind == GOTO {
                session.goto(step, input).await
            } else if step.kind == REQUEST {
                session.request(step, input).await
            } else {
                return Err(codes::not_usable(format!(
                    "the `page_http` driver has no step of the kind `{}`",
                    step.kind
                )));
            };
            // What the tab refused during the step undoes what it read: the
            // page tried to leave, or to write.
            let what = format!("the end of a `{}` step", step.kind);
            match refusal(&session.tab.blocked(), &what) {
                Some(failure) => Err(StepFailure::not_sent(failure)),
                None => done,
            }
        })
    }

    fn health(&self, _surface: &SurfaceRef) -> DriverHealth {
        if self.installed {
            DriverHealth::Ready
        } else {
            DriverHealth::Unavailable(codes::BROWSER_UNAVAILABLE)
        }
    }

    fn release<'a>(&'a self, prepared: Prepared, _poisoned: bool) -> BoxFuture<'a, Cleanup> {
        Box::pin(async move {
            drop(prepared);
            Cleanup::complete()
        })
    }
}
