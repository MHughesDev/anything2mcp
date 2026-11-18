//! The `page_http` mode: a request made from inside the page, with the
//! page's own session (decisions 0190 and 0206).
//!
//! | Verb | Act | Confirm | Read back |
//! | --- | --- | --- | --- |
//! | `read` | `goto` the page that makes the request, in an observing tab; `request` | | |
//! | `update` | `goto` the page, in an acting tab; `request`, a PUT with the page's own anti-forgery token, marked as the commit point | | `goto` and `request` of the read entry `read_back` names, in an observing tab |
//! | the other write verbs | Not served: the compiler refuses them as verbs the mode does not declare | | |
//!
//! The `goto` sends a tab to the page the entry names and waits, as every
//! `goto` does, for its document to load: that is the named condition the
//! request waits on. A page that answers from another path, as a sign-in
//! page does once the session has ended, stops the step before anything
//! is sent. The `request` step makes the request from inside that page
//! through the browser engine, checks the status and the answer's shape
//! against the entry, and only then gives the values the operation names.
//! Checking and extracting are inside the step, so no unchecked answer is
//! held between two steps.
//!
//! A read is made with the engine's read, which takes no method and sends
//! GET alone, in the observing tab every session opens at prepare, in every
//! posture: the browser fails every request of the page that could change
//! the system, beneath the read. A write is made with the engine's write,
//! which only an acting tab has: the tab is opened in the same profile at
//! the write's `goto`, after the engine stored the record's intent, and
//! only in a posture that may act. The page's anti-forgery token is read by
//! the browser engine from the page, where the entry says, in the same call
//! that sends the write, and never leaves the page.
//!
//! A page that writes by itself as it loads in the acting tab does so as
//! part of the write's own steps, after the intent; only the read-back
//! verifies the record. That is the limit decision 0195 states for the
//! `ui` mode, and it holds here the same.

use std::collections::BTreeMap;

use a2m_auth::SessionLease;
use a2m_core::{
    EffectClass, FailsAs, FailureClass, FileKindName, IdentityStrategy, Mode, PackPayload, PackRef,
    SectionIndex, Step, StepArg, StepKind, SurfaceKind, SurfaceName, Value, ValueType, Verb,
};
use a2m_files::{
    Diagnostics, Doc, JsonSchema, Located, LowerContext, Lowered, Lowering, Place, Possible,
    ResolvedBinding, Stage, StepCommits,
};
use a2m_pack_web_browser::{
    ActTab, Address, AntiForgery, BodyEncoding, Browsers, Dispatch, FieldValue, Look, ObserveTab,
    Origins, PageAnswer, PageEnvironment, PagePath, PageWrite, ProfileKey, SafeRequests, TokenFrom,
    TokenTo, Visibility, WriteMethod,
};
use a2m_provider::{
    BoxFuture, Cleanup, Driver, DriverHealth, Observed, Posture, PrepareContext, Prepared,
    StepFailure, StepInput, StepOutcome, SurfaceRef,
};
use a2m_telemetry::Detail;
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

use crate::codes::{self, shown};
use crate::driver::{
    TabSetup, acting, browser_failure, fail, refusal, segment, tab_setup, until, within_pattern,
};
use crate::lower::{GOTO, REACH_ARG};
use crate::model::{GotoStep, PAYLOAD_FORMAT, ReadItem, TabKind};
use crate::requests::{Kind, PageRequest, REQUEST_KIND, RequestSection, WriteForm, pattern};
use crate::screens::{PACK, WEB_SURFACE, problem};

/// The name of the mode.
pub const PAGE_HTTP_MODE: &str = "page_http";

/// The step that makes the request from inside the page, checks its
/// answer, and gives its values.
pub const REQUEST: StepKind = StepKind::fixed("request");

/// What the name of a `request` step's argument for a path starts with.
pub(crate) const PATH_ARG: &str = "path.";
/// What the name of a `request` step's argument for a body field starts
/// with.
pub(crate) const BODY_ARG: &str = "body.";

/// The verbs that read. A read verb on an operation declared to write is
/// refused here, as `web.page_http_writes`.
const READ_VERBS: &[&str] = &["read"];
/// The verbs that write: `update`, by PUT (decision 0206). Every other
/// write verb is not one the manifest declares, so the compiler refuses it
/// as `kernel.unknown_verb`.
const WRITE_VERBS: &[&str] = &["update"];

const DEFAULT_TIMEOUT_MS: u32 = 30_000;

/// The payload of a `request` step.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub(crate) struct RequestStep {
    /// The values given, each from the answer's top-level name of `from`.
    pub outputs: Vec<ReadItem>,
    /// For a write: the body's fields the step's arguments fill, each under
    /// [`BODY_ARG`] and its name.
    #[serde(default)]
    pub body: Vec<String>,
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
            schema: JsonSchema::object()
                .required("endpoint", JsonSchema::name())
                .optional("body", JsonSchema::map_of(JsonSchema::expression()))
                .optional(
                    "read_back",
                    JsonSchema::object().required("endpoint", JsonSchema::name()),
                ),
            verbs: READ_VERBS
                .iter()
                .chain(WRITE_VERBS)
                .map(|verb| Verb::fixed(verb))
                .collect(),
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

/// One entry as a lowering uses it.
struct Found {
    name: String,
    section: SectionIndex,
    entry: PageRequest,
}

/// The entry a binding, or its `read_back`, names.
fn found(cx: &LowerContext<'_>, part: &Located<Doc>) -> Result<Found, Diagnostics> {
    let named = part.get("endpoint");
    let name = named.and_then(|named| named.as_text()).unwrap_or_default();
    let name_at = named.map_or(&part.at, |named| &named.at);
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
    Ok(Found {
        name: name.to_owned(),
        section,
        entry,
    })
}

fn encoded<T: Serialize>(value: &T, at: &Place, problems: &mut Diagnostics) -> PackPayload {
    match PackPayload::encode(value) {
        Ok(encoded) => encoded,
        Err(failure) => {
            problems.push(problem(
                "not_encodable",
                Stage::Lower,
                at,
                failure.message().to_owned(),
                "Report this to the web pack's author.",
            ));
            PackPayload::none()
        }
    }
}

/// The `goto` and `request` of a read of an entry: the steps of a `read`,
/// and of a write's read-back.
fn read_steps(
    cx: &LowerContext<'_>,
    found: &Found,
    at: &Place,
    problems: &mut Diagnostics,
) -> Vec<Step> {
    let payload = outputs(cx, &found.entry, at, problems);
    let mut goto = step(
        cx,
        GOTO,
        found.section,
        &found.name,
        FailsAs {
            class: FailureClass::UnknownVariant,
            code: codes::UNEXPECTED_PAGE,
        },
    );
    goto.args = holes_of(cx, &found.entry.page, REACH_ARG, at, problems);
    goto.payload = encoded(
        &GotoStep {
            tab: TabKind::Observe,
        },
        at,
        problems,
    );
    let mut request = step(
        cx,
        REQUEST,
        found.section,
        &found.name,
        FailsAs {
            class: FailureClass::Drift,
            code: codes::RESPONSE_SCHEMA_MISMATCH,
        },
    );
    request.args = holes_of(cx, &found.entry.path, PATH_ARG, at, problems);
    request.payload = encoded(&payload, at, problems);
    vec![goto, request]
}

impl PageHttpLowering {
    fn lower_read(
        binding: &ResolvedBinding<'_>,
        cx: &LowerContext<'_>,
        found: &Found,
    ) -> Result<Lowered, Diagnostics> {
        let mut problems = Diagnostics::new();
        let at = &binding.doc.at;
        if found.entry.write.is_some() {
            problems.push(problem(
                "effect_mismatch",
                Stage::Analyze,
                at,
                format!(
                    "The verb `{}` reads, and the page request `{}` writes.",
                    binding.verb,
                    shown(&found.name)
                ),
                "Bind a read to a page request with `method: GET` and `effect: read`.",
            ));
        }
        for key in ["body", "read_back"] {
            if let Some(stray) = binding.field(key) {
                problems.push(problem(
                    "read_acts",
                    Stage::Analyze,
                    &stray.at,
                    format!(
                        "The verb `{}` reads, and the binding states `{key}`, which belongs to a write.",
                        binding.verb
                    ),
                    "Remove it, or bind a verb that writes.",
                ));
            }
        }
        let mut act = read_steps(cx, found, at, &mut problems);
        // A read of one record may find none, when the entry declares how
        // the application says so.
        if !found.entry.absent.is_empty() {
            for step in &mut act {
                step.lookup = true;
            }
        }
        if !problems.is_empty() {
            return Err(problems);
        }
        Ok(Lowered {
            surface: surface_of(&found.entry, at)?,
            act,
            confirm: Vec::new(),
            read_back: Vec::new(),
            lookup: Vec::new(),
            settle_ms: 0,
            prefer_response: false,
        })
    }

    fn lower_write(
        binding: &ResolvedBinding<'_>,
        cx: &LowerContext<'_>,
        found: &Found,
    ) -> Result<Lowered, Diagnostics> {
        let mut problems = Diagnostics::new();
        let at = &binding.doc.at;
        let Some(write) = &found.entry.write else {
            return Err(Diagnostics::from(problem(
                "page_http_writes",
                Stage::Analyze,
                at,
                format!(
                    "The verb `{}` writes, and the page request `{}` is a read: a write from inside the page is made only through an entry that declares it, written by a person.",
                    binding.verb,
                    shown(&found.name)
                ),
                "Write the entry with `method: PUT`, `effect: write`, its body, its answers, and its `anti_forgery`, or bind the write in the `ui` mode.",
            )));
        };
        // The record is read back through an entry that reads, on the same
        // surface, and says how the application answers for a record that
        // does not exist.
        let Some(read_back) = binding.field("read_back") else {
            return Err(Diagnostics::from(problem(
                "read_back_missing",
                Stage::Analyze,
                at,
                "A write from inside the page names the read entry its record is read back through, under `read_back`.",
                "Add `read_back: {endpoint: ...}` with a page request that reads the record by its key.",
            )));
        };
        let reader = found_part(cx, read_back)?;
        if reader.entry.write.is_some() {
            problems.push(problem(
                "effect_mismatch",
                Stage::Analyze,
                &read_back.at,
                format!(
                    "The record is read back through `{}`, which writes.",
                    shown(&reader.name)
                ),
                "Name a page request that reads.",
            ));
        }
        if reader.entry.absent.is_empty() {
            problems.push(problem(
                "read_back_never_absent",
                Stage::Analyze,
                &read_back.at,
                format!(
                    "The record is read back through `{}`, which declares no status for a record that does not exist.",
                    shown(&reader.name)
                ),
                "Add `responses: {absent: [404]}` to the entry, with the status the application answers.",
            ));
        }
        if reader.entry.surface != found.entry.surface {
            problems.push(problem(
                "read_back_elsewhere",
                Stage::Analyze,
                &read_back.at,
                "The record is read back on another surface than the one it is written on.",
                "Name a page request of the same surface.",
            ));
        }
        let body_args = body_of(cx, binding, &found.name, write, &mut problems);
        let mut goto = step(
            cx,
            GOTO,
            found.section,
            &found.name,
            FailsAs {
                class: FailureClass::UnknownVariant,
                code: codes::UNEXPECTED_PAGE,
            },
        );
        goto.args = holes_of(cx, &found.entry.page, REACH_ARG, at, &mut problems);
        goto.payload = encoded(&GotoStep { tab: TabKind::Act }, at, &mut problems);
        let mut request = step(
            cx,
            REQUEST,
            found.section,
            &found.name,
            FailsAs {
                class: FailureClass::UnknownVariant,
                code: codes::UNEXPECTED_STATUS,
            },
        );
        request.args = holes_of(cx, &found.entry.path, PATH_ARG, at, &mut problems);
        request.payload = encoded(
            &RequestStep {
                outputs: Vec::new(),
                body: body_args.iter().map(|(field, _)| field.clone()).collect(),
            },
            at,
            &mut problems,
        );
        request
            .args
            .extend(body_args.into_iter().map(|(_, arg)| arg));
        // The one step that saves: the PUT, once the intent is stored.
        request.commits = true;
        let mut back = read_steps(cx, &reader, &read_back.at, &mut problems);
        for step in &mut back {
            step.lookup = true;
        }
        if !problems.is_empty() {
            return Err(problems);
        }
        Ok(Lowered {
            surface: surface_of(&found.entry, at)?,
            act: vec![goto, request],
            confirm: Vec::new(),
            read_back: back,
            lookup: Vec::new(),
            settle_ms: 0,
            prefer_response: false,
        })
    }
}

fn found_part(cx: &LowerContext<'_>, part: &Located<Doc>) -> Result<Found, Diagnostics> {
    found(cx, part)
}

fn surface_of(entry: &PageRequest, at: &Place) -> Result<SurfaceName, Diagnostics> {
    SurfaceName::new(&entry.surface).map_err(|_| {
        Diagnostics::from(problem(
            "unknown_endpoint",
            Stage::Resolve,
            at,
            "The page request's surface has no well-formed name.",
            "Name a surface the target declares.",
        ))
    })
}

/// The arguments of a write's body, each an expression of the binding's
/// `body` for a field the entry declares, of a type its kind takes.
fn body_of(
    cx: &LowerContext<'_>,
    binding: &ResolvedBinding<'_>,
    name: &str,
    write: &WriteForm,
    problems: &mut Diagnostics,
) -> Vec<(String, StepArg)> {
    let mut args = Vec::new();
    let given = binding
        .field("body")
        .map(Located::entries)
        .unwrap_or_default();
    for entry in given {
        let field = &entry.key.value;
        let Some(kind) = write.fields.get(field) else {
            problems.push(problem(
                "body_undeclared",
                Stage::Resolve,
                &entry.key.at,
                format!(
                    "The page request `{}` declares no body field named `{}`.",
                    shown(name),
                    shown(field)
                ),
                "Add the field to the entry's `request_schema`, or remove it from the binding.",
            ));
            continue;
        };
        match cx.expr(&entry.value, None) {
            Ok(index) => {
                let fits = cx.expr_type(index).is_some_and(|ty| match kind {
                    Kind::String => matches!(ty, ValueType::Text | ValueType::Choice),
                    Kind::Integer => ty == ValueType::Int,
                    Kind::Boolean => ty == ValueType::Bool,
                    _ => false,
                });
                if !fits {
                    problems.push(problem(
                        "value_type",
                        Stage::TypeCheck,
                        &entry.value.at,
                        format!(
                            "The value for the body field `{}` is not of the kind the entry declares, `{}`.",
                            shown(field),
                            kind.word()
                        ),
                        "Give a value of that kind: text for a string, a whole number for an integer, a truth value for a boolean.",
                    ));
                }
                args.push((
                    field.clone(),
                    StepArg {
                        name: format!("{BODY_ARG}{field}"),
                        value: index,
                    },
                ));
            }
            Err(wrong) => problems.push(wrong),
        }
    }
    for required in &write.required {
        if !given.iter().any(|entry| entry.key.value == *required) {
            problems.push(problem(
                "body_incomplete",
                Stage::Resolve,
                &binding.doc.at,
                format!(
                    "The page request `{}` requires the body field `{}`, and the binding gives it no value.",
                    shown(name),
                    shown(required)
                ),
                "Add the field under the binding's `body`.",
            ));
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

    /// A `request` saves exactly when its entry writes, and the lowering
    /// marks it then; the driver refuses the mismatch both ways. A `goto`
    /// never saves.
    fn commits(&self, kind: &StepKind) -> StepCommits {
        if *kind == REQUEST {
            StepCommits::AsDeclared
        } else {
            StepCommits::Never
        }
    }

    /// The HTTP pack's rule, for one meaning of a verb across the modes: a
    /// verb that reads is a read; an `update` that names how its record is
    /// read back is a write, and one that names nothing is `irreversible`,
    /// since what was written could never be found again.
    fn least_effect(&self, binding: &ResolvedBinding<'_>) -> EffectClass {
        if READ_VERBS.contains(&binding.verb.as_str()) {
            EffectClass::Read
        } else if binding.field("read_back").is_some() {
            EffectClass::Write
        } else {
            EffectClass::Irreversible
        }
    }

    fn binding_schema(&self) -> &JsonSchema {
        &self.schema
    }

    fn identity(&self, binding: &ResolvedBinding<'_>, strategy: IdentityStrategy) -> Possible {
        let reads = READ_VERBS.contains(&binding.verb.as_str());
        match strategy {
            IdentityStrategy::None if reads => Possible::Yes,
            IdentityStrategy::Natural | IdentityStrategy::Composite
                if reads || binding.field("read_back").is_some() =>
            {
                Possible::Yes
            }
            _ => Possible::No(
                "a `page_http` write is an `update` of a record found again by its key, through the read its `read_back` names.".to_owned(),
            ),
        }
    }

    fn lower(
        &self,
        binding: &ResolvedBinding<'_>,
        cx: &LowerContext<'_>,
    ) -> Result<Lowered, Diagnostics> {
        let at = &binding.doc.at;
        let reads = READ_VERBS.contains(&binding.verb.as_str());
        if reads && cx.operation().effect() != EffectClass::Read {
            return Err(Diagnostics::from(problem(
                "page_http_writes",
                Stage::Analyze,
                at,
                "A `page_http` read is bound to an operation declared to write. A write from inside the page is an `update` through an entry that declares it.",
                "Make this a read with `effect: read`, or bind the write with the verb `update` to a write entry.",
            )));
        }
        let found = found(cx, binding.doc)?;
        if reads {
            Self::lower_read(binding, cx, &found)
        } else {
            Self::lower_write(binding, cx, &found)
        }
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

/// What a session keeps to open its acting tab, once a write's `goto` asks
/// for it.
struct ToAct {
    browsers: Browsers,
    profile: ProfileKey,
    environment: PageEnvironment,
    origins: Origins,
}

/// What one run holds: its tabs and the page requests of its plan.
///
/// Every session has an observing tab, opened at prepare, in which every
/// read runs. An acting tab, in the same profile, is opened only when a
/// write's `goto` asks for one, and only in a posture that may act.
struct Session {
    looking: ObserveTab,
    acting: Option<ActTab>,
    /// The tab the last `goto` chose.
    on: TabKind,
    /// How to open the acting tab; `None` in a posture that may only look.
    to_act: Option<ToAct>,
    origin: String,
    sections: BTreeMap<usize, RequestSection>,
    /// What the run may do. A write is sent in a live run, and against a
    /// declared test record for the step marked as the commit point.
    posture: Posture,
}

/// What a write's answer is, by what its entry declares.
enum Verdict {
    /// A status that means it was written, in the declared shape.
    Written,
    /// The application refused the page's token.
    TokenRefused,
    /// The application refused the values, with what it said.
    Refused(Option<String>),
    /// Anything else: whether it was written is not known.
    Unknown(String),
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

    /// What every tab refused since it was last asked.
    fn refused(&self, what: &str) -> Option<a2m_core::Failure> {
        let mut refused = self.looking.blocked();
        if let Some(acting) = &self.acting {
            refused.extend(acting.blocked());
        }
        refusal(&refused, what)
    }

    /// The tab for a `goto`: the acting tab only for a write's own `goto`,
    /// opened then, in a posture that may act; the observing tab in every
    /// other case. A plan compiled before a `goto` said its tab held reads
    /// alone, so a `goto` that does not say is the observing tab's.
    async fn choose(&mut self, step: &Step) -> Result<(), StepFailure> {
        let wanted = if step.payload.is_none() {
            TabKind::Observe
        } else {
            step.payload
                .decode::<GotoStep>()
                .map(|goto| goto.tab)
                .map_err(|_| codes::not_usable("its payload does not decode"))?
        };
        let Some(to_act) = self.to_act.as_ref().filter(|_| wanted == TabKind::Act) else {
            self.on = TabKind::Observe;
            return Ok(());
        };
        if self.acting.is_none() {
            let browser = to_act
                .browsers
                .open(&to_act.profile, Visibility::Background)
                .await
                .map_err(|error| browser_failure(&error, "the start of the browser"))?;
            let tab = browser
                .acting_tab(&to_act.environment, &to_act.origins)
                .await
                .map_err(|error| browser_failure(&error, "the opening of a tab"))?;
            self.acting = Some(tab);
        }
        self.on = TabKind::Act;
        Ok(())
    }

    async fn goto(
        &mut self,
        step: &Step,
        input: &StepInput<'_>,
    ) -> Result<StepOutcome, StepFailure> {
        self.choose(step).await?;
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
        let deadline = until(step, input);
        let loaded = match (self.on, &self.acting) {
            (TabKind::Act, Some(tab)) => tab.goto(&address, deadline).await,
            _ => self.looking.goto(&address, deadline).await,
        }
        .map_err(|error| browser_failure(&error, &what))?;
        let Some(rest) = loaded.address.strip_prefix(&self.origin) else {
            return Err(StepFailure::answered(codes::surface_not_declared(
                "The address the page moved to",
            )));
        };
        if !(rest.is_empty() || rest.starts_with(['/', '?', '#'])) {
            return Err(StepFailure::answered(codes::surface_not_declared(
                "The address the page moved to",
            )));
        }
        // The page must be the one asked for. One answered from another
        // path, as a sign-in page is once the session has ended, holds
        // neither the session nor the token the request needs: nothing is
        // sent from it.
        let landed = rest.split(['?', '#']).next().unwrap_or_default();
        if landed != path {
            return Err(StepFailure::not_sent(fail(
                FailureClass::UnknownVariant,
                codes::UNEXPECTED_PAGE,
                format!(
                    "The page of the request `{name}` was answered from another address, as a sign-in page is when the session has ended. Nothing was sent from it."
                ),
                "Sign in again if the session has ended; otherwise look at where the application sends the page, and correct the entry's `page`.",
            )));
        }
        let status = loaded
            .status
            .map_or_else(|| "none".to_owned(), |status| status.to_string());
        let tab = match self.on {
            TabKind::Act => "act",
            TabKind::Observe => "observe",
        };
        Ok(StepOutcome::Done(Observed {
            captures: vec![acting(
                vec![
                    Detail::new("step", "goto"),
                    Detail::new("request", name),
                    Detail::new("page", pattern(&entry.page)),
                    Detail::new("tab", tab),
                    Detail::new("status", status),
                ],
                Vec::new(),
            )],
            ..Observed::default()
        }))
    }

    /// The path of a request, from the step's arguments.
    fn path_of(entry: &PageRequest, input: &StepInput<'_>) -> Result<PagePath, StepFailure> {
        let mut values = BTreeMap::new();
        for part in &entry.path {
            if let crate::model::Segment::Hole(hole) = part {
                let value = input.args.get(&format!("{PATH_ARG}{hole}"));
                values.insert(hole.clone(), segment(hole, value)?);
            }
        }
        // The browser engine checks the values again as it makes the path.
        PagePath::fill(&pattern(&entry.path), &values).map_err(StepFailure::not_sent)
    }

    async fn request(
        &self,
        step: &Step,
        input: &StepInput<'_>,
    ) -> Result<StepOutcome, StepFailure> {
        let (name, entry) = self.entry(step)?;
        match &entry.write {
            Some(write) => self.write(step, input, name, entry, write).await,
            None if step.commits => Err(codes::not_usable(
                "it is marked as a commit point, and its page request reads",
            )),
            None => self.read(step, input, name, entry).await,
        }
    }

    async fn read(
        &self,
        step: &Step,
        input: &StepInput<'_>,
        name: &str,
        entry: &PageRequest,
    ) -> Result<StepOutcome, StepFailure> {
        let payload: RequestStep = step
            .payload
            .decode()
            .map_err(|_| codes::not_usable("its payload does not decode"))?;
        let path = Self::path_of(entry, input)?;
        let what = format!("the request `{name}`");
        // A read runs in the observing tab, whichever tab the last `goto`
        // chose (decision 0195).
        let answer = self
            .looking
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
        if step.lookup && entry.absent.contains(&answer.status) {
            means.push(Detail::new("found", "absent"));
            return Ok(StepOutcome::Absent(Observed {
                captures: vec![acting(means, Vec::new())],
                ..Observed::default()
            }));
        }
        if !entry.is_ok(answer.status) {
            return Err(StepFailure::answered(fail(
                FailureClass::UnknownVariant,
                codes::UNEXPECTED_STATUS,
                format!(
                    "The request `{name}` was answered with the status {}, which it does not declare.",
                    answer.status
                ),
                "Look at what the application answers; the entry declares the statuses it reads.",
            )));
        }
        let fields = checked(name, entry, &answer).map_err(|why| mismatch(name, why))?;
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

    /// The one request of a write: a PUT from inside the page in the
    /// acting tab, with the page's own token. Every failure before the
    /// request may have left says so (`NotSent`); after it, only an answer
    /// the entry declares as a refusal settles it, and every other outcome
    /// leaves the write in doubt, never "not written".
    async fn write(
        &self,
        step: &Step,
        input: &StepInput<'_>,
        name: &str,
        entry: &PageRequest,
        write: &WriteForm,
    ) -> Result<StepOutcome, StepFailure> {
        // The driver's own guard: a request whose entry writes is sent
        // only as the commit point, in a posture that may write that step.
        if !step.commits {
            return Err(codes::not_usable(
                "its page request writes, and it is not marked as the commit point",
            ));
        }
        let may = match self.posture {
            Posture::Live | Posture::Sandbox { test_records: true } => true,
            Posture::Sandbox {
                test_records: false,
            }
            | Posture::Discovery => false,
        };
        if !may {
            return Err(StepFailure::not_sent(fail(
                FailureClass::Policy,
                a2m_provider::codes::READ_ONLY_POSTURE,
                format!("The request `{name}` writes, and this run may not write."),
                "Run the operation live, or against a declared test record.",
            )));
        }
        let tab = match (self.on, &self.acting) {
            (TabKind::Act, Some(tab)) => tab,
            _ => {
                return Err(codes::not_usable(
                    "a write is sent only from the acting tab its own `goto` reached the page in",
                ));
            }
        };
        let payload: RequestStep = step
            .payload
            .decode()
            .map_err(|_| codes::not_usable("its payload does not decode"))?;
        let path = Self::path_of(entry, input)?;
        let mut fields = BTreeMap::new();
        for field in &payload.body {
            let kind = write.fields.get(field).ok_or_else(|| {
                codes::not_usable("it fills a body field its entry does not declare")
            })?;
            let value = match (kind, input.args.get(&format!("{BODY_ARG}{field}"))) {
                (Kind::String, Some(Value::Text(text))) => FieldValue::Text(text.clone()),
                (Kind::Integer, Some(Value::Int(number))) => FieldValue::Whole(*number),
                (Kind::Boolean, Some(Value::Bool(flag))) => FieldValue::Truth(*flag),
                (_, None) => continue,
                _ => {
                    return Err(StepFailure::not_sent(fail(
                        FailureClass::Input,
                        codes::STEP_NOT_USABLE,
                        format!(
                            "The value for the body field `{}` of the request `{name}` is not of the kind its entry declares.",
                            shown(field)
                        ),
                        "Give a value of the declared kind.",
                    )));
                }
            };
            fields.insert(field.clone(), value);
        }
        if let Some(missing) = write
            .required
            .iter()
            .find(|field| !fields.contains_key(*field))
        {
            return Err(StepFailure::not_sent(fail(
                FailureClass::Input,
                codes::STEP_NOT_USABLE,
                format!(
                    "The request `{name}` requires the body field `{}`, and it was given no value.",
                    shown(missing)
                ),
                "Give the operation the input the field is made from.",
            )));
        }
        let token = match &write.token {
            None => None,
            Some(token) => {
                let from = match token.from.as_str() {
                    "meta" => TokenFrom::Meta(token.from_name.clone()),
                    "input" => TokenFrom::Input(token.from_name.clone()),
                    "cookie" => TokenFrom::Cookie(token.from_name.clone()),
                    _ => {
                        return Err(codes::not_usable(
                            "its token is read from a place this version does not know",
                        ));
                    }
                };
                let to = match token.to.as_str() {
                    "header" => TokenTo::Header(token.to_name.clone()),
                    "field" => TokenTo::Field(token.to_name.clone()),
                    _ => {
                        return Err(codes::not_usable(
                            "its token goes to a place this version does not know",
                        ));
                    }
                };
                Some(AntiForgery { from, to })
            }
        };
        let method = WriteMethod::from_word(&write.method)
            .filter(|method| *method == WriteMethod::Put)
            .ok_or_else(|| codes::not_usable("its method is not one this version sends"))?;
        let encoding = match write.encoding.as_str() {
            "json" => BodyEncoding::Json,
            "form" => BodyEncoding::Form,
            _ => {
                return Err(codes::not_usable(
                    "its body is written in a way this version does not know",
                ));
            }
        };
        let made =
            PageWrite::new(method, path, encoding, fields, token).map_err(StepFailure::not_sent)?;
        let what = format!("the request `{name}`");
        let answer = match tab.write_from_page(&made, until(step, input)).await {
            Ok(answer) => answer,
            Err(error) if error.dispatch() == Dispatch::NotSent => {
                let failed = browser_failure(&error, &what);
                return Err(StepFailure::not_sent(failed.failure));
            }
            // From here the write may have reached the application.
            Err(error) => {
                return Err(in_doubt(name, &format!("then {error}")));
            }
        };
        let mut means = vec![
            Detail::new("step", "request"),
            Detail::new("request", name),
            Detail::new("method", method.as_str()),
            Detail::new("path", pattern(&entry.path)),
            Detail::new("fields", payload.body.join(",")),
            Detail::new(
                "token",
                write.token.as_ref().map_or_else(
                    || "none".to_owned(),
                    |token| format!("{}:{}", token.from, token.to),
                ),
            ),
            Detail::new("status", answer.status.to_string()),
        ];
        match verdict(name, entry, write, &answer) {
            Verdict::Written => {
                means.push(Detail::new("answer", "written"));
                Ok(StepOutcome::Done(Observed {
                    captures: vec![acting(means, Vec::new())],
                    ..Observed::default()
                }))
            }
            // Never taken for "not written": a browser sends a write again
            // by itself when a connection it reused closed before any
            // answer, and the token the page gave was then used by the
            // first, which may have landed. Measured with the stand's
            // fault "lost after commit" (decision 0206).
            Verdict::TokenRefused => Err(StepFailure::unknown(fail(
                FailureClass::PossibleCommit,
                a2m_provider::codes::NO_CONFIRMATION,
                format!(
                    "The application refused the request `{name}` for its anti-forgery token, with an answer the entry declares for that: the page's token is stale or used, perhaps by an earlier sending of this same write."
                ),
                "Reconcile the record: read it back before anything is written again; then run the tool again, and the page is loaded afresh with a new token.",
            ))),
            Verdict::Refused(said) => {
                let refused = StepFailure::answered(fail(
                    FailureClass::Rejected,
                    codes::APP_VALIDATION_ERROR,
                    format!(
                        "The application refused the request `{name}` with the status {}, which the entry declares as a refusal.",
                        answer.status
                    ),
                    "Correct the values and run the tool again; the application refused them.",
                ));
                Err(match said {
                    Some(said) => refused.with_said(said),
                    None => refused,
                })
            }
            Verdict::Unknown(why) => Err(in_doubt(name, &why)),
        }
    }
}

/// A write that may have reached the application, and of which nothing
/// says what became of it: in doubt, reconciled by reading the record
/// before anything is sent again.
fn in_doubt(name: &str, why: &str) -> StepFailure {
    StepFailure::unknown(fail(
        FailureClass::PossibleCommit,
        a2m_provider::codes::NO_CONFIRMATION,
        format!("The request `{name}` was sent from inside the page, and {why}."),
        "Reconcile the record: read it back before anything is written again.",
    ))
}

/// What a write's answer is, by its entry. A status the entry does not
/// declare, a declared refusal whose fields do not match, and a written
/// status in a shape the entry does not declare are all unknown.
fn verdict(name: &str, entry: &PageRequest, write: &WriteForm, answer: &PageAnswer) -> Verdict {
    let json = answer.media_type == "application/json" || answer.media_type.ends_with("+json");
    let body: Option<Json> = if json {
        serde_json::from_str(&answer.body).ok()
    } else {
        None
    };
    if entry.is_ok(answer.status) {
        return match checked(name, entry, answer) {
            Ok(_) => Verdict::Written,
            Err(why) => Verdict::Unknown(format!(
                "it was answered with the status {} in a shape the entry does not declare: {why}",
                answer.status
            )),
        };
    }
    let token_refused = write
        .token
        .iter()
        .flat_map(|token| &token.refused)
        .any(|declared| declared.matches(answer.status, body.as_ref()));
    if token_refused {
        return Verdict::TokenRefused;
    }
    if write
        .refused
        .iter()
        .any(|declared| declared.matches(answer.status, body.as_ref()))
    {
        let said = body
            .as_ref()
            .and_then(|body| body.get("message"))
            .and_then(Json::as_str)
            .map(str::to_owned);
        return Verdict::Refused(said);
    }
    Verdict::Unknown(format!(
        "it was answered with the status {}, which the entry does not declare",
        answer.status
    ))
}

/// The answer's object, when it is the shape the entry declares; otherwise
/// where it differs, never quoting a value.
fn checked(
    _name: &str,
    entry: &PageRequest,
    answer: &PageAnswer,
) -> Result<serde_json::Map<String, Json>, String> {
    let json = answer.media_type == "application/json" || answer.media_type.ends_with("+json");
    let body: Option<Json> = if json {
        serde_json::from_str(&answer.body).ok()
    } else {
        None
    };
    let Some(Json::Object(fields)) = body else {
        return Err("the answer is not a JSON object".to_owned());
    };
    for required in &entry.required {
        if !fields.contains_key(required) {
            return Err(format!(
                "the answer lacks `{}`, which it declares as required",
                shown(required)
            ));
        }
    }
    for (field, value) in &fields {
        match entry.properties.get(field) {
            Some(kind) if !kind.fits(value) => {
                return Err(format!(
                    "the answer's `{}` is not a {}",
                    shown(field),
                    kind.word()
                ));
            }
            None if entry.closed => {
                return Err("the answer holds a name it does not declare, and it declares that it holds no other".to_owned());
            }
            _ => {}
        }
    }
    Ok(fields)
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
        // Every session looks through an observing tab, in which the
        // browser fails every request of the page that could change the
        // system, and none is marked safe. Every read runs there.
        let looking = self
            .browsers
            .observer()
            .observing_tab(&profile, &environment, &SafeRequests::default(), &origins)
            .await
            .map_err(|error| browser_failure(&error, "the opening of a tab"))?;
        let may_act = matches!(
            cx.posture,
            Posture::Live | Posture::Sandbox { test_records: true }
        );
        Ok(Session {
            looking,
            acting: None,
            on: TabKind::Observe,
            to_act: may_act.then(|| ToAct {
                browsers: self.browsers.clone(),
                profile,
                environment,
                origins,
            }),
            origin,
            sections,
            posture: cx.posture,
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
            if let Some(failure) = session.refused(&what) {
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
            // What a tab refused during the step undoes what it read: the
            // page tried to leave, or to write where it may only read. After
            // a write was sent, it leaves the write in doubt.
            let what = format!("the end of a `{}` step", step.kind);
            match session.refused(&what) {
                Some(failure)
                    if step.commits
                        && !matches!(&done, Err(failed) if failed.sent == a2m_provider::Sent::NotSent) =>
                {
                    Err(StepFailure::unknown(fail(
                        FailureClass::PossibleCommit,
                        a2m_provider::codes::NO_CONFIRMATION,
                        format!(
                            "The write was sent from inside the page, and then the page did something no rule covers: {}",
                            failure.message()
                        ),
                        "Reconcile the record: read it back before anything is written again.",
                    )))
                }
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
