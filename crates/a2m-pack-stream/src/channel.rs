//! The pack's two file kinds: `channels/`, one document per channel, and
//! `schemas/`, one document per message schema (spec section 309, "The file
//! kind"). Each compiles to one section of the plan, which only this pack's
//! driver decodes.

use std::collections::BTreeMap;

use a2m_core::{FileKindName, LintCode, PackPayload, SurfaceKind};
use a2m_files::{
    CompiledSection, DefinedName, Diagnostic, Diagnostics, Doc, ExprSite, FileKind, JsonSchema,
    KindContext, Located, Place, RelPath, Stage,
};
use serde::{Deserialize, Serialize};

use crate::json::Json;
use crate::names::{BROKER, CHANNEL, MESSAGE_SCHEMA};

/// The version of the two sections' own shape.
pub(crate) const SECTION_FORMAT: u16 = 1;

/// The longest a broker's name for a channel, a header, or a subscription
/// may be in a channel file.
const LONGEST_WIRE_NAME: usize = 255;

macro_rules! words {
    ($(#[$doc:meta])* $name:ident { $( $(#[$vdoc:meta])* $variant:ident => $word:literal ),+ $(,)? }) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub(crate) enum $name { $( $(#[$vdoc])* $variant ),+ }

        impl $name {
            /// Every word, as a channel file writes it.
            pub(crate) const WORDS: &'static [&'static str] = &[ $( $word ),+ ];

            /// The word, as a channel file writes it.
            pub(crate) const fn as_str(self) -> &'static str {
                match self { $( Self::$variant => $word ),+ }
            }

            /// The value a word names.
            pub(crate) fn from_word(word: &str) -> Option<Self> {
                [ $( Self::$variant ),+ ]
                    .into_iter()
                    .find(|value| value.as_str() == word)
            }
        }
    };
}

words! {
    /// What a channel does with a message it accepts.
    Shape {
        /// The broker keeps the last value per key.
        Retained => "retained",
        /// The broker keeps messages in order, each at a position it reports.
        Log => "log",
        /// A message goes to one reader and is removed when acknowledged.
        Queue => "queue",
        /// Every open subscription gets a copy, and nothing is kept.
        Topic => "topic",
    }
}

words! {
    /// What a message on the channel is. A judgment the developer makes.
    Role {
        /// The latest value for a key is the record.
        State => "state",
        /// Each message is a thing that happened.
        Events => "events",
    }
}

words! {
    /// How the runtime reads without taking from others.
    ReadThrough {
        /// A subscription of the runtime's own, named in the channel file.
        OwnSubscription => "own_subscription",
        /// The broker's look that takes nothing.
        Browse => "browse",
        /// Not at all.
        None => "none",
    }
}

words! {
    /// Whether anything but the runtime reads a queue.
    Ownership {
        /// Others read it. Nothing is acknowledged.
        Shared => "shared",
        /// The target declares that nothing else reads it.
        RuntimeOwned => "runtime_owned",
    }
}

words! {
    /// Who reads what is published.
    Consumers {
        /// Something outside the target.
        Outside => "outside",
        /// Nothing outside the target.
        None => "none",
        /// Not stated; treated as outside.
        Unknown => "unknown",
    }
}

/// One channel, compiled. Everything the driver knows about where the
/// messages are and what they look like.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ChannelEntry {
    /// The surface the channel lives on, by name.
    pub surface: String,
    /// The topic, queue, or feed as the broker names it.
    pub address: String,
    /// What the channel does with a message.
    pub shape: Shape,
    /// What a message on it is. Only a publish needs it: a channel that
    /// does not say compiles for reads, and a publish to it does not.
    pub role: Option<Role>,
    /// The message schema, by the name a file under `schemas/` gives it.
    pub schema: Option<String>,
    /// The field of the body the message key is taken from.
    pub key_field: Option<String>,
    /// How a reply is matched to a request: `headers.<name>`, or a field of
    /// the body.
    pub correlation: Option<String>,
    /// Whether the broker offers a look that takes nothing.
    pub browse: bool,
    /// Whether the runtime reads through a subscription of its own.
    pub own_subscription: bool,
    /// The runtime's own subscription, as the broker names it. On a log it
    /// may be absent: the run then reads through a cursor of its own, which
    /// the broker discards.
    pub subscription: Option<String>,
    /// Whether anything but the runtime reads the queue.
    pub ownership: Ownership,
    /// Who reads what is published.
    pub consumers: Consumers,
    /// The header the record's idempotency token travels in.
    pub token_header: Option<String>,
    /// Whether the broker itself refuses a repeated token.
    pub broker_dedupe: bool,
    /// How long the broker remembers a token, in milliseconds, as the file
    /// states it: checked against what the broker reports, never longer.
    pub dedupe_window_ms: Option<u32>,
    /// The wait of a binding that states none, in milliseconds.
    pub wait_default_ms: u32,
    /// The longest wait on this channel, in milliseconds.
    pub wait_max_ms: u32,
    /// The most messages one look returns.
    pub browse_count: u32,
    /// The largest message body, in bytes of its JSON.
    pub message_bytes: u32,
}

impl ChannelEntry {
    /// Whether the latest value for a key is the record and nothing outside
    /// reads it: the one channel a publish to which is an ordinary write.
    pub(crate) fn is_private_state(&self) -> bool {
        self.shape == Shape::Retained
            && self.role == Some(Role::State)
            && self.consumers == Consumers::None
    }
}

/// A lint code of this pack.
pub(crate) fn code(rule: &'static str) -> LintCode {
    match rule {
        "channel_invalid" => LintCode::fixed("stream.channel_invalid"),
        "unknown_schema" => LintCode::fixed("stream.unknown_schema"),
        "schema_unsupported" => LintCode::fixed("stream.schema_unsupported"),
        "unknown_channel" => LintCode::fixed("stream.unknown_channel"),
        "verb_needs_shape" => LintCode::fixed("stream.verb_needs_shape"),
        "wait_unbounded" => LintCode::fixed("stream.wait_unbounded"),
        "ack_not_owned" => LintCode::fixed("stream.ack_not_owned"),
        "publish_effect" => LintCode::fixed("stream.publish_effect"),
        "consumers_unknown" => LintCode::fixed("stream.consumers_unknown"),
        "untrusted_unmarked" => LintCode::fixed("stream.untrusted_unmarked"),
        "protocol_unfit" => LintCode::fixed("stream.protocol_unfit"),
        _ => LintCode::fixed("stream.binding_invalid"),
    }
}

/// A problem in one of the pack's files.
pub(crate) fn problem(
    rule: &'static str,
    stage: Stage,
    at: &Place,
    message: impl Into<String>,
    fix: impl Into<String>,
) -> Diagnostic {
    Diagnostic::error(code(rule), stage, at, message, fix)
}

/// A length of time as a channel file writes it: a whole number and `ms`,
/// `s`, or `m`. Gives milliseconds.
pub(crate) fn duration_ms(text: &str) -> Option<u32> {
    let (digits, unit) = text.split_at(text.find(|c: char| !c.is_ascii_digit())?);
    let number: u32 = digits.parse().ok()?;
    let scale = match unit {
        "ms" => 1,
        "s" => 1_000,
        "m" => 60_000,
        _ => return None,
    };
    number.checked_mul(scale).filter(|ms| *ms > 0)
}

/// Whether text can be a broker's name for something: printable, with no
/// space at either end, and not too long.
fn is_wire_name(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= LONGEST_WIRE_NAME
        && text.trim() == text
        && !text.chars().any(char::is_control)
}

/// Reads one channel document. `ceiling_ms` is the settings' ceiling on any
/// wait. The surface is left empty; the caller fills it.
pub(crate) fn read_channel(
    doc: &Located<Doc>,
    ceiling_ms: u32,
) -> Result<ChannelEntry, Vec<Diagnostic>> {
    let mut problems = Vec::new();
    let mut wrong = |at: &Place, message: String, fix: &str| {
        problems.push(problem(
            "channel_invalid",
            Stage::Validate,
            at,
            message,
            fix,
        ));
    };
    let word = |key: &str| doc.text(key).unwrap_or_default();
    let shape = Shape::from_word(word("shape")).unwrap_or(Shape::Topic);
    let role = doc.text("role").and_then(Role::from_word);
    let read_through_at = doc.get("read_through").map_or(&doc.at, |field| &field.at);
    let read_through: Vec<ReadThrough> = doc
        .get("read_through")
        .map(|field| match field.as_text() {
            Some(one) => ReadThrough::from_word(one).into_iter().collect(),
            None => field
                .items()
                .iter()
                .filter_map(|item| item.as_text().and_then(ReadThrough::from_word))
                .collect(),
        })
        .unwrap_or_default();
    let browse = read_through.contains(&ReadThrough::Browse);
    let own_subscription = read_through.contains(&ReadThrough::OwnSubscription);
    if read_through.contains(&ReadThrough::None) && read_through.len() > 1 {
        wrong(
            read_through_at,
            "The channel is read `none` and by another path at once.".to_owned(),
            "Write `none` alone, or list the paths the broker offers.",
        );
    }
    let ownership = Ownership::from_word(word("ownership")).unwrap_or(Ownership::Shared);
    let consumers = Consumers::from_word(word("consumers")).unwrap_or(Consumers::Unknown);

    let address = word("address").to_owned();
    if !is_wire_name(&address) {
        wrong(
            doc.get("address").map_or(&doc.at, |address| &address.at),
            "The channel's address is empty, too long, or holds a control character.".to_owned(),
            "Write the topic, queue, or feed exactly as the broker names it.",
        );
    }

    let key_field = doc
        .get("key")
        .and_then(|key| key.text("field"))
        .map(str::to_owned);
    if shape == Shape::Retained && key_field.is_none() {
        wrong(
            &doc.at,
            "A retained channel keeps one value per key, and this one states no key.".to_owned(),
            "Add `key: {field: ...}` with the field of the message the key is taken from.",
        );
    }

    let correlation = doc.get("correlation").and_then(|correlation| {
        let field = correlation.get("field")?;
        let text = field.as_text()?;
        let name = text.strip_prefix("headers.").unwrap_or(text);
        if !is_wire_name(name) || name.contains('.') {
            wrong(
                &field.at,
                "A correlation field is `headers.` and a header's name, or the name of one field of the body.".to_owned(),
                "Write `headers.correlation_id`, or the body field that carries the correlation.",
            );
        }
        Some(text.to_owned())
    });

    let subscription = doc.text("subscription").map(str::to_owned);
    match (own_subscription, &subscription) {
        // On a log or a topic the run may read through a cursor or a
        // subscription of its own, which the broker discards.
        (true, None) if shape != Shape::Log && shape != Shape::Topic => wrong(
            read_through_at,
            "The channel is read through the runtime's own subscription and names none.".to_owned(),
            "Add `subscription:` with the name of a subscription a person declared on the broker for the runtime. The runtime never creates one.",
        ),
        // On a queue the runtime owns, the consumer a person made for the
        // runtime's takes.
        (false, Some(_)) if !(shape == Shape::Queue && ownership == Ownership::RuntimeOwned) => {
            wrong(
                doc.get("subscription").map_or(&doc.at, |field| &field.at),
                "The channel names a subscription and is not read through one.".to_owned(),
                "Add `own_subscription` to `read_through`, or remove `subscription`.",
            );
        }
        (_, Some(name)) if !is_wire_name(name) => wrong(
            doc.get("subscription").map_or(&doc.at, |field| &field.at),
            "The subscription's name is empty, too long, or holds a control character.".to_owned(),
            "Write the subscription exactly as the broker names it.",
        ),
        _ => {}
    }
    if browse && shape == Shape::Topic {
        wrong(
            read_through_at,
            "A topic keeps nothing, so there is nothing to look at without subscribing.".to_owned(),
            "Write `read_through: own_subscription` with the runtime's subscription, or `none`.",
        );
    }
    if own_subscription && shape == Shape::Queue {
        wrong(
            read_through_at,
            "A queue hands each message to one reader, so a subscription on it takes messages from the others.".to_owned(),
            "Write `read_through: browse` to look, or declare `ownership: runtime_owned` and use the verb `take`.",
        );
    }
    if ownership == Ownership::RuntimeOwned && shape != Shape::Queue {
        wrong(
            doc.get("ownership").map_or(&doc.at, |field| &field.at),
            "Only a queue can be owned by the runtime: on any other shape nothing is taken."
                .to_owned(),
            "Remove `ownership`, or correct `shape`.",
        );
    }

    let idempotency = doc.get("idempotency");
    let token_header = idempotency
        .and_then(|idempotency| idempotency.text("header"))
        .map(str::to_owned);
    if let (Some(header), Some(idempotency)) = (&token_header, idempotency)
        && !is_wire_name(header)
    {
        wrong(
            &idempotency.at,
            "The idempotency header's name is empty, too long, or holds a control character."
                .to_owned(),
            "Write the header exactly as the broker or its consumers read it.",
        );
    }
    let broker_dedupe = idempotency
        .and_then(|idempotency| idempotency.get("broker_dedupe"))
        .and_then(Located::as_bool)
        .unwrap_or(false);
    let dedupe_window_ms = match idempotency.and_then(|idempotency| idempotency.get("window")) {
        None => None,
        Some(field) if !broker_dedupe => {
            wrong(
                &field.at,
                "The file states how long the broker remembers a token, and says the broker refuses none.".to_owned(),
                "Write `broker_dedupe: true` with the window, or remove `window`.",
            );
            None
        }
        Some(field) => match field.as_text().and_then(duration_ms) {
            Some(ms) => Some(ms),
            None => {
                wrong(
                    &field.at,
                    "This is not a length of time.".to_owned(),
                    "Write a whole number and a unit, such as `500ms`, `10s`, or `2m`.",
                );
                None
            }
        },
    };

    let wait = doc.get("wait");
    let mut wait_ms = |key: &str, otherwise: u32| -> u32 {
        let Some(field) = wait.and_then(|wait| wait.get(key)) else {
            return otherwise;
        };
        match field.as_text().and_then(duration_ms) {
            Some(ms) => ms,
            None => {
                wrong(
                    &field.at,
                    "This is not a length of time.".to_owned(),
                    "Write a whole number and a unit, such as `500ms`, `10s`, or `1m`.",
                );
                otherwise
            }
        }
    };
    let wait_max_ms = wait_ms("max", ceiling_ms);
    let wait_default_ms = wait_ms("default", wait_max_ms.min(10_000));
    if wait_max_ms > ceiling_ms {
        wrong(
            wait.map_or(&doc.at, |wait| &wait.at),
            format!(
                "The channel's longest wait is {wait_max_ms} ms, above the settings' ceiling of {ceiling_ms} ms."
            ),
            "Lower `wait.max`, or raise `packs.stream.wait_ceiling_seconds` where that is allowed.",
        );
    }
    if wait_default_ms > wait_max_ms {
        wrong(
            wait.map_or(&doc.at, |wait| &wait.at),
            format!(
                "The channel's default wait is {wait_default_ms} ms, above its longest wait of {wait_max_ms} ms."
            ),
            "Lower `wait.default`, or raise `wait.max`.",
        );
    }

    let limit = |key: &str, otherwise: u32| -> u32 {
        doc.get("limits")
            .and_then(|limits| limits.get(key))
            .and_then(Located::as_int)
            .and_then(|number| u32::try_from(number).ok())
            .unwrap_or(otherwise)
    };

    if problems.is_empty() {
        Ok(ChannelEntry {
            surface: String::new(),
            address,
            shape,
            role,
            schema: doc.text("schema").map(str::to_owned),
            key_field,
            correlation,
            browse,
            own_subscription,
            subscription,
            ownership,
            consumers,
            token_header,
            broker_dedupe,
            dedupe_window_ms,
            wait_default_ms,
            wait_max_ms,
            browse_count: limit("browse_count", 100),
            message_bytes: limit("message_bytes", 65_536),
        })
    } else {
        Err(problems)
    }
}

/// The schema of one document under `channels/`.
pub(crate) fn channel_schema() -> JsonSchema {
    JsonSchema::object()
        .required("channel", JsonSchema::name())
        .optional("surface", JsonSchema::surface())
        .required("address", JsonSchema::text())
        .required("shape", JsonSchema::words(Shape::WORDS))
        .optional("role", JsonSchema::words(Role::WORDS))
        .optional("schema", JsonSchema::name())
        .optional("encoding", JsonSchema::words(&["json"]))
        .optional(
            "key",
            JsonSchema::object()
                .required("field", JsonSchema::text())
                .optional("type", JsonSchema::field_type()),
        )
        .optional(
            "correlation",
            JsonSchema::object().required("field", JsonSchema::text()),
        )
        .optional(
            "read_through",
            JsonSchema::either(vec![
                JsonSchema::words(ReadThrough::WORDS),
                JsonSchema::non_empty_list(JsonSchema::words(ReadThrough::WORDS)),
            ]),
        )
        .optional("subscription", JsonSchema::text())
        .optional("ownership", JsonSchema::words(Ownership::WORDS))
        .optional("consumers", JsonSchema::words(Consumers::WORDS))
        .optional(
            "idempotency",
            JsonSchema::object()
                .required("header", JsonSchema::text())
                .optional("broker_dedupe", JsonSchema::bool())
                .optional("window", JsonSchema::text()),
        )
        .optional(
            "wait",
            JsonSchema::object()
                .optional("default", JsonSchema::text())
                .optional("max", JsonSchema::text()),
        )
        .optional(
            "limits",
            JsonSchema::object()
                .optional("browse_count", JsonSchema::int_between(1, 1000))
                .optional("message_bytes", JsonSchema::int_between(1, 1_048_576)),
        )
}

fn defined(doc: &Located<Doc>, key: &str) -> Vec<Located<DefinedName>> {
    doc.get(key)
        .and_then(|name| {
            name.as_text().map(|text| {
                Located::new(
                    DefinedName {
                        name: text.to_owned(),
                    },
                    name.at.clone(),
                )
            })
        })
        .into_iter()
        .collect()
}

fn section<T: Serialize>(entries: &BTreeMap<String, T>) -> Result<CompiledSection, Diagnostics> {
    PackPayload::encode(entries)
        .map(|payload| CompiledSection {
            format: SECTION_FORMAT,
            payload,
        })
        .map_err(|failure| {
            Diagnostics::from(problem(
                "channel_invalid",
                Stage::Lower,
                &Place::start_of(&RelPath::target_file()),
                failure.message().to_owned(),
                "Report this to the pack's author: its own section did not encode.",
            ))
        })
}

/// The `channels/` file kind.
pub(crate) struct ChannelKind {
    schema: JsonSchema,
    kinds: Vec<SurfaceKind>,
    ceiling_ms: u32,
}

impl ChannelKind {
    pub(crate) fn new(ceiling_ms: u32) -> Self {
        Self {
            schema: channel_schema(),
            kinds: vec![BROKER],
            ceiling_ms,
        }
    }
}

impl FileKind for ChannelKind {
    fn name(&self) -> FileKindName {
        CHANNEL
    }

    fn surface_kinds(&self) -> &[SurfaceKind] {
        &self.kinds
    }

    fn folder(&self) -> &str {
        "channels"
    }

    fn schema(&self) -> &JsonSchema {
        &self.schema
    }

    fn names(&self, doc: &Located<Doc>) -> Vec<Located<DefinedName>> {
        defined(doc, "channel")
    }

    fn expressions(&self, _doc: &Located<Doc>) -> Vec<ExprSite> {
        Vec::new()
    }

    /// A channel file describes a write when it declares an `ownership`
    /// other than plainly `shared`: `runtime_owned` is what lets a `take`
    /// remove messages. An `idempotency` header states how the broker
    /// treats a repeat and lets nothing run, and the drafter writes it
    /// from what the broker stated; a publish is allowed by an operation
    /// and its binding, not by this file.
    fn writes(&self, doc: &Located<Doc>) -> bool {
        doc.get("ownership")
            .is_some_and(|ownership| ownership.as_text() != Some("shared"))
    }

    fn compile(
        &self,
        docs: &[Located<Doc>],
        cx: &KindContext<'_>,
    ) -> Result<CompiledSection, Diagnostics> {
        let mut entries = BTreeMap::new();
        let mut problems = Diagnostics::new();
        for doc in docs {
            let name = doc.text("channel").unwrap_or_default().to_owned();
            match read_channel(doc, self.ceiling_ms) {
                Ok(mut entry) => {
                    entry.surface = cx
                        .surface_of(doc)
                        .map(|surface| surface.name.as_str().to_owned())
                        .unwrap_or_default();
                    entries.insert(name, entry);
                }
                Err(found) => problems.extend(found),
            }
        }
        if problems.is_empty() {
            section(&entries)
        } else {
            Err(problems)
        }
    }
}

words! {
    /// The JSON types a message schema may give a field.
    JsonType {
        /// Text.
        String => "string",
        /// A whole number.
        Integer => "integer",
        /// Any number.
        Number => "number",
        /// True or false.
        Boolean => "boolean",
        /// An array.
        Array => "array",
        /// An object.
        Object => "object",
    }
}

/// What a schema says of one field of a message body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FieldRule {
    /// The field's JSON type.
    pub ty: JsonType,
    /// Whether every message has the field.
    pub required: bool,
    /// The texts the field may hold. Empty means any.
    pub allowed: Vec<String>,
}

/// The schema of a message body: an object with named fields. It is the
/// part of JSON Schema this pack checks, and a schema that needs more does
/// not compile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MessageSchema {
    /// The fields, by name.
    pub fields: BTreeMap<String, FieldRule>,
    /// Whether a field the schema does not name is refused.
    pub closed: bool,
}

impl MessageSchema {
    /// Checks a message body.
    ///
    /// # Errors
    ///
    /// What does not fit, naming the field and never its value.
    pub(crate) fn check(&self, body: &Json) -> Result<(), String> {
        let Some(map) = body.as_object() else {
            return Err(format!(
                "the body is a JSON {}, and the schema needs an object",
                body.type_name()
            ));
        };
        for (name, rule) in &self.fields {
            let Some(value) = map.get(name) else {
                if rule.required {
                    return Err(format!("the required field `{name}` is missing"));
                }
                continue;
            };
            let found = value.type_name();
            let fits =
                found == rule.ty.as_str() || (rule.ty == JsonType::Number && found == "integer");
            if !fits {
                return Err(format!(
                    "the field `{name}` is a JSON {found}, and the schema needs a {}",
                    rule.ty.as_str()
                ));
            }
            if !rule.allowed.is_empty()
                && !value
                    .as_str()
                    .is_some_and(|text| rule.allowed.iter().any(|allowed| allowed == text))
            {
                return Err(format!(
                    "the field `{name}` holds a text that is not one the schema lists"
                ));
            }
        }
        if self.closed
            && let Some(extra) = map.keys().find(|name| !self.fields.contains_key(*name))
        {
            return Err(format!(
                "the body has a field the schema does not name ({} characters long)",
                extra.chars().count()
            ));
        }
        Ok(())
    }

    /// The schema a broker registered, when it is within the part this pack
    /// checks: an object, `required`, and `properties` with a `type` and an
    /// optional `enum` of texts.
    pub(crate) fn from_registered(schema: &Json) -> Option<Self> {
        let map = schema.as_object()?;
        if map.keys().any(|key| {
            !["type", "required", "properties", "additionalProperties"].contains(&key.as_str())
        }) || map.get("type")?.as_str()? != "object"
        {
            return None;
        }
        let required: Vec<&str> = match map.get("required") {
            Some(list) => list
                .as_list()?
                .iter()
                .map(Json::as_str)
                .collect::<Option<_>>()?,
            None => Vec::new(),
        };
        let mut fields = BTreeMap::new();
        for (name, property) in map.get("properties")?.as_object()? {
            let property = property.as_object()?;
            if property.keys().any(|key| key != "type" && key != "enum") {
                return None;
            }
            let allowed = match property.get("enum") {
                Some(list) => list
                    .as_list()?
                    .iter()
                    .map(|item| item.as_str().map(str::to_owned))
                    .collect::<Option<_>>()?,
                None => Vec::new(),
            };
            fields.insert(
                name.clone(),
                FieldRule {
                    ty: JsonType::from_word(property.get("type")?.as_str()?)?,
                    required: required.contains(&name.as_str()),
                    allowed,
                },
            );
        }
        if required.iter().any(|name| !fields.contains_key(*name)) {
            return None;
        }
        Some(Self {
            fields,
            closed: map.get("additionalProperties").and_then(Json::as_bool) == Some(false),
        })
    }
}

/// Reads one schema document.
pub(crate) fn read_schema(doc: &Located<Doc>) -> Result<MessageSchema, Vec<Diagnostic>> {
    let mut problems = Vec::new();
    let required: Vec<&Located<Doc>> = doc
        .get("required")
        .map(|list| list.items().iter().collect())
        .unwrap_or_default();
    let mut fields = BTreeMap::new();
    for entry in doc
        .get("properties")
        .map(Located::entries)
        .unwrap_or_default()
    {
        let name = entry.key.value.clone();
        let allowed: Vec<String> = entry
            .value
            .get("enum")
            .map(|list| {
                list.items()
                    .iter()
                    .filter_map(|item| item.as_text().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        let ty = entry
            .value
            .text("type")
            .and_then(JsonType::from_word)
            .unwrap_or(JsonType::String);
        if !allowed.is_empty() && ty != JsonType::String {
            problems.push(problem(
                "schema_unsupported",
                Stage::Validate,
                &entry.value.at,
                format!("The field `{name}` lists allowed texts and is not of the type `string`."),
                "List allowed values only for a field of the type `string`.",
            ));
        }
        fields.insert(
            name.clone(),
            FieldRule {
                ty,
                required: required
                    .iter()
                    .any(|item| item.as_text() == Some(name.as_str())),
                allowed,
            },
        );
    }
    for item in required {
        if !item.as_text().is_some_and(|name| fields.contains_key(name)) {
            problems.push(problem(
                "schema_unsupported",
                Stage::Validate,
                &item.at,
                "The schema requires a field that `properties` does not describe.",
                "Describe the field under `properties`, or remove it from `required`.",
            ));
        }
    }
    if problems.is_empty() {
        Ok(MessageSchema {
            fields,
            closed: doc.get("additional_properties").and_then(Located::as_bool) == Some(false),
        })
    } else {
        Err(problems)
    }
}

/// The schema of one document under `schemas/`.
pub(crate) fn schema_schema() -> JsonSchema {
    JsonSchema::object()
        .required("schema", JsonSchema::name())
        .optional("surface", JsonSchema::surface())
        .required("type", JsonSchema::words(&["object"]))
        .optional("required", JsonSchema::list(JsonSchema::text()))
        .required(
            "properties",
            JsonSchema::map_of(
                JsonSchema::object()
                    .required("type", JsonSchema::words(JsonType::WORDS))
                    .optional("enum", JsonSchema::non_empty_list(JsonSchema::text())),
            ),
        )
        .optional("additional_properties", JsonSchema::bool())
}

/// The `schemas/` file kind.
pub(crate) struct SchemaKind {
    schema: JsonSchema,
    kinds: Vec<SurfaceKind>,
}

impl SchemaKind {
    pub(crate) fn new() -> Self {
        Self {
            schema: schema_schema(),
            kinds: vec![BROKER],
        }
    }
}

impl FileKind for SchemaKind {
    fn name(&self) -> FileKindName {
        MESSAGE_SCHEMA
    }

    fn surface_kinds(&self) -> &[SurfaceKind] {
        &self.kinds
    }

    fn folder(&self) -> &str {
        "schemas"
    }

    fn schema(&self) -> &JsonSchema {
        &self.schema
    }

    fn names(&self, doc: &Located<Doc>) -> Vec<Located<DefinedName>> {
        defined(doc, "schema")
    }

    fn expressions(&self, _doc: &Located<Doc>) -> Vec<ExprSite> {
        Vec::new()
    }

    fn compile(
        &self,
        docs: &[Located<Doc>],
        _cx: &KindContext<'_>,
    ) -> Result<CompiledSection, Diagnostics> {
        let mut entries = BTreeMap::new();
        let mut problems = Diagnostics::new();
        for doc in docs {
            match read_schema(doc) {
                Ok(schema) => {
                    entries.insert(doc.text("schema").unwrap_or_default().to_owned(), schema);
                }
                Err(found) => problems.extend(found),
            }
        }
        if problems.is_empty() {
            section(&entries)
        } else {
            Err(problems)
        }
    }
}
