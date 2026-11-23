//! The drafting rule from a `dbus_service` finding to reads of a service's
//! properties (spec section 402, decision 0179; decision 0316).
//!
//! From the finding of a running service the rule drafts one `read`
//! operation for each property whose document states `access="read"`,
//! whose type is text, a flag, a whole number, or an object path, and whose
//! interface only one object of the service answers. It never drafts:
//!
//! - a write, or a call of any method: the verb is `read`, the operation's
//!   effect is `read`, and the binding names a property and nothing that
//!   acts;
//! - a property that states `readwrite` or `write`: the document says a
//!   caller may set it, and a draft that reads it is left to a person;
//! - anything from a file's finding, which names no object path, or from a
//!   service that was not running;
//! - a name the compiler takes for a secret's (decision 0302), or words
//!   that are not plain.
//!
//! The surface the draft binds is the target's one `desktop` surface that
//! names this program and this service; a target that states no `service`
//! on its surface gets no draft.

use a2m_core::{Failure, FileKindName, Finding, FindingKind, OperationName};
use a2m_files::{
    DraftContext, DraftDoc, DraftValue, DraftingRule, RelPath, UNTRUSTED_TEXT, left_out_sentence,
};
use serde_json::Value as Json;

use crate::draft::{is_name, named_as_secret, plain_words, snake};
use crate::screens::DESKTOP_SURFACE;
use crate::service::FORMAT;

/// The kernel's file kind for operations.
const OPERATION: FileKindName = FileKindName::fixed("operation");

/// The signatures whose value the `property` step gives as text.
const TEXT_SIGNATURES: &[&str] = &["s", "o", "b", "y", "n", "q", "i", "u", "x", "t"];

fn unreadable() -> Failure {
    a2m_core::codes::row::FINDING_INVALID.failure(
        "A `dbus_service` finding is not in the form the desktop pack's discoverer writes.",
        "Run the discovery again with the version of the desktop pack that is enabled.",
    )
}

/// `CamelCase` and `snake_case` words as the snake case target files use.
fn words(text: &str) -> String {
    let mut spaced = String::new();
    let mut last_lower = false;
    for c in text.chars() {
        if c.is_ascii_uppercase() && last_lower {
            spaced.push(' ');
        }
        last_lower = c.is_ascii_lowercase() || c.is_ascii_digit();
        spaced.push(c);
    }
    snake(&spaced)
}

/// The target's one `desktop` surface that is this program and states this
/// service.
fn surface_of(cx: &DraftContext<'_>, program: &str, service: &str) -> Option<String> {
    let target = cx.existing()?.docs_of("target").next()?;
    let mut found = target
        .get("surfaces")?
        .entries()
        .iter()
        .filter(|entry| {
            entry.value.text("kind") == Some(DESKTOP_SURFACE.as_str())
                && entry.value.text(crate::ADDRESS_KEY) == Some(program)
                && entry.value.text("service") == Some(service)
        })
        .map(|entry| entry.key.value.clone());
    let first = found.next()?;
    found.next().is_none().then_some(first)
}

struct Property<'a> {
    path: &'a str,
    interface: &'a str,
    name: &'a str,
}

fn drafts(finding: &Finding, cx: &DraftContext<'_>) -> Result<Vec<DraftDoc>, Failure> {
    let found: Json = serde_json::from_str(finding.found()).map_err(|_| unreadable())?;
    if found.get("format").and_then(Json::as_str) != Some(FORMAT) {
        return Err(unreadable());
    }
    // Only what the running service said of itself: a file names no path.
    if found.get("from").and_then(Json::as_str) != Some("live")
        || found.get("running").and_then(Json::as_bool) != Some(true)
        || found.get("cut").is_some_and(|cut| !cut.is_null())
    {
        return Ok(Vec::new());
    }
    let text = |key: &str| found.get(key).and_then(Json::as_str);
    let (Some(program), Some(service)) = (text("program"), text("service")) else {
        return Err(unreadable());
    };
    let (Some(target), Some(surface)) = (cx.target(), surface_of(cx, program, service)) else {
        return Ok(Vec::new());
    };
    let objects = found
        .get("objects")
        .and_then(Json::as_array)
        .ok_or_else(unreadable)?;
    // How many objects answer each interface: a draft binds one object, so
    // an interface that many objects answer is not drafted.
    let mut answered: Vec<(&str, usize)> = Vec::new();
    for object in objects {
        for interface in object
            .get("interfaces")
            .and_then(Json::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default()
        {
            let name = interface
                .get("name")
                .and_then(Json::as_str)
                .ok_or_else(unreadable)?;
            match answered.iter_mut().find(|(known, _)| *known == name) {
                Some((_, count)) => *count += 1,
                None => answered.push((name, 1)),
            }
        }
    }
    let mut out = Vec::new();
    let mut named: Vec<String> = Vec::new();
    for object in objects {
        let path = object
            .get("path")
            .and_then(Json::as_str)
            .ok_or_else(unreadable)?;
        for interface in object
            .get("interfaces")
            .and_then(Json::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default()
        {
            let interface_name = interface
                .get("name")
                .and_then(Json::as_str)
                .ok_or_else(unreadable)?;
            if answered
                .iter()
                .any(|(known, count)| *known == interface_name && *count > 1)
                || !plain_words(interface_name)
            {
                continue;
            }
            let from = out.len();
            let mut secrets: Vec<String> = Vec::new();
            for property in interface
                .get("properties")
                .and_then(Json::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default()
            {
                let (Some(name), Some(ty), Some(access)) = (
                    property.get("name").and_then(Json::as_str),
                    property.get("type").and_then(Json::as_str),
                    property.get("access").and_then(Json::as_str),
                ) else {
                    return Err(unreadable());
                };
                // Only what the document states can only be read, of a type
                // the step gives as text.
                if access != "read" || !TEXT_SIGNATURES.contains(&ty) || !plain_words(name) {
                    continue;
                }
                let output = words(name);
                if named_as_secret(name, &output) {
                    if is_name(&output) && !secrets.contains(&output) {
                        secrets.push(output);
                    }
                    continue;
                }
                let module = interface_name
                    .rsplit('.')
                    .next()
                    .map(words)
                    .unwrap_or_default();
                let operation = format!("{}_{module}_{output}_get", target.as_str());
                let file = format!("{module}_{output}_get");
                if !is_name(&output)
                    || !is_name(&file)
                    || OperationName::new(&operation).is_err()
                    || named.contains(&operation)
                {
                    continue;
                }
                named.push(operation.clone());
                let at = Property {
                    path,
                    interface: interface_name,
                    name,
                };
                out.push(DraftDoc {
                    path: RelPath::new(&format!("operations/{file}.yaml"))?,
                    body: read_operation(&operation, &surface, &at, &output),
                });
            }
            // What was left out for its name is said in the description of
            // each read drafted from the same interface.
            if !secrets.is_empty() {
                for doc in &mut out[from..] {
                    if let DraftValue::Map(entries) = &mut doc.body {
                        for (key, value) in entries.iter_mut() {
                            if key == "description"
                                && let DraftValue::Text(text) = value
                            {
                                text.push_str(&left_out_sentence(&secrets));
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(out)
}

fn read_operation(operation: &str, surface: &str, at: &Property<'_>, output: &str) -> DraftValue {
    DraftValue::map(vec![
        ("operation", DraftValue::text(operation)),
        ("effect", DraftValue::text("read")),
        ("inputs", DraftValue::Map(Vec::new())),
        (
            "outputs",
            DraftValue::Map(vec![(
                output.to_owned(),
                DraftValue::map(vec![("type", DraftValue::text(UNTRUSTED_TEXT))]),
            )]),
        ),
        (
            "bindings",
            DraftValue::List(vec![DraftValue::map(vec![
                ("mode", DraftValue::text(crate::DESKTOP_MODE)),
                ("verb", DraftValue::text("read")),
                (
                    "property",
                    DraftValue::map(vec![
                        ("surface", DraftValue::text(surface)),
                        ("path", DraftValue::text(at.path)),
                        ("interface", DraftValue::text(at.interface)),
                        ("name", DraftValue::text(at.name)),
                    ]),
                ),
            ])]),
        ),
        (
            "description",
            DraftValue::text(&format!(
                "Reads the property `{}` of the interface `{}` of one object of the application's service on the session bus, which the service's own introspection states can only be read. Drafted by rule from a discovery; its name is provisional.",
                at.name, at.interface
            )),
        ),
    ])
}

/// From a `dbus_service` finding to one read for each property its document
/// states can only be read.
pub(crate) struct PropertyReadRule {
    pub(crate) kind: FindingKind,
}

impl DraftingRule for PropertyReadRule {
    fn finding_kind(&self) -> FindingKind {
        self.kind.clone()
    }

    fn file_kind(&self) -> FileKindName {
        OPERATION
    }

    fn draft(&self, finding: &Finding, cx: &DraftContext<'_>) -> Result<Vec<DraftDoc>, Failure> {
        drafts(finding, cx)
    }
}
