//! The drafting rules (spec section 309, "Discovery and drafting"): from a
//! finding about a channel the broker lists, a channel file and one
//! operation that reads; from a schema the broker holds, a schema file.
//!
//! A rule writes what the broker stated and nothing else. What a message
//! on a channel is, who reads it, and whether the runtime owns it are
//! judgments, and are left as open questions or left out. No rule writes a
//! publish, a `take`, or a subscription's name.

use a2m_core::{Failure, FailureClass, FailureCode, FileKindName, Finding, FindingKind};
use a2m_files::{DraftContext, DraftDoc, DraftValue, DraftingRule, RelPath, UNTRUSTED_TEXT};

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
            // What a message is (`role`) is a judgment only a publish needs,
            // so no question of it is left here: the drafted reads compile,
            // and a publish written over the channel says it is missing.
        ];
        if found.json.get("schema").and_then(Json::as_bool) == Some(true) {
            body.push(("schema", DraftValue::text(&found.word)));
        }
        if let Some(field) = text("key_field") {
            body.push((
                "key",
                DraftValue::map(vec![("field", DraftValue::text(field))]),
            ));
        } else if shape == Shape::Retained {
            // A retained channel keeps one value per key, and the broker
            // does not say which field of a message the key is.
            body.push((
                "key",
                cx.open_question(&format!(
                    "Which field of a message on `{}` is its key? The broker keeps one value per key and does not say.",
                    found.channel
                )),
            ));
        }
        if text("look") == Some("browse") {
            body.push(("read_through", DraftValue::text("browse")));
        }
        if let Some(header) = text("dedupe_header") {
            // The broker states both: the header, and that it refuses a
            // repeat of its value; and, where it says, for how long.
            let mut idempotency = vec![
                ("header", DraftValue::text(header)),
                ("broker_dedupe", DraftValue::Bool(true)),
            ];
            if let Some(ms) = found
                .json
                .get("dedupe_window_ms")
                .and_then(Json::as_u64)
                .filter(|ms| *ms > 0 && u32::try_from(*ms).is_ok())
            {
                idempotency.push(("window", DraftValue::text(&format!("{ms}ms"))));
            }
            body.push(("idempotency", DraftValue::map(idempotency)));
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

/// The field type an output of a message's field is drafted with, from the
/// JSON type the broker's schema states: text the broker holds is
/// untrusted, and a number is read exactly, as a decimal. An array or an
/// object is no one value, and is left out.
fn output_type(json_type: &str) -> Option<&'static str> {
    match json_type {
        "string" => Some(UNTRUSTED_TEXT),
        "integer" => Some("integer"),
        "number" => Some("decimal"),
        "boolean" => Some("boolean"),
        _ => None,
    }
}

/// The outputs of a read of one key: each plain field of a message the
/// broker's schema states, but the key, which is the input. Where the
/// broker holds no schema the pack reads, which fields a message has is a
/// question: the read returns nothing until it is answered.
fn read_outputs(found: &Found, key: &str, cx: &DraftContext<'_>) -> DraftValue {
    let fields: Vec<(String, DraftValue)> = found
        .json
        .get("schema_fields")
        .and_then(Json::as_object)
        .map(|fields| {
            fields
                .iter()
                .filter(|(name, _)| name.as_str() != key && a2m_core::Verb::new(name).is_ok())
                .filter_map(|(name, json_type)| {
                    let field_type = output_type(json_type.as_str()?)?;
                    Some((
                        name.clone(),
                        DraftValue::map(vec![("type", DraftValue::text(field_type))]),
                    ))
                })
                .collect()
        })
        .unwrap_or_default();
    if fields.is_empty() {
        cx.open_question(&format!(
            "Which fields does a message on `{}` have, and of which types? The broker holds no schema for it that the pack reads.",
            found.channel
        ))
    } else {
        DraftValue::Map(fields)
    }
}

/// The names a look states of itself, which no field of a message takes
/// over.
pub(crate) const LOOK_FACTS: &[&str] = &["messages", "count", "more", "coverage", "next"];

/// The outputs of a look at a channel: each plain field of a message the
/// broker's schema states, or else each a look saw with one type, as a
/// list of its values in the order of the messages; then what the driver states of every look, whatever its
/// messages hold: how many messages the look returned, and whether the
/// channel held more than one look returns.
fn list_outputs(found: &Found) -> DraftValue {
    // The broker's schema where it holds one the pack reads; else the
    // fields a look saw, each with the one type every message gave it.
    let stated = found
        .json
        .get("schema_fields")
        .and_then(Json::as_object)
        .filter(|fields| !fields.is_empty());
    let mut outputs: Vec<(String, DraftValue)> = stated
        .or_else(|| found.json.get("sampled_fields").and_then(Json::as_object))
        .map(|fields| {
            fields
                .iter()
                .filter(|(name, _)| {
                    !LOOK_FACTS.contains(&name.as_str()) && a2m_core::Verb::new(name).is_ok()
                })
                .filter_map(|(name, json_type)| {
                    let field_type = output_type(json_type.as_str()?)?;
                    Some((
                        name.clone(),
                        DraftValue::map(vec![
                            ("type", DraftValue::text(field_type)),
                            ("list", DraftValue::Bool(true)),
                        ]),
                    ))
                })
                .collect()
        })
        .unwrap_or_default();
    outputs.push((
        "count".to_owned(),
        DraftValue::map(vec![("type", DraftValue::text("integer"))]),
    ));
    outputs.push((
        "more".to_owned(),
        DraftValue::map(vec![("type", DraftValue::text("boolean"))]),
    ));
    DraftValue::Map(outputs)
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
                    ("outputs", read_outputs(&found, key, cx)),
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
                        DraftValue::text(&format!(
                            "Reads the last value the broker holds for one key of the channel `{}`.",
                            found.channel
                        )),
                    ),
                ],
            )
        } else if text("look") == Some("browse") {
            (
                "list",
                vec![
                    ("effect", DraftValue::text("read")),
                    ("inputs", DraftValue::Map(Vec::new())),
                    ("outputs", list_outputs(&found)),
                    ("bindings", binding("list", Vec::new())),
                    (
                        "description",
                        DraftValue::text(&format!(
                            "Lists what the channel `{}` holds now, without taking anything: each field of its messages in their order, how many messages one look returns, and whether it holds more.",
                            found.channel
                        )),
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

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        reason = "a test: a helper that cannot do its work stops the test"
    )]

    use std::sync::Arc;

    use a2m_core::{
        Failure, FileKindName, Finding, FindingKind, ProducedBy, Source, Subject, Timestamp,
    };
    use a2m_files::{
        CompilerSet, DraftContext, DraftDoc, DraftValue, Drafter, DraftingRule, RelPath, TargetRoot,
    };

    use crate::channel::ChannelKind;
    use crate::names::{CHANNEL, Names, PACK};

    /// A rule that drafts what the pack's own rules never do: a channel
    /// file that says the runtime owns a queue, which is what lets a
    /// `take` remove messages.
    struct DraftsOwnership(FindingKind);

    impl DraftingRule for DraftsOwnership {
        fn finding_kind(&self) -> FindingKind {
            self.0.clone()
        }

        fn file_kind(&self) -> FileKindName {
            CHANNEL
        }

        fn draft(
            &self,
            _finding: &Finding,
            cx: &DraftContext<'_>,
        ) -> Result<Vec<DraftDoc>, Failure> {
            let channel = |file: &str, ownership: DraftValue| -> Result<DraftDoc, Failure> {
                Ok(DraftDoc {
                    path: RelPath::new(&format!("channels/{file}.yaml"))?,
                    body: DraftValue::map(vec![
                        ("channel", DraftValue::text("orders.work")),
                        ("address", DraftValue::text("orders.work")),
                        ("shape", DraftValue::text("queue")),
                        ("role", DraftValue::text("events")),
                        ("ownership", ownership),
                    ]),
                })
            };
            Ok(vec![
                channel("tries_a_take", DraftValue::text("runtime_owned"))?,
                channel("only_looks", DraftValue::text("shared"))?,
                // A question about who owns the queue is a question about
                // whether a take may remove from it: it counts as a write
                // (decision 0161, point 10).
                channel(
                    "asks_who_owns",
                    cx.open_question("Does the runtime own this queue?"),
                )?,
            ])
        }
    }

    #[test]
    fn a_drafting_rule_that_drafts_a_channel_the_runtime_owns_or_may_own_is_refused() {
        let names = Names::new().unwrap();
        let mut builder = CompilerSet::builder();
        builder
            .file_kind(PACK, Arc::new(ChannelKind::new(10_000)))
            .unwrap()
            .drafting_rule(
                PACK,
                Arc::new(DraftsOwnership(names.finding_channel.clone())),
            )
            .unwrap();
        let set = builder.build();
        let finding = Finding::about(
            Subject::Unknown {
                reason: a2m_core::UnknownReason::NotSupported,
            },
            names.finding_channel.clone(),
            "{}",
            Source::LiveSystem,
            ProducedBy {
                provider: PACK.to_owned(),
                version: "0.0.1".to_owned(),
                component: None,
            },
            Timestamp::from_unix_millis(0),
        )
        .build()
        .unwrap();
        let root =
            std::env::temp_dir().join(format!("a2m-pack-stream-draft-take-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let report = Drafter::new(&set, &TargetRoot::new(&root))
            .draft(std::slice::from_ref(&finding))
            .unwrap();
        let refused: Vec<&str> = report
            .refused
            .iter()
            .map(|(path, _)| path.as_str())
            .collect();
        let written: Vec<&str> = report.written.iter().map(RelPath::as_str).collect();
        let exists = root.join("channels").join("tries_a_take.yaml").exists()
            || root.join("channels").join("asks_who_owns.yaml").exists();
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(
            refused,
            ["channels/tries_a_take.yaml", "channels/asks_who_owns.yaml"]
        );
        assert_eq!(written, ["channels/only_looks.yaml"]);
        assert!(!exists);
    }
}
