//! The drafting rules (spec section 309, "Discovery and drafting"): from a
//! finding about a channel the broker lists, a channel file and one
//! operation that reads; from a schema the broker holds, a schema file.
//!
//! A rule writes what the broker stated and nothing else. What a message
//! on a channel is, who reads it, and whether the runtime owns it are
//! judgments, and are left as open questions or left out. No rule writes a
//! publish, a `take`, or a subscription's name.

use a2m_core::{Failure, FailureClass, FailureCode, FileKindName, Finding, FindingKind};
use a2m_files::{DraftContext, DraftDoc, DraftValue, DraftingRule, RelPath};

use crate::channel::{MessageSchema, Shape};
use crate::json::Json;
use crate::names::{CHANNEL, MESSAGE_SCHEMA, Names, OPERATION, PACK};

fn unreadable(kind: &FindingKind) -> Failure {
    Failure::new(
        FailureClass::Input,
        FailureCode::FINDING_INVALID,
        format!(
            "A `{}` finding does not hold what the stream pack's discoverer writes.",
            kind.as_str()
        ),
        "Run the discovery again; a finding of this kind is made by the stream pack.",
    )
}

/// What a `channel` or `message_schema` finding holds, and the names a
/// draft is given for it.
struct Found {
    json: Json,
    address: String,
    /// The channel's name in the target: `module.object`, from the last two
    /// words of the broker's own name for it.
    channel: String,
    /// The same as one word, for a file name and a schema's name.
    word: String,
}

/// Reads a finding's `found`, and derives the draft's names mechanically
/// from the channel's address: its words in lowercase, the last two kept.
fn found(finding: &Finding) -> Result<Found, Failure> {
    let json = Json::parse(finding.found()).map_err(|_| unreadable(finding.kind()))?;
    let address = json
        .get("address")
        .and_then(Json::as_str)
        .ok_or_else(|| unreadable(finding.kind()))?
        .to_owned();
    let words: Vec<String> = address
        .split(|c: char| !c.is_ascii_alphanumeric())
        .map(str::to_ascii_lowercase)
        .filter(|word| word.starts_with(|c: char| c.is_ascii_lowercase()))
        .collect();
    let (module, object) = match words.as_slice() {
        [] => return Err(unreadable(finding.kind())),
        [only] => ("channels".to_owned(), only.clone()),
        [.., module, object] => (module.clone(), object.clone()),
    };
    Ok(Found {
        json,
        address,
        channel: format!("{module}.{object}"),
        word: format!("{module}_{object}"),
    })
}

fn path(folder: &str, word: &str) -> Result<RelPath, Failure> {
    RelPath::new(&format!("{folder}/{word}.yaml"))
}

/// From a channel the broker lists: a channel file.
pub(crate) struct ChannelDraft {
    names: Names,
}

impl ChannelDraft {
    pub(crate) fn new(names: Names) -> Self {
        Self { names }
    }
}

impl DraftingRule for ChannelDraft {
    fn finding_kind(&self) -> FindingKind {
        self.names.finding_channel.clone()
    }

    fn file_kind(&self) -> FileKindName {
        CHANNEL
    }

    fn draft(&self, finding: &Finding, cx: &DraftContext<'_>) -> Result<Vec<DraftDoc>, Failure> {
        let found = found(finding)?;
        let text = |key: &str| found.json.get(key).and_then(Json::as_str);
        let shape = text("shape")
            .and_then(Shape::from_word)
            .ok_or_else(|| unreadable(finding.kind()))?;
        let mut body = vec![
            ("channel", DraftValue::text(&found.channel)),
            ("address", DraftValue::text(&found.address)),
            ("shape", DraftValue::text(shape.as_str())),
            (
                "role",
                cx.open_question(&format!(
                    "Is each message on `{}` the latest state for its key (`state`), or a thing that happened (`events`)?",
                    found.channel
                )),
            ),
        ];
        if found.json.get("schema").and_then(Json::as_bool) == Some(true) {
            body.push(("schema", DraftValue::text(&found.word)));
        }
        if let Some(field) = text("key_field") {
            body.push((
                "key",
                DraftValue::map(vec![("field", DraftValue::text(field))]),
            ));
        }
        if text("look") == Some("browse") {
            body.push(("read_through", DraftValue::text("browse")));
        }
        if let Some(header) = text("dedupe_header") {
            // The broker states both: the header, and that it refuses a
            // repeat of its value.
            body.push((
                "idempotency",
                DraftValue::map(vec![
                    ("header", DraftValue::text(header)),
                    ("broker_dedupe", DraftValue::Bool(true)),
                ]),
            ));
        }
        Ok(vec![DraftDoc {
            path: path("channels", &found.word)?,
            body: DraftValue::map(body),
        }])
    }
}

/// From a schema the broker holds for a channel: a schema file.
pub(crate) struct SchemaDraft {
    names: Names,
}

impl SchemaDraft {
    pub(crate) fn new(names: Names) -> Self {
        Self { names }
    }
}

impl DraftingRule for SchemaDraft {
    fn finding_kind(&self) -> FindingKind {
        self.names.finding_schema.clone()
    }

    fn file_kind(&self) -> FileKindName {
        MESSAGE_SCHEMA
    }

    fn draft(&self, finding: &Finding, cx: &DraftContext<'_>) -> Result<Vec<DraftDoc>, Failure> {
        let found = found(finding)?;
        let registered = found
            .json
            .get("schema")
            .ok_or_else(|| unreadable(finding.kind()))?;
        let mut body = vec![
            ("schema", DraftValue::text(&found.word)),
            ("type", DraftValue::text("object")),
        ];
        match MessageSchema::from_registered(registered) {
            Some(schema) => {
                let required: Vec<DraftValue> = schema
                    .fields
                    .iter()
                    .filter(|(_, rule)| rule.required)
                    .map(|(name, _)| DraftValue::text(name))
                    .collect();
                if !required.is_empty() {
                    body.push(("required", DraftValue::List(required)));
                }
                let properties: Vec<(String, DraftValue)> = schema
                    .fields
                    .iter()
                    .map(|(name, rule)| {
                        let mut property = vec![("type", DraftValue::text(rule.ty.as_str()))];
                        if !rule.allowed.is_empty() {
                            property.push((
                                "enum",
                                DraftValue::List(
                                    rule.allowed
                                        .iter()
                                        .map(|allowed| DraftValue::text(allowed))
                                        .collect(),
                                ),
                            ));
                        }
                        (name.clone(), DraftValue::map(property))
                    })
                    .collect();
                body.push(("properties", DraftValue::Map(properties)));
                if schema.closed {
                    body.push(("additional_properties", DraftValue::Bool(false)));
                }
            }
            None => body.push((
                "properties",
                cx.open_question(&format!(
                    "The schema the broker holds for `{}` uses parts of JSON Schema this pack does not check. Which fields does a message have, and of which types?",
                    found.channel
                )),
            )),
        }
        Ok(vec![DraftDoc {
            path: path("schemas", &found.word)?,
            body: DraftValue::map(body),
        }])
    }
}

/// From a channel the broker lists: one operation that reads it. `read` of
/// a retained key where the broker states the key; else `list` where the
/// broker let a look through; else nothing.
pub(crate) struct ReadDraft {
    names: Names,
}

impl ReadDraft {
    pub(crate) fn new(names: Names) -> Self {
        Self { names }
    }
}

impl DraftingRule for ReadDraft {
    fn finding_kind(&self) -> FindingKind {
        self.names.finding_channel.clone()
    }

    fn file_kind(&self) -> FileKindName {
        OPERATION
    }

    fn draft(&self, finding: &Finding, cx: &DraftContext<'_>) -> Result<Vec<DraftDoc>, Failure> {
        let found = found(finding)?;
        let Some(target) = cx.target() else {
            // A tool is named after its target; with none there is no name.
            return Ok(Vec::new());
        };
        let text = |key: &str| found.json.get(key).and_then(Json::as_str);
        let key = text("key_field").filter(|field| {
            a2m_core::Verb::new(field).is_ok() && text("shape") == Some(Shape::Retained.as_str())
        });
        let binding = |verb: &str, more: Vec<(&str, DraftValue)>| {
            let mut binding = vec![
                ("mode", DraftValue::text(PACK)),
                ("verb", DraftValue::text(verb)),
                ("channel", DraftValue::text(&found.channel)),
            ];
            binding.extend(more);
            DraftValue::List(vec![DraftValue::map(binding)])
        };
        let (verb, body) = if let Some(key) = key {
            (
                "get",
                vec![
                    ("effect", DraftValue::text("read")),
                    ("key", DraftValue::text(key)),
                    (
                        "inputs",
                        DraftValue::map(vec![(
                            key,
                            DraftValue::map(vec![("type", DraftValue::text("text"))]),
                        )]),
                    ),
                    ("outputs", DraftValue::Map(Vec::new())),
                    (
                        "bindings",
                        binding(
                            "read",
                            vec![(
                                "key",
                                DraftValue::map(vec![(
                                    key,
                                    DraftValue::text(&format!("input.{key}")),
                                )]),
                            )],
                        ),
                    ),
                    (
                        "description",
                        DraftValue::text(
                            "Reads the last value the broker holds for one key of the channel.",
                        ),
                    ),
                ],
            )
        } else if text("look") == Some("browse") {
            (
                "list",
                vec![
                    ("effect", DraftValue::text("read")),
                    ("inputs", DraftValue::Map(Vec::new())),
                    ("outputs", DraftValue::Map(Vec::new())),
                    ("bindings", binding("list", Vec::new())),
                    (
                        "description",
                        DraftValue::text(
                            "Lists what the channel holds now, without taking anything.",
                        ),
                    ),
                ],
            )
        } else {
            return Ok(Vec::new());
        };
        let name = format!("{}_{}_{verb}", target.as_str(), found.word);
        if a2m_core::OperationName::new(&name).is_err() {
            return Ok(Vec::new());
        }
        let mut doc = vec![("operation", DraftValue::text(&name))];
        doc.extend(body);
        Ok(vec![DraftDoc {
            path: path("operations", &format!("{}_{verb}", found.word))?,
            body: DraftValue::map(doc),
        }])
    }
}
