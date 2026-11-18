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

use std::sync::atomic::AtomicBool;

use quick_xml::NsReader;
use quick_xml::XmlVersion;
use quick_xml::escape::resolve_predefined_entity;
use quick_xml::events::{BytesStart, Event};
use quick_xml::name::{NamespaceError, ResolveResult};

use crate::forms::{
    Charset, MAX_ATTRIBUTE_BYTES, MAX_NAME_BYTES, MAX_NAMESPACE_BYTES, MAX_NAMESPACES, Unread,
    Work, XmlField, XmlForm, XmlName, XmlPath, XmlTake, typed,
};
use crate::json::Json;
use crate::shape::Kind;

/// Whether a name the document resolves is the declared one. The
/// document's namespace is borrowed, never copied (decision 0249).
fn equals(name: &XmlName, namespace: &ResolveResult<'_>, local: &str) -> bool {
    let namespace_fits = match (namespace, &name.namespace) {
        (ResolveResult::Unbound, None) => true,
        (ResolveResult::Bound(uri), Some(declared)) => uri.as_ref() == declared.as_str(),
        _ => false,
    };
    namespace_fits && local == name.local
}

/// The declared names of a form, each once. An open element is held as
/// the position of the declared name it is, or as none.
struct Names<'f> {
    all: Vec<&'f XmlName>,
}

impl<'f> Names<'f> {
    fn of(form: &'f XmlForm) -> Self {
        let mut all: Vec<&'f XmlName> = Vec::new();
        fn add<'f>(all: &mut Vec<&'f XmlName>, path: &'f [XmlName]) {
            for name in path {
                if !all.contains(&name) {
                    all.push(name);
                }
            }
        }
        for field in &form.fields {
            match &field.take {
                XmlTake::One(path) => add(&mut all, &path.elements),
                XmlTake::Each { elements, fields } => {
                    add(&mut all, elements);
                    for inner in fields {
                        if let XmlTake::One(path) = &inner.take {
                            add(&mut all, &path.elements);
                        }
                    }
                }
            }
        }
        Self { all }
    }

    /// The declared name an element is, by position.
    fn position(
        &self,
        namespace: &ResolveResult<'_>,
        local: &str,
        work: &mut Work<'_>,
    ) -> Result<Option<usize>, Unread> {
        work.spend(u64::try_from(self.all.len()).unwrap_or(u64::MAX))?;
        Ok(self
            .all
            .iter()
            .position(|name| equals(name, namespace, local)))
    }
}

/// The stack of open elements: each the position of the declared name it
/// is, or `None` for a name nothing declares.
type Stack = Vec<Option<usize>>;

fn same(names: &Names<'_>, stack: &[Option<usize>], path: &[XmlName]) -> bool {
    stack.len() == path.len()
        && stack
            .iter()
            .zip(path)
            .all(|(open, name)| open.is_some_and(|at| names.all[at] == name))
}

/// What one element costs to compare with a form, at most: one for each
/// declared field and each step of its path, and one for each declared
/// name. The budget of a read grows with it.
fn path_length(take: &XmlTake) -> usize {
    match take {
        XmlTake::One(path) => path.elements.len(),
        XmlTake::Each { elements, .. } => elements.len(),
    }
}

fn weight(form: &XmlForm, names: &Names<'_>) -> usize {
    let path = path_length;
    let fields: usize = form
        .fields
        .iter()
        .map(|field| {
            let inner: usize = match &field.take {
                XmlTake::One(_) => 0,
                XmlTake::Each { fields, .. } => {
                    fields.iter().map(|inner| 1 + path(&inner.take)).sum()
                }
            };
            1 + path(&field.take) + inner
        })
        .sum();
    fields + names.all.len()
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
    names: Names<'f>,
    top: Vec<Slot>,
    lists: Vec<Vec<Json>>,
    open: Vec<Option<OpenRecord>>,
    gathering: Option<Gathering>,
}

fn shape(why: impl Into<String>) -> Unread {
    Unread::Shape(why.into())
}

fn too_large(why: impl Into<String>) -> Unread {
    Unread::TooLarge(why.into())
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

/// Holds every attribute of an element that opens to the bounds: the
/// length of its name, of its value as written, and of the address of a
/// namespace it declares. Duplicate names are refused by the reader.
fn bounded(start: &BytesStart<'_>, work: &mut Work<'_>) -> Result<(), Unread> {
    if start.name().as_ref().len() > MAX_NAME_BYTES {
        return Err(too_large(format!(
            "an element's name is longer than {MAX_NAME_BYTES} bytes"
        )));
    }
    for attribute in start.attributes() {
        work.spend(1)?;
        let attribute =
            attribute.map_err(|_| shape("an attribute is not written as the format defines"))?;
        if attribute.key.as_ref().len() > MAX_NAME_BYTES {
            return Err(too_large(format!(
                "an attribute's name is longer than {MAX_NAME_BYTES} bytes"
            )));
        }
        if attribute.key.as_namespace_binding().is_some() {
            if attribute.value.len() > MAX_NAMESPACE_BYTES {
                return Err(too_large(format!(
                    "a namespace's address is longer than {MAX_NAMESPACE_BYTES} bytes"
                )));
            }
        } else if attribute.value.len() > MAX_ATTRIBUTE_BYTES {
            return Err(too_large(format!(
                "an attribute's value is longer than {MAX_ATTRIBUTE_BYTES} bytes"
            )));
        }
    }
    Ok(())
}

/// The value of one attribute of an element, when it has it. Every
/// attribute is looked at: two that resolve to the declared name, through
/// two prefixes of one namespace, are a stop.
fn attribute(
    reader: &NsReader<&[u8]>,
    element: &BytesStart<'_>,
    wanted: &XmlName,
    field: &str,
    version: XmlVersion,
    work: &mut Work<'_>,
) -> Result<Option<String>, Unread> {
    let mut found = None;
    for attribute in element.attributes() {
        work.spend(1)?;
        let attribute =
            attribute.map_err(|_| shape("an attribute is not written as the format defines"))?;
        let (namespace, local) = reader.resolver().resolve_attribute(attribute.key);
        if let ResolveResult::Unknown(_) = namespace {
            return Err(shape(
                "an attribute uses a prefix the document does not declare",
            ));
        }
        if !equals(wanted, &namespace, local.into_inner()) {
            continue;
        }
        if found.is_some() {
            return Err(twice(field));
        }
        work.spend(u64::try_from(attribute.value.len()).unwrap_or(u64::MAX))?;
        let value = attribute
            .normalized_value_with(version, 1, resolve_predefined_entity)
            .map_err(|_| {
                shape("an attribute names a character or an entity the format does not define")
            })?;
        found = Some(value.into_owned());
    }
    Ok(found)
}

/// Whether text is only the white space the format defines.
fn is_space(text: &str) -> bool {
    text.bytes()
        .all(|byte| matches!(byte, b' ' | b'\t' | b'\n' | b'\r'))
}

/// Why the reader stopped, as the refusal it is.
fn unread(error: &quick_xml::Error, at: u64) -> Unread {
    match error {
        quick_xml::Error::Namespace(NamespaceError::TooManyBindings(_)) => too_large(format!(
            "the document declares more than {MAX_NAMESPACES} namespaces at once"
        )),
        _ => shape(format!("the body is not well-formed XML near byte {at}")),
    }
}

/// Reads an XML answer into its record. `said` is the character set the
/// answer's header names; the document's own declaration must agree with
/// it. The read stops past its count of work, or once `stop` is set.
pub(crate) fn read(
    body: &[u8],
    form: &XmlForm,
    said: Option<Charset>,
    stop: &AtomicBool,
) -> Result<Json, Unread> {
    let mut work = Work::for_answer(body.len(), weight(form, &Names::of(form)), stop);
    read_counted(body, form, said, &mut work)
}

/// [`read`], with the count of work given.
pub(crate) fn read_counted(
    body: &[u8],
    form: &XmlForm,
    said: Option<Charset>,
    work: &mut Work<'_>,
) -> Result<Json, Unread> {
    let marked = body.starts_with(b"\xEF\xBB\xBF");
    let body = body.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(body);
    if marked && said == Some(Charset::UsAscii) {
        return Err(shape(
            "the body starts with UTF-8's byte-order mark, and its content type names US-ASCII",
        ));
    }
    let text = std::str::from_utf8(body).map_err(|error| {
        shape(format!(
            "the body is not UTF-8 at byte {}",
            error.valid_up_to()
        ))
    })?;
    let mut reader = NsReader::from_str(text);
    reader
        .resolver_mut()
        .set_max_namespace_bindings(MAX_NAMESPACES);
    let mut version = XmlVersion::Implicit1_0;
    let mut state = State {
        form,
        names: Names::of(form),
        top: form.fields.iter().map(|_| Slot::default()).collect(),
        lists: form.fields.iter().map(|_| Vec::new()).collect(),
        open: form.fields.iter().map(|_| None).collect(),
        gathering: None,
    };
    let mut stack: Stack = Vec::new();
    let mut elements: u64 = 0;
    let mut root_closed = false;
    let mut first = true;

    loop {
        work.spend(1)?;
        let event = reader
            .read_event()
            .map_err(|error| unread(&error, reader.error_position()))?;
        let at_start = std::mem::take(&mut first);
        let (start, empty) = match event {
            Event::Eof => break,
            Event::DocType(_) => return Err(Unread::DocumentType),
            Event::Decl(decl) => {
                if !at_start {
                    return Err(shape("the XML declaration is not at the start of the body"));
                }
                if let Some(encoding) = decl.encoding() {
                    let encoding =
                        encoding.map_err(|_| shape("the XML declaration is not well formed"))?;
                    let declared = Charset::from_word(&encoding)
                        .filter(|charset| matches!(charset, Charset::Utf8 | Charset::UsAscii))
                        .ok_or_else(|| {
                            shape("the document declares an encoding other than UTF-8")
                        })?;
                    // The header and the document agree, or the answer is
                    // refused: neither is taken over the other.
                    if said.is_some_and(|said| said != declared) {
                        return Err(shape(
                            "the document declares an encoding other than the one its content type names",
                        ));
                    }
                    if marked && declared != Charset::Utf8 {
                        return Err(shape(
                            "the body starts with UTF-8's byte-order mark, and the document declares another encoding",
                        ));
                    }
                    if declared == Charset::UsAscii && !body.is_ascii() {
                        return Err(shape(
                            "the document declares US-ASCII, and the body is not ASCII",
                        ));
                    }
                }
                version = decl
                    .xml_version()
                    .map_err(|_| shape("the XML declaration is not well formed"))?;
                continue;
            }
            Event::Comment(_) | Event::PI(_) => continue,
            Event::Text(part) => {
                match &mut state.gathering {
                    Some(gathering) if gathering.depth == stack.len() => {
                        let part = part.xml_content(version);
                        work.spend(u64::try_from(part.len()).unwrap_or(u64::MAX))?;
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
                if stack.is_empty() {
                    return Err(shape("the body holds a CDATA section outside its root element"));
                }
                if let Some(gathering) = &mut state.gathering
                    && gathering.depth == stack.len()
                {
                    let part = part.xml_content(version);
                    work.spend(u64::try_from(part.len()).unwrap_or(u64::MAX))?;
                    gathering.text.push_str(&part);
                }
                continue;
            }
            Event::GeneralRef(reference) => {
                if stack.is_empty() {
                    return Err(shape("the body holds a reference outside its root element"));
                }
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
        bounded(&start, work)?;
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
        if let ResolveResult::Unknown(_) = namespace {
            return Err(shape(
                "an element uses a prefix the document does not declare",
            ));
        }
        let position = state
            .names
            .position(&namespace, local.into_inner(), work)?;
        stack.push(position);
        elements += 1;
        if stack.len() > usize::try_from(form.max_depth).unwrap_or(usize::MAX) {
            return Err(too_large(format!(
                "the document is nested deeper than {} elements",
                form.max_depth
            )));
        }
        if elements > form.max_elements {
            return Err(too_large(format!(
                "the document holds more than {} elements",
                form.max_elements
            )));
        }
        open(&mut state, &reader, &start, &stack, version, work)?;
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

/// Where the reader is when an element opens.
struct Here<'a, 'r> {
    reader: &'a NsReader<&'r [u8]>,
    start: &'a BytesStart<'a>,
    version: XmlVersion,
}

/// One field whose path ends at the element that opens: its attribute is
/// read now, and its text is gathered until the element closes.
fn found(
    state: &mut State<'_>,
    here: &Here<'_, '_>,
    target: Target,
    field: &XmlField,
    path: &XmlPath,
    work: &mut Work<'_>,
) -> Result<Option<(Target, Kind)>, Unread> {
    match &path.attribute {
        Some(wanted) => {
            let value = attribute(
                here.reader,
                here.start,
                wanted,
                &field.name,
                here.version,
                work,
            )?;
            if let Some(value) = value {
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
    stack: &[Option<usize>],
    version: XmlVersion,
    work: &mut Work<'_>,
) -> Result<(), Unread> {
    let form: &XmlForm = state.form;
    let here = Here {
        reader,
        start,
        version,
    };
    let mut gather: Vec<(Target, Kind)> = Vec::new();
    for (at, field) in form.fields.iter().enumerate() {
        work.spend(1 + u64::try_from(path_length(&field.take)).unwrap_or(u64::MAX))?;
        match &field.take {
            XmlTake::One(path) => {
                if same(&state.names, stack, &path.elements) {
                    gather.extend(found(state, &here, Target::Top(at), field, path, work)?);
                }
            }
            XmlTake::Each {
                elements,
                fields: inner,
            } => {
                if same(&state.names, stack, elements) {
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
                    work.spend(
                        1 + u64::try_from(path_length(&inner_field.take)).unwrap_or(u64::MAX),
                    )?;
                    if let XmlTake::One(path) = &inner_field.take
                        && same(&state.names, relative, &path.elements)
                    {
                        let target = Target::Inner(at, inner_at);
                        gather.extend(found(state, &here, target, inner_field, path, work)?);
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
fn close(state: &mut State<'_>, stack: &mut Stack, root_closed: &mut bool) -> Result<(), Unread> {
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
