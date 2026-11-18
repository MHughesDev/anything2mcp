//! Reading an XML answer into the record its endpoint declares.
//!
//! The reader streams: it keeps the names of the open elements and the
//! values it was asked for, and never builds a tree of the document. It
//! reads no document type: an answer that declares one is refused before
//! any value is read, so no entity it defines is expanded and nothing it
//! names is fetched. Of references, only characters and the five names the
//! format itself defines are read; any other is refused. Depth and the
//! number of elements are bounded by the endpoint, and the size by the
//! answer's ceiling, which the body was read under.

use quick_xml::NsReader;
use quick_xml::XmlVersion;
use quick_xml::escape::resolve_predefined_entity;
use quick_xml::events::{BytesStart, Event};
use quick_xml::name::ResolveResult;

use crate::forms::{Unread, XmlField, XmlForm, XmlName, XmlPath, XmlTake, typed};
use crate::json::Json;
use crate::shape::Kind;

/// The name of an element or an attribute, as the document resolves it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Resolved {
    namespace: Option<String>,
    local: String,
}

impl Resolved {
    fn is(&self, name: &XmlName) -> bool {
        self.namespace == name.namespace && self.local == name.local
    }
}

fn same(stack: &[Resolved], path: &[XmlName]) -> bool {
    stack.len() == path.len() && stack.iter().zip(path).all(|(open, name)| open.is(name))
}

/// One value found, and how many places held it.
#[derive(Debug, Default)]
struct Slot {
    value: Option<Json>,
    found: u32,
}

/// Where the text being gathered goes.
#[derive(Debug, Clone, Copy)]
enum Target {
    /// A top-level field, by its position.
    Top(usize),
    /// A field of the open record of a list, by the list's and the field's
    /// positions.
    Inner(usize, usize),
    /// A new entry of a list of texts, by the list's position.
    Entry(usize),
}

/// Text being gathered for an element that is open.
struct Gathering {
    depth: usize,
    targets: Vec<(Target, Kind)>,
    text: String,
}

/// A record of a list, open while its element is.
struct OpenRecord {
    depth: usize,
    slots: Vec<Slot>,
}

struct State<'f> {
    form: &'f XmlForm,
    top: Vec<Slot>,
    lists: Vec<Vec<Json>>,
    open: Vec<Option<OpenRecord>>,
    gathering: Option<Gathering>,
}

fn shape(why: impl Into<String>) -> Unread {
    Unread::Shape(why.into())
}

fn twice(name: &str) -> Unread {
    shape(format!(
        "the path of `{name}` matches more than one place, and the endpoint declares one"
    ))
}

impl State<'_> {
    fn fill(&mut self, target: Target, kind: Kind, text: &str) -> Result<(), Unread> {
        let form: &XmlForm = self.form;
        let fields = &form.fields;
        let (name, slot) = match target {
            Target::Top(at) => (&fields[at].name, &mut self.top[at]),
            Target::Inner(list, at) => {
                let XmlTake::Each { fields: inner, .. } = &fields[list].take else {
                    return Err(shape("a field of a list is declared as one value"));
                };
                let Some(record) = self.open[list].as_mut() else {
                    return Err(shape("a field of a list was found outside its element"));
                };
                (&inner[at].name, &mut record.slots[at])
            }
            Target::Entry(list) => {
                let value = typed(text, kind).ok_or_else(|| {
                    shape(format!(
                        "an entry of `{}` is not a {}",
                        fields[list].name,
                        kind.word()
                    ))
                })?;
                self.lists[list].push(value);
                return Ok(());
            }
        };
        slot.found += 1;
        if slot.found > 1 {
            return Err(twice(name));
        }
        slot.value = Some(
            typed(text, kind).ok_or_else(|| shape(format!("`{name}` is not a {}", kind.word())))?,
        );
        Ok(())
    }
}

/// The value of one attribute of an element, when it has it.
fn attribute(
    reader: &NsReader<&[u8]>,
    element: &BytesStart<'_>,
    wanted: &XmlName,
    version: XmlVersion,
) -> Result<Option<String>, Unread> {
    for attribute in element.attributes() {
        let attribute =
            attribute.map_err(|_| shape("an attribute is not written as the format defines"))?;
        let (namespace, local) = reader.resolver().resolve_attribute(attribute.key);
        let namespace = match namespace {
            ResolveResult::Unbound => None,
            ResolveResult::Bound(uri) => Some(uri.into_inner().to_owned()),
            ResolveResult::Unknown(_) => {
                return Err(shape(
                    "an attribute uses a prefix the document does not declare",
                ));
            }
        };
        if namespace == wanted.namespace && local.into_inner() == wanted.local {
            let value = attribute
                .normalized_value_with(version, 1, resolve_predefined_entity)
                .map_err(|_| {
                    shape("an attribute names a character or an entity the format does not define")
                })?;
            return Ok(Some(value.into_owned()));
        }
    }
    Ok(None)
}

/// Whether text is only the white space the format defines.
fn is_space(text: &str) -> bool {
    text.chars().all(|c| matches!(c, ' ' | '\t' | '\n' | '\r'))
}

/// Reads an XML answer into its record.
pub(crate) fn read(body: &[u8], form: &XmlForm) -> Result<Json, Unread> {
    let body = body.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(body);
    let text = std::str::from_utf8(body).map_err(|error| {
        shape(format!(
            "the body is not UTF-8 at byte {}",
            error.valid_up_to()
        ))
    })?;
    let mut reader = NsReader::from_str(text);
    let mut version = XmlVersion::Implicit1_0;
    let mut state = State {
        form,
        top: form.fields.iter().map(|_| Slot::default()).collect(),
        lists: form.fields.iter().map(|_| Vec::new()).collect(),
        open: form.fields.iter().map(|_| None).collect(),
        gathering: None,
    };
    let mut stack: Vec<Resolved> = Vec::new();
    let mut elements: u64 = 0;
    let mut root_closed = false;

    loop {
        let event = reader.read_event().map_err(|_| {
            shape(format!(
                "the body is not well-formed XML near byte {}",
                reader.error_position()
            ))
        })?;
        let (start, empty) = match event {
            Event::Eof => break,
            Event::DocType(_) => return Err(Unread::DocumentType),
            Event::Decl(decl) => {
                if let Some(encoding) = decl.encoding() {
                    let encoding =
                        encoding.map_err(|_| shape("the XML declaration is not well formed"))?;
                    if !matches!(encoding.to_ascii_lowercase().as_str(), "utf-8" | "us-ascii") {
                        return Err(shape("the document declares an encoding other than UTF-8"));
                    }
                }
                version = decl
                    .xml_version()
                    .map_err(|_| shape("the XML declaration is not well formed"))?;
                continue;
            }
            Event::Comment(_) | Event::PI(_) => continue,
            Event::Text(part) => {
                let part = part.xml_content(version);
                match &mut state.gathering {
                    Some(gathering) if gathering.depth == stack.len() => {
                        gathering.text.push_str(&part);
                    }
                    _ if stack.is_empty() && !is_space(&part) => {
                        return Err(shape("the body holds text outside its root element"));
                    }
                    _ => {}
                }
                continue;
            }
            Event::CData(part) => {
                if let Some(gathering) = &mut state.gathering
                    && gathering.depth == stack.len()
                {
                    gathering.text.push_str(&part.xml_content(version));
                }
                continue;
            }
            Event::GeneralRef(reference) => {
                let resolved = match reference.resolve_char_ref() {
                    Ok(Some(c)) => c.to_string(),
                    Ok(None) => resolve_predefined_entity(&reference.xml_content(version))
                        .ok_or_else(|| {
                            shape("the body names an entity the format does not define")
                        })?
                        .to_owned(),
                    Err(_) => return Err(shape("the body names a character that is not one")),
                };
                if let Some(gathering) = &mut state.gathering
                    && gathering.depth == stack.len()
                {
                    gathering.text.push_str(&resolved);
                }
                continue;
            }
            Event::End(_) => {
                close(&mut state, &mut stack, &mut root_closed)?;
                continue;
            }
            Event::Start(start) => (start, false),
            Event::Empty(start) => (start, true),
        };

        // An element opens.
        if root_closed {
            return Err(shape("the body holds more than one root element"));
        }
        if state
            .gathering
            .as_ref()
            .is_some_and(|gathering| gathering.depth <= stack.len())
        {
            return Err(shape(
                "an element is inside an element whose text the endpoint declares",
            ));
        }
        let (namespace, local) = reader.resolver().resolve_element(start.name());
        let namespace = match namespace {
            ResolveResult::Unbound => None,
            ResolveResult::Bound(uri) => Some(uri.into_inner().to_owned()),
            ResolveResult::Unknown(_) => {
                return Err(shape(
                    "an element uses a prefix the document does not declare",
                ));
            }
        };
        stack.push(Resolved {
            namespace,
            local: local.into_inner().to_owned(),
        });
        elements += 1;
        if stack.len() > usize::try_from(form.max_depth).unwrap_or(usize::MAX) {
            return Err(Unread::TooLarge(format!(
                "the document is nested deeper than {} elements",
                form.max_depth
            )));
        }
        if elements > form.max_elements {
            return Err(Unread::TooLarge(format!(
                "the document holds more than {} elements",
                form.max_elements
            )));
        }
        open(&mut state, &reader, &start, &stack, version)?;
        if empty {
            close(&mut state, &mut stack, &mut root_closed)?;
        }
    }
    if !root_closed {
        return Err(shape(if stack.is_empty() {
            "the body holds no element"
        } else {
            "the body ends inside an element"
        }));
    }

    let mut record = Vec::new();
    for (at, field) in form.fields.iter().enumerate() {
        let value = match &field.take {
            XmlTake::Each { .. } => Json::List(std::mem::take(&mut state.lists[at])),
            XmlTake::One(_) => match state.top[at].value.take() {
                Some(value) => value,
                None if field.required => return Err(missing(&field.name)),
                None => Json::Null,
            },
        };
        record.push((field.name.clone(), value));
    }
    Ok(Json::Object(record))
}

fn missing(name: &str) -> Unread {
    shape(format!(
        "the path of `{name}` matches nothing, and the endpoint declares it required"
    ))
}

/// One field whose path ends at the element that opens: its attribute is
/// read now, and its text is gathered until the element closes.
fn found(
    state: &mut State<'_>,
    at: (&NsReader<&[u8]>, &BytesStart<'_>, XmlVersion),
    target: Target,
    field: &XmlField,
    path: &XmlPath,
) -> Result<Option<(Target, Kind)>, Unread> {
    let (reader, start, version) = at;
    match &path.attribute {
        Some(wanted) => {
            if let Some(value) = attribute(reader, start, wanted, version)? {
                state.fill(target, field.kind, &value)?;
            }
            Ok(None)
        }
        None => Ok(Some((target, field.kind))),
    }
}

/// What an element that opens is to the declared fields.
fn open(
    state: &mut State<'_>,
    reader: &NsReader<&[u8]>,
    start: &BytesStart<'_>,
    stack: &[Resolved],
    version: XmlVersion,
) -> Result<(), Unread> {
    let form: &XmlForm = state.form;
    let here = (reader, start, version);
    let mut gather: Vec<(Target, Kind)> = Vec::new();
    for (at, field) in form.fields.iter().enumerate() {
        match &field.take {
            XmlTake::One(path) => {
                if same(stack, &path.elements) {
                    gather.extend(found(state, here, Target::Top(at), field, path)?);
                }
            }
            XmlTake::Each {
                elements,
                fields: inner,
            } => {
                if same(stack, elements) {
                    if inner.is_empty() {
                        gather.push((Target::Entry(at), field.kind));
                        continue;
                    }
                    state.open[at] = Some(OpenRecord {
                        depth: stack.len(),
                        slots: inner.iter().map(|_| Slot::default()).collect(),
                    });
                }
                let Some(depth) = state.open[at].as_ref().map(|record| record.depth) else {
                    continue;
                };
                let relative = stack.get(depth..).unwrap_or_default();
                for (inner_at, inner_field) in inner.iter().enumerate() {
                    if let XmlTake::One(path) = &inner_field.take
                        && same(relative, &path.elements)
                    {
                        let target = Target::Inner(at, inner_at);
                        gather.extend(found(state, here, target, inner_field, path)?);
                    }
                }
            }
        }
    }
    if !gather.is_empty() {
        state.gathering = Some(Gathering {
            depth: stack.len(),
            targets: gather,
            text: String::new(),
        });
    }
    Ok(())
}

/// What an element that closes completes.
fn close(
    state: &mut State<'_>,
    stack: &mut Vec<Resolved>,
    root_closed: &mut bool,
) -> Result<(), Unread> {
    let depth = stack.len();
    if let Some(gathering) = state
        .gathering
        .take_if(|gathering| gathering.depth == depth)
    {
        let text = gathering
            .text
            .trim_matches(|c| matches!(c, ' ' | '\t' | '\n' | '\r'));
        for (target, kind) in gathering.targets {
            state.fill(target, kind, text)?;
        }
    }
    let form: &XmlForm = state.form;
    for (at, field) in form.fields.iter().enumerate() {
        let XmlTake::Each { fields: inner, .. } = &field.take else {
            continue;
        };
        let Some(record) = state.open[at].take_if(|record| record.depth == depth) else {
            continue;
        };
        let mut values = Vec::new();
        for (slot, inner_field) in record.slots.into_iter().zip(inner) {
            let value = match slot.value {
                Some(value) => value,
                None if inner_field.required => return Err(missing(&inner_field.name)),
                None => Json::Null,
            };
            values.push((inner_field.name.clone(), value));
        }
        state.lists[at].push(Json::Object(values));
    }
    stack.pop();
    if stack.is_empty() {
        *root_closed = true;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::forms::{Unread, XmlField, XmlForm, XmlName, XmlPath, XmlTake};
    use crate::json::Json;
    use crate::shape::Kind;

    const NS: &str = "urn:example:orders";

    fn name(text: &str) -> XmlName {
        XmlName {
            namespace: Some(NS.to_owned()),
            local: text.to_owned(),
        }
    }

    fn one(field: &str, elements: &[&str], attribute: Option<&str>, kind: Kind) -> XmlField {
        XmlField {
            name: field.to_owned(),
            take: XmlTake::One(XmlPath {
                elements: elements.iter().map(|e| name(e)).collect(),
                attribute: attribute.map(|a| XmlName {
                    namespace: None,
                    local: a.to_owned(),
                }),
            }),
            kind,
            required: true,
        }
    }

    fn form(fields: Vec<XmlField>) -> XmlForm {
        XmlForm {
            fields,
            max_depth: 8,
            max_elements: 20,
        }
    }

    const ORDER: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!-- the document's own prefix is not the catalog's -->
<x:order xmlns:x="urn:example:orders" id="A-100">
  <x:customer>Example &amp; Co &#x41;</x:customer>
  <items xmlns="urn:example:orders">
    <item code="L-1"><quantity> 2 </quantity></item>
    <item code="L-2"><quantity><![CDATA[4]]></quantity></item>
  </items>
</x:order>"#;

    #[test]
    fn declared_paths_give_one_record_of_typed_values() {
        let items = XmlField {
            name: "items".to_owned(),
            take: XmlTake::Each {
                elements: vec![name("order"), name("items"), name("item")],
                fields: vec![
                    one("code", &[], Some("code"), Kind::String),
                    one("quantity", &["quantity"], None, Kind::Integer),
                ],
            },
            kind: Kind::String,
            required: true,
        };
        let read = super::read(
            ORDER.as_bytes(),
            &form(vec![
                one("order_id", &["order"], Some("id"), Kind::String),
                one("customer", &["order", "customer"], None, Kind::String),
                items,
            ]),
        )
        .unwrap();
        assert_eq!(read.get("order_id"), Some(&Json::Text("A-100".to_owned())));
        assert_eq!(
            read.get("customer"),
            Some(&Json::Text("Example & Co A".to_owned()))
        );
        let Some(Json::List(items)) = read.get("items") else {
            panic!("{read:?}");
        };
        assert_eq!(items.len(), 2);
        assert_eq!(items[1].get("code"), Some(&Json::Text("L-2".to_owned())));
        assert_eq!(
            items[0].get("quantity"),
            Some(&Json::Number("2".to_owned()))
        );
    }

    #[test]
    fn a_path_that_matches_nothing_or_twice_and_a_document_type_are_refused() {
        let nothing = super::read(
            ORDER.as_bytes(),
            &form(vec![one("note", &["order", "note"], None, Kind::String)]),
        );
        assert!(matches!(nothing, Err(Unread::Shape(why)) if why.contains("matches nothing")));
        let mut optional = one("note", &["order", "note"], None, Kind::String);
        optional.required = false;
        let read = super::read(ORDER.as_bytes(), &form(vec![optional])).unwrap();
        assert_eq!(read.get("note"), Some(&Json::Null));

        let twice = super::read(
            ORDER.as_bytes(),
            &form(vec![one(
                "code",
                &["order", "items", "item"],
                Some("code"),
                Kind::String,
            )]),
        );
        assert!(matches!(twice, Err(Unread::Shape(why)) if why.contains("more than one")));

        let declared = "<?xml version=\"1.0\"?><!DOCTYPE o [<!ENTITY a \"b\">]><o>&a;</o>";
        assert_eq!(
            super::read(declared.as_bytes(), &form(Vec::new())),
            Err(Unread::DocumentType)
        );
        // An entity no document type defines is refused, never left as it is.
        let undeclared = "<order xmlns=\"urn:example:orders\"><customer>&who;</customer></order>";
        assert!(matches!(
            super::read(
                undeclared.as_bytes(),
                &form(vec![one(
                    "customer",
                    &["order", "customer"],
                    None,
                    Kind::String
                )])
            ),
            Err(Unread::Shape(_))
        ));
        // Mixed content where text is declared is refused.
        let mixed = "<order xmlns=\"urn:example:orders\"><customer>a<b/></customer></order>";
        assert!(matches!(
            super::read(
                mixed.as_bytes(),
                &form(vec![one(
                    "customer",
                    &["order", "customer"],
                    None,
                    Kind::String
                )])
            ),
            Err(Unread::Shape(_))
        ));
    }

    #[test]
    fn depth_and_the_number_of_elements_are_bounded() {
        let deep = format!("{}{}", "<a>".repeat(9), "</a>".repeat(9));
        assert!(matches!(
            super::read(deep.as_bytes(), &form(Vec::new())),
            Err(Unread::TooLarge(_))
        ));
        let wide = format!("<a>{}</a>", "<b/>".repeat(20));
        assert!(matches!(
            super::read(wide.as_bytes(), &form(Vec::new())),
            Err(Unread::TooLarge(_))
        ));
        let fits = format!("<a>{}</a>", "<b/>".repeat(19));
        assert!(super::read(fits.as_bytes(), &form(Vec::new())).is_ok());
        for broken in ["", "<a>", "<a></b>", "<a/><b/>", "text<a/>"] {
            assert!(
                matches!(
                    super::read(broken.as_bytes(), &form(Vec::new())),
                    Err(Unread::Shape(_))
                ),
                "{broken:?}"
            );
        }
    }
}
