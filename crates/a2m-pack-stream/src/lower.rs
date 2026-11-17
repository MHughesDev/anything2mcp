//! The lowering of the `stream` mode: each verb of spec section 309 into
//! steps, with this pack's own details as the step's opaque payload.
//!
//! The channel is named in the binding and resolved here, at compile time.
//! A step refers to it by `refers_to`; no step argument can name one.

use a2m_core::{
    EffectClass, FailsAs, FailureClass, FileKindName, Hazard, IdentityStrategy, Mode, PackPayload,
    PackRef, Step, StepArg, StepKind, SurfaceKind, ValueType, Verb,
};
use a2m_files::{
    Diagnostics, Doc, JsonSchema, Located, LowerContext, Lowered, Lowering, Possible,
    ResolvedBinding, Stage,
};
use serde::{Deserialize, Serialize};

use crate::channel::{
    ChannelEntry, Ownership, Role, Shape, duration_ms, problem, read_channel,
};
use crate::codes;
use crate::names::{
    BROKER, CHANNEL, STEP_ACKNOWLEDGE, STEP_AWAIT, STEP_BROWSE, STEP_LATEST, STEP_PUBLISH,
    STEP_SCAN, STEP_SUBSCRIBE, VERB_AWAIT, VERB_LIST, VERB_PUBLISH, VERB_READ, VERB_TAKE,
};

/// The time a step that does not wait for a message may take, when the
/// target states no default of its own.
const STEP_TIMEOUT_MS: u32 = 10_000;

/// The argument that carries the message key.
pub(crate) const ARG_KEY: &str = "key";
/// The argument that carries the correlation value to match.
pub(crate) const ARG_CORRELATION: &str = "correlation";
/// The argument that carries the log position a wait starts from.
pub(crate) const ARG_FROM: &str = "from";
/// What starts the name of an argument that is one field of the body.
pub(crate) const ARG_BODY: &str = "body.";
/// What starts the name of an argument that is one header.
pub(crate) const ARG_HEADER: &str = "header.";

/// The pack's own details of one step. Every field has a meaning for some
/// step kinds only; the driver reads the ones its step kind uses.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StepDetail {
    /// `await`: the longest the wait may last, in milliseconds.
    pub wait_ms: u32,
    /// `await`: whether nothing arriving stops the run.
    pub fail_on_nothing: bool,
    /// `await`: whether a message must carry the key in the `key` argument.
    pub by_key: bool,
    /// `await`: whether a message must carry the value in the `correlation`
    /// argument in the channel's correlation field.
    pub by_correlation: bool,
    /// `subscribe` and `await`: whether the run takes the message, to
    /// acknowledge it in a later step.
    pub take: bool,
    /// `channels`, `describe`, `browse` sent by the discoverer: the channel
    /// as the broker names it. Honoured only where nothing may be written.
    pub address: Option<String>,
    /// `browse` sent by the discoverer: how many messages to look at.
    pub sample: u32,
}

impl StepDetail {
    /// The details as a step's payload.
    pub(crate) fn encode(&self) -> PackPayload {
        // A struct of flags, whole numbers, and text always encodes.
        PackPayload::encode(self).unwrap_or_default()
    }
}

/// The lowering of every verb of the mode.
pub(crate) struct StreamLowering {
    mode: Mode,
    schema: JsonSchema,
    verbs: Vec<Verb>,
    step_kinds: Vec<StepKind>,
    ceiling_ms: u32,
}

impl StreamLowering {
    pub(crate) fn new(mode: Mode, ceiling_ms: u32) -> Self {
        Self {
            mode,
            schema: JsonSchema::object()
                .required("channel", JsonSchema::name())
                .optional("key", JsonSchema::map_of(JsonSchema::expression()))
                .optional("headers", JsonSchema::map_of(JsonSchema::expression()))
                .optional("body", JsonSchema::map_of(JsonSchema::expression()))
                .optional("correlation", JsonSchema::expression())
                .optional("from", JsonSchema::expression())
                .optional("wait", JsonSchema::text())
                .optional("on_nothing", JsonSchema::words(&["answer", "fail"]))
                .optional(
                    "read_back",
                    JsonSchema::object()
                        .optional("verb", JsonSchema::words(&["read"]))
                        .required("channel", JsonSchema::name()),
                ),
            verbs: vec![VERB_READ, VERB_LIST, VERB_AWAIT, VERB_PUBLISH, VERB_TAKE],
            step_kinds: vec![
                STEP_LATEST,
                STEP_BROWSE,
                STEP_SUBSCRIBE,
                STEP_AWAIT,
                STEP_PUBLISH,
                STEP_ACKNOWLEDGE,
                STEP_SCAN,
            ],
            ceiling_ms,
        }
    }
}

/// What one binding is lowered with.
struct Work<'a, 'b> {
    binding: &'a ResolvedBinding<'b>,
    cx: &'a LowerContext<'b>,
    name: &'a str,
    entry: &'a ChannelEntry,
    section: a2m_core::SectionIndex,
    timeout_ms: u32,
    problems: Diagnostics,
}

impl Work<'_, '_> {
    fn wrong(&mut self, rule: &'static str, at: &Located<Doc>, message: String, fix: &str) {
        self.problems
            .push(problem(rule, Stage::Lower, &at.at, message, fix));
    }

    /// Refuses a field the verb cannot use.
    fn no_field(&mut self, key: &str) {
        if let Some(field) = self.binding.field(key) {
            let verb = self.binding.verb.clone();
            self.wrong(
                "binding_invalid",
                field,
                format!("The verb `{verb}` has no use for `{key}`."),
                "Remove the field, or choose the verb that uses it.",
            );
        }
    }

    fn arg(
        &mut self,
        name: String,
        doc: &Located<Doc>,
        expected: Option<&ValueType>,
    ) -> Option<StepArg> {
        match self.cx.expr(doc, expected) {
            Ok(value) => Some(StepArg { name, value }),
            Err(found) => {
                self.problems.push(found);
                None
            }
        }
    }

    /// The `key` argument: the one entry of the binding's `key` map.
    fn key_arg(&mut self, required: bool) -> Option<StepArg> {
        let Some(key) = self.binding.field("key") else {
            if required {
                let verb = self.binding.verb.clone();
                self.wrong(
                    "binding_invalid",
                    self.binding.doc,
                    format!("The verb `{verb}` on the channel `{}` needs the message key.", self.name),
                    "Add `key:` with the key field and the expression that gives it, such as `key: {order_id: input.order_id}`.",
                );
            }
            return None;
        };
        let [only] = key.entries() else {
            self.wrong(
                "binding_invalid",
                key,
                "A message has one key, and `key` does not have exactly one entry.".to_owned(),
                "Write one entry: the key field and the expression that gives it.",
            );
            return None;
        };
        if let Some(field) = &self.entry.key_field
            && *field != only.key.value
        {
            self.wrong(
                "binding_invalid",
                key,
                format!(
                    "The channel `{}` takes its key from `{field}`, and the binding gives a key named `{}`.",
                    self.name, only.key.value
                ),
                "Name the key as the channel file does.",
            );
            return None;
        }
        self.arg(ARG_KEY.to_owned(), &only.value, Some(&ValueType::Text))
    }

    /// One argument per entry of a map of the binding, each named with
    /// `prefix` and the entry's key.
    fn map_args(&mut self, key: &str, prefix: &str, expected: Option<&ValueType>) -> Vec<StepArg> {
        let Some(map) = self.binding.field(key) else {
            return Vec::new();
        };
        let mut args = Vec::new();
        for entry in map.entries() {
            if let Some(arg) = self.arg(format!("{prefix}{}", entry.key.value), &entry.value, expected) {
                args.push(arg);
            }
        }
        args
    }

    fn step(&self, kind: StepKind, args: Vec<StepArg>, detail: &StepDetail) -> Step {
        let (class, code) = if kind == STEP_PUBLISH {
            (FailureClass::Rejected, codes::BROKER_REFUSED)
        } else if kind == STEP_AWAIT {
            (FailureClass::Transient, codes::NOTHING_ARRIVED)
        } else {
            (FailureClass::Drift, codes::RESPONSE_SCHEMA_MISMATCH)
        };
        Step {
            commits: kind == STEP_PUBLISH || kind == STEP_ACKNOWLEDGE,
            lookup: kind == STEP_LATEST || kind == STEP_SCAN,
            idempotent: kind == STEP_PUBLISH
                && self.entry.broker_dedupe
                && self.entry.token_header.is_some(),
            timeout_ms: self.timeout_ms.saturating_add(detail.wait_ms),
            kind,
            refers_to: Some(PackRef {
                section: self.section,
                entry: self.name.to_owned(),
            }),
            args,
            fails_as: FailsAs { class, code },
            when: None,
            payload: detail.encode(),
        }
    }

    /// The wait limit of an `await` or a `take`: the binding's, or the
    /// channel's default, and never above the channel's longest or the
    /// settings' ceiling.
    fn wait_ms(&mut self, ceiling_ms: u32) -> u32 {
        let Some(wait) = self.binding.field("wait") else {
            return self.entry.wait_default_ms;
        };
        let Some(ms) = wait.as_text().and_then(duration_ms) else {
            self.wrong(
                "wait_unbounded",
                wait,
                "This is not a length of time.".to_owned(),
                "Write a whole number and a unit, such as `500ms`, `10s`, or `1m`.",
            );
            return self.entry.wait_default_ms;
        };
        let longest = self.entry.wait_max_ms.min(ceiling_ms);
        if ms > longest {
            self.wrong(
                "wait_unbounded",
                wait,
                format!(
                    "The wait is {ms} ms, above the {longest} ms the channel `{}` and the settings allow.",
                    self.name
                ),
                "Shorten the wait, or raise `wait.max` in the channel file where the settings' ceiling allows.",
            );
        }
        ms
    }

    fn read(&mut self) -> Vec<Step> {
        for key in ["headers", "body", "correlation", "from", "wait", "on_nothing", "read_back"] {
            self.no_field(key);
        }
        if self.entry.shape != Shape::Retained {
            self.wrong(
                "verb_needs_shape",
                self.binding.doc,
                format!(
                    "`read` gives the last retained value for a key, and the channel `{}` is a {}.",
                    self.name,
                    self.entry.shape.as_str()
                ),
                "Use `list` to look at what the channel holds, or `await` to wait for a message.",
            );
        }
        let args: Vec<StepArg> = self.key_arg(true).into_iter().collect();
        vec![self.step(STEP_LATEST, args, &StepDetail::default())]
    }

    fn list(&mut self) -> Vec<Step> {
        for key in ["key", "headers", "body", "correlation", "wait", "on_nothing", "read_back"] {
            self.no_field(key);
        }
        if !self.entry.browse {
            self.wrong(
                "verb_needs_shape",
                self.binding.doc,
                format!(
                    "`list` looks at a channel without taking anything, and the channel `{}` does not declare `read_through: browse`.",
                    self.name
                ),
                "Declare `read_through: browse` in the channel file if the broker offers a look that takes nothing; otherwise the channel cannot be listed.",
            );
        }
        let mut args = Vec::new();
        if let Some(from) = self.binding.field("from") {
            if self.entry.shape == Shape::Log {
                args.extend(self.arg(ARG_FROM.to_owned(), from, Some(&ValueType::Int)));
            } else {
                self.no_field("from");
            }
        }
        vec![self.step(STEP_BROWSE, args, &StepDetail::default())]
    }

    /// The `subscribe` and `await` steps of a wait.
    fn wait_steps(&mut self, ceiling_ms: u32, take: bool) -> Vec<Step> {
        let mut detail = StepDetail {
            wait_ms: self.wait_ms(ceiling_ms),
            fail_on_nothing: take || self.binding.doc.text("on_nothing") == Some("fail"),
            take,
            ..StepDetail::default()
        };
        let mut subscribe_args = Vec::new();
        if let Some(from) = self.binding.field("from") {
            if self.entry.shape == Shape::Log && self.entry.subscription.is_none() {
                subscribe_args.extend(self.arg(ARG_FROM.to_owned(), from, Some(&ValueType::Int)));
            } else {
                self.wrong(
                    "binding_invalid",
                    from,
                    "`from` is a position in a log read through a cursor of the run's own; this channel has no positions, or is read through a named subscription that keeps its own.".to_owned(),
                    "Remove `from`.",
                );
            }
        }
        let mut await_args = Vec::new();
        if let Some(key) = self.key_arg(false) {
            detail.by_key = true;
            await_args.push(key);
        }
        if let Some(correlation) = self.binding.field("correlation") {
            if self.entry.correlation.is_some() {
                detail.by_correlation = true;
                await_args.extend(self.arg(
                    ARG_CORRELATION.to_owned(),
                    correlation,
                    Some(&ValueType::Text),
                ));
            } else {
                self.wrong(
                    "binding_invalid",
                    correlation,
                    format!(
                        "The channel `{}` declares no correlation field to match against.",
                        self.name
                    ),
                    "Add `correlation: {field: ...}` to the channel file, or match by `key`.",
                );
            }
        }
        let subscribe = self.step(
            STEP_SUBSCRIBE,
            subscribe_args,
            &StepDetail {
                take,
                ..StepDetail::default()
            },
        );
        vec![subscribe, self.step(STEP_AWAIT, await_args, &detail)]
    }

    fn await_(&mut self, ceiling_ms: u32) -> Vec<Step> {
        for key in ["headers", "body", "read_back"] {
            self.no_field(key);
        }
        if self.entry.shape == Shape::Queue {
            self.wrong(
                "verb_needs_shape",
                self.binding.doc,
                format!(
                    "Waiting on the queue `{}` would take messages that others expect to read.",
                    self.name
                ),
                "Use `list` to look at the queue, or declare `ownership: runtime_owned` and use `take`.",
            );
        } else if !self.entry.own_subscription {
            self.wrong(
                "verb_needs_shape",
                self.binding.doc,
                format!(
                    "`await` reads through a subscription of the runtime's own, and the channel `{}` does not declare `read_through: own_subscription`.",
                    self.name
                ),
                "Declare `read_through: own_subscription` in the channel file, with the `subscription` a person made for the runtime.",
            );
        }
        self.wait_steps(ceiling_ms, false)
    }

    fn take(&mut self, ceiling_ms: u32) -> Vec<Step> {
        for key in ["headers", "body", "from", "on_nothing", "read_back"] {
            self.no_field(key);
        }
        if self.entry.shape != Shape::Queue || self.entry.ownership != Ownership::RuntimeOwned {
            self.wrong(
                "ack_not_owned",
                self.binding.doc,
                format!(
                    "`take` acknowledges a message, and the channel `{}` is not a queue the target declares the runtime owns.",
                    self.name
                ),
                "Declare `ownership: runtime_owned` only when nothing else reads the queue; otherwise look with `list`.",
            );
        }
        let operation = self.binding.operation;
        if operation.effect() != EffectClass::Irreversible
            || operation.strategy() != IdentityStrategy::None
        {
            self.wrong(
                "publish_effect",
                self.binding.doc,
                "A message that is taken and acknowledged is gone from the queue and cannot be put back or read again.".to_owned(),
                "Declare `effect: irreversible` and no `key` or `identity` on the operation.",
            );
        }
        let mut steps = self.wait_steps(ceiling_ms, true);
        steps.push(self.step(STEP_ACKNOWLEDGE, Vec::new(), &StepDetail::default()));
        steps
    }

    /// The read-back steps a publish can have: by the latest value on a
    /// retained channel, by the record's token on a log.
    fn read_back(&mut self, key: Option<&StepArg>) -> Vec<Step> {
        let stated = self.binding.field("read_back");
        if let Some(read_back) = stated
            && read_back.text("channel") != Some(self.name)
        {
            self.wrong(
                "binding_invalid",
                read_back,
                "A publish is read back on the channel it was sent to.".to_owned(),
                "Name the binding's own channel under `read_back`.",
            );
        }
        match (self.entry.shape, key, &self.entry.token_header) {
            (Shape::Retained, Some(key), _) => {
                vec![self.step(STEP_LATEST, vec![key.clone()], &StepDetail::default())]
            }
            (Shape::Log, _, Some(_)) => {
                vec![self.step(STEP_SCAN, Vec::new(), &StepDetail::default())]
            }
            _ => {
                if let Some(read_back) = stated {
                    self.wrong(
                        "binding_invalid",
                        read_back,
                        format!(
                            "The channel `{}` keeps nothing the runtime can read again by a key or by the record's token, so a publish to it has no read-back.",
                            self.name
                        ),
                        "Remove `read_back`, and declare the operation `irreversible` with no identity.",
                    );
                }
                Vec::new()
            }
        }
    }

    fn publish(&mut self) -> (Vec<Step>, Vec<Step>, Vec<Step>) {
        for key in ["correlation", "from", "wait", "on_nothing"] {
            self.no_field(key);
        }
        let operation = self.binding.operation;
        let private = self.entry.is_private_state();
        if !private
            && (operation.effect() != EffectClass::Irreversible
                || operation.hazard() != Hazard::SendsOutside)
        {
            self.wrong(
                "publish_effect",
                self.binding.doc,
                format!(
                    "A message published to the channel `{}` leaves the target and cannot be recalled: the channel is not a retained state channel that declares `consumers: none`.",
                    self.name
                ),
                "Declare `effect: irreversible` and `hazard: sends_outside` on the operation.",
            );
        }
        let strategy_fits = match operation.strategy() {
            IdentityStrategy::Natural => {
                self.entry.shape == Shape::Retained && self.entry.role == Role::State
            }
            // A stamp needs somewhere the runtime can find it again: a log,
            // or a retained value. On a queue or a topic the token still
            // travels, and the broker may refuse a repeat, but nothing can be
            // read back, so the operation declares no identity.
            IdentityStrategy::Stamped => {
                self.entry.token_header.is_some()
                    && matches!(self.entry.shape, Shape::Log | Shape::Retained)
            }
            IdentityStrategy::None => true,
            IdentityStrategy::Composite | IdentityStrategy::Find => false,
        };
        if !strategy_fits {
            self.wrong(
                "publish_effect",
                self.binding.doc,
                format!(
                    "The identity strategy `{}` cannot find a message on the channel `{}` again.",
                    operation.strategy(),
                    self.name
                ),
                "Use `natural` by the key on a retained state channel; `stamped` where the channel declares an idempotency header and keeps what was sent, as a log does; otherwise no identity, with `effect: irreversible`.",
            );
        }
        let key = self.key_arg(self.entry.shape == Shape::Retained);
        if self.binding.field("body").is_none() {
            self.wrong(
                "binding_invalid",
                self.binding.doc,
                "A publish states the message body, field by field.".to_owned(),
                "Add `body:` with each field and the expression that gives it.",
            );
        }
        let mut args: Vec<StepArg> = key.iter().cloned().collect();
        if let (Some(headers), Some(token)) =
            (self.binding.field("headers"), &self.entry.token_header)
            && headers.get(token).is_some()
        {
            self.wrong(
                "binding_invalid",
                headers,
                format!("The header `{token}` carries the record's idempotency token, which the ledger gives."),
                "Remove the header from the binding.",
            );
        }
        args.extend(self.map_args("headers", ARG_HEADER, Some(&ValueType::Text)));
        args.extend(self.map_args("body", ARG_BODY, None));
        let read_back = self.read_back(key.as_ref());
        let lookup = if operation.strategy() == IdentityStrategy::Stamped
            && self.entry.shape == Shape::Log
        {
            vec![self.step(STEP_SCAN, Vec::new(), &StepDetail::default())]
        } else {
            Vec::new()
        };
        (
            vec![self.step(STEP_PUBLISH, args, &StepDetail::default())],
            read_back,
            lookup,
        )
    }
}

impl Lowering for StreamLowering {
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

    fn binding_schema(&self) -> &JsonSchema {
        &self.schema
    }

    /// Whether a strategy is possible depends on the channel the binding
    /// names, and this method is not shown the channel files. It refuses
    /// what no channel can do; [`Lowering::lower`] refuses the rest.
    fn identity(&self, binding: &ResolvedBinding<'_>, strategy: IdentityStrategy) -> Possible {
        let writes = *binding.verb == VERB_PUBLISH || *binding.verb == VERB_TAKE;
        match strategy {
            IdentityStrategy::Composite | IdentityStrategy::Find if writes => Possible::No(
                "a message has one key, and a lookup over a flow is a bounded scan for a stamp."
                    .to_owned(),
            ),
            _ => Possible::Yes,
        }
    }

    fn lower(
        &self,
        binding: &ResolvedBinding<'_>,
        cx: &LowerContext<'_>,
    ) -> Result<Lowered, Diagnostics> {
        let unknown = |message: String, fix: &str| -> Diagnostics {
            let at = binding.field("channel").unwrap_or(binding.doc);
            Diagnostics::from(problem("unknown_channel", Stage::Resolve, &at.at, message, fix))
        };
        let name = binding
            .field("channel")
            .and_then(Located::as_text)
            .unwrap_or_default();
        let doc = cx
            .entries(&CHANNEL, &BROKER)
            .unwrap_or_default()
            .iter()
            .find(|doc| doc.text("channel") == Some(name));
        let Some(doc) = doc else {
            return Err(unknown(
                format!("No file under `channels/` defines a channel named `{name}`."),
                "Name a channel that `channels/` defines.",
            ));
        };
        let (Ok(entry), Some(section)) = (
            read_channel(doc, self.ceiling_ms),
            cx.section_index(&CHANNEL, &BROKER),
        ) else {
            return Err(unknown(
                format!("The channel `{name}` has problems of its own, so nothing can be bound to it."),
                "Correct the channel file first.",
            ));
        };
        let surface = match doc.text("surface") {
            Some(named) => cx.target().surface(named),
            None => cx
                .target()
                .surfaces()
                .into_iter()
                .find(|surface| *surface.kind == BROKER),
        };
        let Some(surface) = surface else {
            return Err(unknown(
                format!("The channel `{name}` lives on no surface of kind `broker`."),
                "Declare a surface of kind `broker` in `target.yaml`.",
            ));
        };
        let mut work = Work {
            binding,
            cx,
            name,
            entry: &entry,
            section,
            timeout_ms: cx
                .target()
                .default_timeout_ms("step")
                .unwrap_or(STEP_TIMEOUT_MS),
            problems: Diagnostics::new(),
        };
        let (act, read_back, lookup) = if *binding.verb == VERB_READ {
            (work.read(), Vec::new(), Vec::new())
        } else if *binding.verb == VERB_LIST {
            (work.list(), Vec::new(), Vec::new())
        } else if *binding.verb == VERB_AWAIT {
            (work.await_(self.ceiling_ms), Vec::new(), Vec::new())
        } else if *binding.verb == VERB_TAKE {
            (work.take(self.ceiling_ms), Vec::new(), Vec::new())
        } else {
            work.publish()
        };
        if work.problems.is_empty() {
            Ok(Lowered {
                surface: surface.name.clone(),
                act,
                confirm: Vec::new(),
                read_back,
                lookup,
                settle_ms: 0,
                prefer_response: false,
            })
        } else {
            Err(work.problems)
        }
    }
}
