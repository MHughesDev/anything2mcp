//! The drafting rules from what a Windows program publishes about itself to
//! reads (spec section 402, decision 0179; decision 0318):
//!
//! - from a `management_class` finding: one `read` for each property of a
//!   singleton class that the definition does not mark writable, of a form
//!   the read gives as text;
//! - from a `com_type_library` finding: one `read` for each property get of
//!   a dispatch interface that takes no argument, and gives a text, a
//!   flag, or a number.
//!
//! They never draft a write, a method, a function, or a property the
//! definition marks writable or declares can be put. A drafted read binds a
//! new key of the `desktop` binding (`management` or `com`), which the
//! lowering turns into one step that this version of the pack does not
//! carry out: the drafts compile and are reads, and they are not run until a
//! driver of those steps is built (decision 0318). A name the compiler
//! takes for a secret's is never drafted (decision 0302).

use a2m_core::{Failure, FileKindName, Finding, FindingKind, OperationName};
use a2m_files::{
    DraftContext, DraftDoc, DraftValue, DraftingRule, RelPath, UNTRUSTED_TEXT, left_out_sentence,
};
use serde_json::Value as Json;

use crate::draft::{is_name, named_as_secret, plain_words, surface};
use crate::propose::words;
use crate::screens::DESKTOP_SURFACE;
use crate::windows_kinds::FORMAT;

/// The kernel's file kind for operations.
const OPERATION: FileKindName = FileKindName::fixed("operation");

/// The CIM type words a read gives as text.
const CIM_TEXT: &[&str] = &[
    "string", "boolean", "sint8", "uint8", "sint16", "uint16", "sint32", "uint32", "sint64",
    "uint64", "datetime",
];
/// The variant types a read gives as text: whole numbers, a flag, a string.
const VT_TEXT: &[u64] = &[2, 3, 8, 11, 16, 17, 18, 19, 20, 21, 22, 23];

fn unreadable() -> Failure {
    a2m_core::codes::row::FINDING_INVALID.failure(
        "A finding is not in the form the desktop pack's discoverer writes.",
        "Run the discovery again with the version of the desktop pack that is enabled.",
    )
}

fn read_operation(
    operation: &str,
    key: &str,
    binding: Vec<(&str, DraftValue)>,
    output: &str,
    description: &str,
) -> DraftValue {
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
                (key, DraftValue::map(binding)),
            ])]),
        ),
        ("description", DraftValue::text(description)),
    ])
}

/// The target's one `desktop` surface that is this program and states, under
/// `key`, this value.
fn surface_stating(cx: &DraftContext<'_>, program: &str, key: &str, value: &str) -> Option<String> {
    let target = cx.existing()?.docs_of("target").next()?;
    let mut found = target
        .get("surfaces")?
        .entries()
        .iter()
        .filter(|entry| {
            entry.value.text("kind") == Some(DESKTOP_SURFACE.as_str())
                && entry.value.text(crate::ADDRESS_KEY) == Some(program)
                && entry.value.text(key) == Some(value)
        })
        .map(|entry| entry.key.value.clone());
    let first = found.next()?;
    found.next().is_none().then_some(first)
}

fn found_of(finding: &Finding, from: &str) -> Result<Option<Json>, Failure> {
    let found: Json = serde_json::from_str(finding.found()).map_err(|_| unreadable())?;
    if found.get("format").and_then(Json::as_str) != Some(FORMAT) {
        return Err(unreadable());
    }
    Ok((found.get("from").and_then(Json::as_str) == Some(from)).then_some(found))
}

fn list<'a>(value: &'a Json, key: &str) -> &'a [Json] {
    value
        .get(key)
        .and_then(Json::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
}

// ---- management ----

fn management(finding: &Finding, cx: &DraftContext<'_>) -> Result<Vec<DraftDoc>, Failure> {
    let Some(found) = found_of(finding, "management_schema")? else {
        return Ok(Vec::new());
    };
    let text = |key: &str| found.get(key).and_then(Json::as_str);
    let (Some(program), Some(class)) = (text("program"), text("class")) else {
        return Err(unreadable());
    };
    // A property of a class with many instances is a property of which
    // one? A schema does not say, and a singleton has one.
    if found.get("singleton").and_then(Json::as_bool) != Some(true) {
        return Ok(Vec::new());
    }
    let (Some(target), Some(surface)) = (
        cx.target(),
        surface_stating(cx, program, "management_class", class),
    ) else {
        return Ok(Vec::new());
    };
    let class_words = words(class);
    let mut out = Vec::new();
    let mut secrets: Vec<String> = Vec::new();
    for property in list(&found, "properties") {
        let (Some(name), Some(ty), Some(access)) = (
            property.get("name").and_then(Json::as_str),
            property.get("type").and_then(Json::as_str),
            property.get("access").and_then(Json::as_str),
        ) else {
            return Err(unreadable());
        };
        let array = property.get("array").and_then(Json::as_bool) == Some(true);
        if access != "read" || array || !CIM_TEXT.contains(&ty) || !plain_words(name) {
            continue;
        }
        let output = words(name);
        if named_as_secret(name, &output) {
            if is_name(&output) && !secrets.contains(&output) {
                secrets.push(output);
            }
            continue;
        }
        let file = format!("{class_words}_{output}_get");
        let operation = format!("{}_{file}", target.as_str());
        if !is_name(&output) || !is_name(&file) || OperationName::new(&operation).is_err() {
            continue;
        }
        let description = format!(
            "Reads the property `{name}` of the management class `{class}`, which the class's own definition does not mark writable. Drafted by rule from a discovery; its name is provisional."
        );
        out.push(DraftDoc {
            path: RelPath::new(&format!("operations/{file}.yaml"))?,
            body: read_operation(
                &operation,
                "management",
                vec![
                    ("surface", DraftValue::text(&surface)),
                    ("class", DraftValue::text(class)),
                    ("name", DraftValue::text(name)),
                ],
                &output,
                &description,
            ),
        });
    }
    append_left_out(&mut out, &secrets);
    Ok(out)
}

fn append_left_out(docs: &mut [DraftDoc], secrets: &[String]) {
    if secrets.is_empty() {
        return;
    }
    for doc in docs {
        if let DraftValue::Map(entries) = &mut doc.body {
            for (key, value) in entries.iter_mut() {
                if key == "description"
                    && let DraftValue::Text(text) = value
                {
                    text.push_str(&left_out_sentence(secrets));
                }
            }
        }
    }
}

/// From a `management_class` finding to one read for each property its
/// definition does not mark writable.
pub(crate) struct ManagementReadRule {
    pub(crate) kind: FindingKind,
}

impl DraftingRule for ManagementReadRule {
    fn finding_kind(&self) -> FindingKind {
        self.kind.clone()
    }

    fn file_kind(&self) -> FileKindName {
        OPERATION
    }

    fn draft(&self, finding: &Finding, cx: &DraftContext<'_>) -> Result<Vec<DraftDoc>, Failure> {
        management(finding, cx)
    }
}

// ---- COM ----

fn com(finding: &Finding, cx: &DraftContext<'_>) -> Result<Vec<DraftDoc>, Failure> {
    let Some(found) = found_of(finding, "type_library")? else {
        return Ok(Vec::new());
    };
    let Some(program) = found.get("program").and_then(Json::as_str) else {
        return Err(unreadable());
    };
    let (Some(target), Some(surface)) = (cx.target(), surface(cx, program)) else {
        return Ok(Vec::new());
    };
    // An interface named twice is not one a draft can bind by name.
    let interfaces = list(&found, "interfaces");
    let name_of = |interface: &Json| {
        interface
            .get("name")
            .and_then(Json::as_str)
            .map(str::to_owned)
    };
    let mut out = Vec::new();
    let mut secrets: Vec<String> = Vec::new();
    for interface in interfaces {
        let Some(interface_name) = name_of(interface) else {
            return Err(unreadable());
        };
        if interface.get("kind").and_then(Json::as_str) != Some("dispatch")
            || !plain_words(&interface_name)
            || interfaces
                .iter()
                .filter(|other| name_of(other).as_deref() == Some(interface_name.as_str()))
                .count()
                > 1
        {
            continue;
        }
        let from = out.len();
        for member in list(interface, "members") {
            let (Some(name), Some(invoke)) = (
                member.get("name").and_then(Json::as_str),
                member.get("invoke").and_then(Json::as_str),
            ) else {
                return Err(unreadable());
            };
            let value_type = member
                .get("returns")
                .and_then(|returns| returns.get("vt"))
                .and_then(Json::as_u64);
            // A property get that takes nothing and gives a simple value.
            // A member the library also lets a caller put is still a read to
            // draft, and is the library's own word for it.
            let takes_nothing = list(member, "parameters")
                .iter()
                .all(|parameter| parameter.get("retval").and_then(Json::as_bool) == Some(true));
            if invoke != "property_get"
                || !takes_nothing
                || !value_type.is_some_and(|vt| VT_TEXT.contains(&vt))
                || !plain_words(name)
            {
                continue;
            }
            let output = words(name);
            if named_as_secret(name, &output) {
                if is_name(&output) && !secrets.contains(&output) {
                    secrets.push(output);
                }
                continue;
            }
            let module = words(&interface_name);
            let file = format!("{module}_{output}_get");
            let operation = format!("{}_{file}", target.as_str());
            if !is_name(&output)
                || !is_name(&file)
                || OperationName::new(&operation).is_err()
                || out
                    .iter()
                    .any(|doc: &DraftDoc| doc.path.as_str() == format!("operations/{file}.yaml"))
            {
                continue;
            }
            let description = format!(
                "Reads the property `{name}` of the interface `{interface_name}`, which its type library declares as a property get. Drafted by rule from a type library; its name is provisional."
            );
            out.push(DraftDoc {
                path: RelPath::new(&format!("operations/{file}.yaml"))?,
                body: read_operation(
                    &operation,
                    "com",
                    vec![
                        ("surface", DraftValue::text(&surface)),
                        ("interface", DraftValue::text(&interface_name)),
                        ("name", DraftValue::text(name)),
                    ],
                    &output,
                    &description,
                ),
            });
        }
        append_left_out(&mut out[from..], &secrets);
        secrets.clear();
    }
    Ok(out)
}

/// From a `com_type_library` finding to one read for each property get of a
/// dispatch interface.
pub(crate) struct ComReadRule {
    pub(crate) kind: FindingKind,
}

impl DraftingRule for ComReadRule {
    fn finding_kind(&self) -> FindingKind {
        self.kind.clone()
    }

    fn file_kind(&self) -> FileKindName {
        OPERATION
    }

    fn draft(&self, finding: &Finding, cx: &DraftContext<'_>) -> Result<Vec<DraftDoc>, Failure> {
        com(finding, cx)
    }
}
