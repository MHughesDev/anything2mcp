//! A small, bounded reader of XML files into a tree, for the readers of
//! what a program publishes about itself as files: a service's
//! introspection (`dbus`), a scripting dictionary (`sdef`), and an XML
//! property list (`plist`).
//!
//! Nothing here fetches anything and no entity is expanded: a document
//! type is accepted only when the caller's own check accepts its text, and
//! none with declarations of its own (`[`) ever is; of references, only
//! characters and the five names XML itself defines are read. Depth,
//! element count, attribute count, and name length are bounded, and a text
//! outside the root, a second root, or an unclosed element is refused by
//! name (rule 7: a form the reader does not know is a stop).

use std::collections::BTreeMap;

use quick_xml::NsReader;
use quick_xml::XmlVersion;
use quick_xml::escape::resolve_predefined_entity;
use quick_xml::events::{BytesStart, Event};

/// Why a document is not read at all.
pub(crate) type NotRead = &'static str;

/// The most attributes one element may carry.
const MOST_ATTRIBUTES: usize = 32;
/// The longest name of an element or an attribute, in bytes.
const LONGEST_NAME: usize = 255;

/// How far a document may go.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Bounds {
    /// The deepest its elements may nest.
    pub depth: usize,
    /// The most elements it may hold.
    pub elements: usize,
}

/// One element, with its attributes, its own text, and its children.
#[derive(Debug)]
pub(crate) struct Element {
    /// Its name without a prefix.
    pub local: String,
    pub attributes: BTreeMap<String, String>,
    pub children: Vec<Element>,
    /// The text directly inside it, as written, joined.
    pub text: String,
}

impl Element {
    pub(crate) fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes.get(name).map(String::as_str)
    }
}

/// Parses a document into a tree, within the bounds. `doctype` is given the
/// text of the document type, when there is one, and says whether the
/// caller takes it.
pub(crate) fn parse(
    text: &str,
    bounds: Bounds,
    doctype: &dyn Fn(&str) -> bool,
) -> Result<Element, NotRead> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut reader = NsReader::from_str(text);
    let mut version = XmlVersion::Implicit1_0;
    let mut stack: Vec<Element> = Vec::new();
    let mut root: Option<Element> = None;
    let mut elements = 0_usize;
    let mut first = true;
    loop {
        let event = reader
            .read_event()
            .map_err(|_| "it is not well-formed XML")?;
        let at_start = std::mem::take(&mut first);
        let (start, empty) = match event {
            Event::Eof => break,
            Event::DocType(said) => {
                let said = said.as_ref().to_owned();
                // A document type with declarations of its own defines
                // entities: never. Any other, only as the caller takes it,
                // and never fetched.
                if said.contains('[') || !doctype(&said) || root.is_some() {
                    return Err(
                        "it declares a document type other than the one the format uses, or one with declarations of its own, which the reader refuses so that no entity is expanded and nothing is fetched",
                    );
                }
                continue;
            }
            Event::Decl(decl) => {
                if !at_start {
                    return Err("its XML declaration is not at its start");
                }
                if let Some(encoding) = decl.encoding() {
                    let encoding =
                        encoding.map_err(|_| "its XML declaration is not well formed")?;
                    if !encoding.eq_ignore_ascii_case("utf-8") {
                        return Err("it declares an encoding other than UTF-8");
                    }
                }
                version = decl
                    .xml_version()
                    .map_err(|_| "its XML declaration is not well formed")?;
                continue;
            }
            Event::Comment(_) | Event::PI(_) => continue,
            Event::Text(part) => {
                let part = part.xml_content(version);
                match stack.last_mut() {
                    Some(open) => open.text.push_str(&part),
                    None if part.trim().is_empty() => {}
                    None => return Err("it holds text outside its root element"),
                }
                continue;
            }
            Event::CData(part) => {
                let Some(open) = stack.last_mut() else {
                    return Err("it holds text outside its root element");
                };
                open.text.push_str(&part.xml_content(version));
                continue;
            }
            Event::GeneralRef(reference) => {
                let Some(open) = stack.last_mut() else {
                    return Err("it holds a reference outside its root element");
                };
                match reference.resolve_char_ref() {
                    Ok(Some(c)) => open.text.push(c),
                    Ok(None) => open.text.push_str(
                        resolve_predefined_entity(&reference.xml_content(version)).ok_or(
                            "it names an entity XML does not define, which is never expanded",
                        )?,
                    ),
                    Err(_) => return Err("it names a character that is not one"),
                }
                continue;
            }
            Event::End(_) => {
                let Some(done) = stack.pop() else {
                    return Err("it is not well-formed XML");
                };
                match stack.last_mut() {
                    Some(parent) => parent.children.push(done),
                    None => root = Some(done),
                }
                continue;
            }
            Event::Start(start) => (start, false),
            Event::Empty(start) => (start, true),
        };
        if root.is_some() {
            return Err("it holds more than one root element");
        }
        elements += 1;
        if elements > bounds.elements {
            return Err("it holds more elements than the reader takes from one document");
        }
        if stack.len() >= bounds.depth {
            return Err("its elements nest deeper than the reader takes");
        }
        let element = element(&start, version)?;
        if empty {
            match stack.last_mut() {
                Some(parent) => parent.children.push(element),
                None => root = Some(element),
            }
        } else {
            stack.push(element);
        }
    }
    if !stack.is_empty() {
        return Err("it ends inside an element");
    }
    root.ok_or("it holds no element")
}

fn element(start: &BytesStart<'_>, version: XmlVersion) -> Result<Element, NotRead> {
    if start.name().as_ref().len() > LONGEST_NAME {
        return Err("an element's name is longer than the reader takes");
    }
    let local = start.local_name().as_ref().to_owned();
    let mut attributes = BTreeMap::new();
    for attribute in start.attributes() {
        let attribute = attribute.map_err(|_| "an attribute is not written as XML defines")?;
        if attribute.key.as_ref().len() > LONGEST_NAME {
            return Err("an attribute's name is longer than the reader takes");
        }
        if attributes.len() == MOST_ATTRIBUTES {
            return Err("an element carries more attributes than the reader takes");
        }
        let value = attribute
            .normalized_value_with(version, 1, resolve_predefined_entity)
            .map_err(
                |_| "an attribute names an entity XML does not define, which is never expanded",
            )?
            .into_owned();
        attributes.insert(attribute.key.as_ref().to_owned(), value);
    }
    Ok(Element {
        local,
        attributes,
        children: Vec::new(),
        text: String::new(),
    })
}
