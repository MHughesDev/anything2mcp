//! Readers of what a D-Bus service publishes about itself (decision 0276,
//! point 1; decision 0279, point 3): the introspection a running service
//! answers, an interface description kept as a file, and the `.service`
//! file that says which program a bus name starts.
//!
//! All three are text, read here with no system call, so the readers run
//! on every system and are tested on every system. Nothing in a file is
//! run, nothing it names is fetched, and no entity it defines is expanded:
//! the one document type the format uses is accepted, and any other, or
//! one that carries declarations of its own, is refused. Bounded in size,
//! depth, and count; a form the reader does not know is a named stop, never
//! a guess (rule 7).
//!
//! What a property's `access` says is carried as the document states it
//! (`read`, `write`, `readwrite`): it is the evidence that a property only
//! reads, and the drafting rule reads only that (decision 0316).

use std::collections::BTreeMap;

use quick_xml::NsReader;
use quick_xml::XmlVersion;
use quick_xml::escape::resolve_predefined_entity;
use quick_xml::events::{BytesStart, Event};

/// The deepest an introspection document's elements may nest.
const DEEPEST: usize = 16;
/// The most elements one document may hold.
const MOST_ELEMENTS: usize = 20_000;
/// The most attributes one element may carry.
const MOST_ATTRIBUTES: usize = 16;
/// The longest name of an element or an attribute, in bytes.
const LONGEST_NAME: usize = 255;
/// The most interfaces, children, members of one kind, or arguments one
/// document may hold in one place.
const MOST_IN_ONE_PLACE: usize = 2_000;

/// The public identifier of the one document type the format uses.
const DOCTYPE_PUBLIC: &str = "-//freedesktop//DTD D-BUS Object Introspection 1.0//EN";

/// Why a document is not read at all.
pub(crate) type NotRead = &'static str;

/// What a property's declaration says of who may use it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Access {
    /// A caller may read it.
    Read,
    /// A caller may set it.
    Write,
    /// A caller may do either.
    ReadWrite,
}

impl Access {
    /// The word the document writes.
    pub(crate) fn word(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
            Self::ReadWrite => "readwrite",
        }
    }

    /// Reads the word a document or a finding writes.
    pub(crate) fn from_word(word: &str) -> Option<Self> {
        match word {
            "read" => Some(Self::Read),
            "write" => Some(Self::Write),
            "readwrite" => Some(Self::ReadWrite),
            _ => None,
        }
    }
}

/// One argument of a method or a signal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Argument {
    /// Its name, where the document gives one.
    pub name: Option<String>,
    /// Its type, as a D-Bus signature.
    pub ty: String,
    /// `in` or `out`, where the document says; a signal's has none.
    pub direction: Option<String>,
}

/// One method or signal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Member {
    pub name: String,
    pub args: Vec<Argument>,
}

/// One property.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Property {
    pub name: String,
    /// Its type, as a D-Bus signature.
    pub ty: String,
    pub access: Access,
}

/// One interface an object answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Interface {
    pub name: String,
    pub methods: Vec<Member>,
    pub signals: Vec<Member>,
    pub properties: Vec<Property>,
}

impl Interface {
    /// Whether the bus itself defines this interface: every object answers
    /// it, and it says nothing about the service.
    pub(crate) fn is_standard(&self) -> bool {
        self.name.starts_with("org.freedesktop.DBus.")
    }
}

/// What one document says of one object.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Object {
    pub interfaces: Vec<Interface>,
    /// The names of the child objects, relative to this one.
    pub children: Vec<String>,
}

/// One element of the document, with its attributes.
#[derive(Debug)]
struct Element {
    local: String,
    attributes: BTreeMap<String, String>,
    children: Vec<Element>,
}

impl Element {
    fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes.get(name).map(String::as_str)
    }
}

/// Whether text is a well-formed D-Bus name or member name's characters: the
/// bounded ASCII a bus accepts. A name from a service is its own words,
/// and nothing else is let through into a finding or a draft.
fn is_name(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= LONGEST_NAME
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
}

/// Whether text is a D-Bus type signature's characters, bounded.
fn is_signature(text: &str) -> bool {
    text.len() <= LONGEST_NAME
        && text.chars().all(|c| {
            c.is_ascii_alphabetic() || matches!(c, '(' | ')' | '{' | '}' | '[' | ']' | '*' | '?')
        })
}

/// Whether text is a valid object path, as the bus defines it.
pub(crate) fn is_object_path(text: &str) -> bool {
    if text == "/" {
        return true;
    }
    text.len() <= 4096
        && text.starts_with('/')
        && !text.ends_with('/')
        && text[1..].split('/').all(|part| {
            !part.is_empty() && part.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        })
}

/// Parses a document into a tree, within the bounds.
fn tree(text: &str) -> Result<Element, NotRead> {
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
            Event::DocType(doctype) => {
                let said = doctype.as_ref().to_owned();
                // The format's one document type, with no declarations of
                // its own: so that no entity is defined and nothing is
                // fetched.
                if said.contains('[') || !said.contains(DOCTYPE_PUBLIC) || root.is_some() {
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
                if stack.is_empty() && !part.xml_content(version).trim().is_empty() {
                    return Err("it holds text outside its root element");
                }
                continue;
            }
            Event::CData(_) | Event::GeneralRef(_) => {
                return Err("it holds text or a reference where the format has none");
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
        if elements > MOST_ELEMENTS {
            return Err("it holds more elements than the reader takes from one document");
        }
        if stack.len() >= DEEPEST {
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
    })
}

fn bounded<T>(items: &[T]) -> Result<(), NotRead> {
    if items.len() > MOST_IN_ONE_PLACE {
        Err("it lists more of one thing than the reader takes")
    } else {
        Ok(())
    }
}

fn member(of: &Element) -> Result<Member, NotRead> {
    let name = of.attribute("name").ok_or("a member has no name")?;
    if !is_name(name) || name.contains('.') {
        return Err("a member's name is not one a bus accepts");
    }
    let mut args = Vec::new();
    for child in &of.children {
        match child.local.as_str() {
            "arg" => {
                let ty = child.attribute("type").ok_or("an argument has no type")?;
                if !is_signature(ty) || ty.is_empty() {
                    return Err("an argument's type is not a signature");
                }
                let argument_name = child.attribute("name");
                if argument_name.is_some_and(|argument| !is_name(argument)) {
                    return Err("an argument's name is not one the reader takes");
                }
                let direction = child.attribute("direction");
                if direction.is_some_and(|word| word != "in" && word != "out") {
                    return Err("an argument's direction is neither `in` nor `out`");
                }
                args.push(Argument {
                    name: argument_name.map(str::to_owned),
                    ty: ty.to_owned(),
                    direction: direction.map(str::to_owned),
                });
            }
            "annotation" => {}
            _ => return Err("a member holds an element the reader does not know"),
        }
    }
    bounded(&args)?;
    Ok(Member {
        name: name.to_owned(),
        args,
    })
}

fn interface(of: &Element) -> Result<Interface, NotRead> {
    let name = of.attribute("name").ok_or("an interface has no name")?;
    if !is_name(name) {
        return Err("an interface's name is not one a bus accepts");
    }
    let mut out = Interface {
        name: name.to_owned(),
        methods: Vec::new(),
        signals: Vec::new(),
        properties: Vec::new(),
    };
    for child in &of.children {
        match child.local.as_str() {
            "method" => out.methods.push(member(child)?),
            "signal" => out.signals.push(member(child)?),
            "property" => {
                let property = child.attribute("name").ok_or("a property has no name")?;
                if !is_name(property) || property.contains('.') {
                    return Err("a property's name is not one a bus accepts");
                }
                let ty = child.attribute("type").ok_or("a property has no type")?;
                if !is_signature(ty) || ty.is_empty() {
                    return Err("a property's type is not a signature");
                }
                // Unknown means stop: a property whose access is not one
                // of the three words is not read as a read or as a write.
                let access = child
                    .attribute("access")
                    .and_then(Access::from_word)
                    .ok_or("a property states no access, or one the reader does not know")?;
                out.properties.push(Property {
                    name: property.to_owned(),
                    ty: ty.to_owned(),
                    access,
                });
            }
            "annotation" => {}
            _ => return Err("an interface holds an element the reader does not know"),
        }
    }
    bounded(&out.methods)?;
    bounded(&out.signals)?;
    bounded(&out.properties)?;
    Ok(out)
}

/// Reads one introspection document: a `node`, as a service answers and as
/// a file of interfaces is written, or a bare `interface`, as the files
/// kept beside a service's source are.
///
/// # Errors
///
/// A named reason when the text is not a document of the format, or holds
/// a form the reader does not know.
pub(crate) fn read_object(text: &str) -> Result<Object, NotRead> {
    let root = tree(text)?;
    let mut out = Object::default();
    match root.local.as_str() {
        "interface" => out.interfaces.push(interface(&root)?),
        "node" => {
            for child in &root.children {
                match child.local.as_str() {
                    "interface" => out.interfaces.push(interface(child)?),
                    "node" => {
                        let name = child.attribute("name").ok_or("a child has no name")?;
                        if name.is_empty()
                            || name.len() > LONGEST_NAME
                            || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                        {
                            return Err("a child's name is not one an object path may hold");
                        }
                        out.children.push(name.to_owned());
                    }
                    "annotation" => {}
                    _ => return Err("a node holds an element the reader does not know"),
                }
            }
        }
        _ => return Err("its root is neither a `node` nor an `interface`"),
    }
    bounded(&out.interfaces)?;
    bounded(&out.children)?;
    Ok(out)
}

/// What a `.service` file says: the bus name it starts a program for, the
/// program, and the user it names.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct ServiceFile {
    /// The `Name` key: the bus name the file is for.
    pub name: String,
    /// The first word of `Exec`: the program, with no argument kept, since
    /// an argument may hold a secret.
    pub program: Option<String>,
    /// The `User` key, which a system bus honours.
    pub user: Option<String>,
    /// The `SystemdService` key.
    pub systemd_service: Option<String>,
    /// How many keys the group held that the reader does not keep.
    pub other_keys: u64,
}

/// The longest `.service` file the reader takes, in lines.
const MOST_LINES: usize = 200;

/// Reads a `.service` file: an ini file with one group, `[D-BUS Service]`.
///
/// # Errors
///
/// A named reason when it is not that, or names no bus name.
pub(crate) fn read_service_file(text: &str) -> Result<ServiceFile, NotRead> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut out = ServiceFile::default();
    let mut in_group = false;
    let mut seen_group = false;
    for (at, line) in text.lines().enumerate() {
        if at >= MOST_LINES {
            return Err("it holds more lines than the reader takes");
        }
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(group) = line
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'))
        {
            if group == "D-BUS Service" {
                if seen_group {
                    return Err("it holds the group `D-BUS Service` more than once");
                }
                seen_group = true;
                in_group = true;
            } else {
                in_group = false;
            }
            continue;
        }
        if !in_group {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err("a line of the group is not `key=value`");
        };
        let (key, value) = (key.trim(), value.trim());
        match key {
            "Name" => {
                if !is_name(value) || !value.contains('.') {
                    return Err("its `Name` is not a bus name");
                }
                out.name = value.to_owned();
            }
            "Exec" => {
                let program = value.split_whitespace().next().unwrap_or_default();
                if program.is_empty() || program.chars().any(char::is_control) {
                    return Err("its `Exec` names no program");
                }
                out.program = Some(program.to_owned());
            }
            "User" => out.user = Some(value.to_owned()),
            "SystemdService" => out.systemd_service = Some(value.to_owned()),
            _ => out.other_keys += 1,
        }
    }
    if !seen_group {
        return Err("it holds no group `D-BUS Service`");
    }
    if out.name.is_empty() {
        return Err("it holds no `Name`");
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOCTYPE: &str = r#"<!DOCTYPE node PUBLIC "-//freedesktop//DTD D-BUS Object Introspection 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/introspect.dtd">"#;

    fn node(body: &str) -> String {
        format!("{DOCTYPE}\n<node>{body}</node>")
    }

    #[test]
    fn a_service_answer_is_read_with_each_propertys_access() {
        let text = node(
            r#"<interface name="org.freedesktop.DBus.Introspectable">
                 <method name="Introspect"><arg type="s" direction="out"/></method>
               </interface>
               <interface name="org.example.Desk.Orders">
                 <method name="Count"><arg name="total" type="u" direction="out"/></method>
                 <signal name="Changed"><arg name="order" type="s"/></signal>
                 <property name="Total" type="u" access="read"/>
                 <property name="Label" type="s" access="readwrite"/>
                 <property name="Secret" type="s" access="write"/>
                 <annotation name="org.example.Note" value="x"/>
               </interface>
               <node name="Orders"/>"#,
        );
        let read = read_object(&text).unwrap();
        assert_eq!(read.children, ["Orders"]);
        assert_eq!(read.interfaces.len(), 2);
        assert!(read.interfaces[0].is_standard());
        let orders = &read.interfaces[1];
        assert!(!orders.is_standard());
        assert_eq!(orders.methods[0].name, "Count");
        assert_eq!(orders.methods[0].args[0].direction.as_deref(), Some("out"));
        assert_eq!(orders.signals[0].name, "Changed");
        let access: Vec<_> = orders.properties.iter().map(|p| p.access).collect();
        assert_eq!(access, [Access::Read, Access::ReadWrite, Access::Write]);
    }

    #[test]
    fn a_bare_interface_file_is_read() {
        let read = read_object(
            r#"<interface name="org.example.Desk.Orders"><property name="Total" type="u" access="read"/></interface>"#,
        )
        .unwrap();
        assert_eq!(read.interfaces[0].properties[0].name, "Total");
    }

    #[test]
    fn a_property_with_no_known_access_is_a_stop_and_never_a_read() {
        for access in ["", " access=\"rw\"", " access=\"READ\""] {
            let text = node(&format!(
                r#"<interface name="org.example.A"><property name="P" type="u"{access}/></interface>"#
            ));
            assert_eq!(
                read_object(&text).unwrap_err(),
                "a property states no access, or one the reader does not know",
                "{access}"
            );
        }
    }

    #[test]
    fn a_hostile_document_is_refused_by_name() {
        let entity = "<!DOCTYPE node [<!ENTITY a \"aaaa\">]><node><interface name=\"&a;\"/></node>";
        assert!(read_object(entity).unwrap_err().contains("document type"));
        let other = r#"<!DOCTYPE node SYSTEM "http://example.invalid/x.dtd"><node/>"#;
        assert!(read_object(other).unwrap_err().contains("document type"));
        // A reference to an entity the document does not define.
        let reference = node(r#"<interface name="org.example.&a;"/>"#);
        assert!(read_object(&reference).is_err());
        // Deep nesting, many roots, a name a bus does not accept.
        let deep = format!("{}{}", "<node>".repeat(40), "</node>".repeat(40));
        assert!(read_object(&deep).unwrap_err().contains("nest deeper"));
        assert!(read_object("<node/><node/>").is_err());
        assert!(read_object(&node(r#"<interface name="a b"/>"#)).is_err());
        assert!(read_object(&node(r#"<node name="../etc"/>"#)).is_err());
        assert!(read_object(&node("<surprise/>")).is_err());
        assert!(read_object("<other/>").is_err());
        assert!(read_object("not xml at all").is_err());
        assert!(read_object("").is_err());
        assert!(read_object("<node><interface").is_err());
    }

    #[test]
    fn a_service_file_gives_its_name_and_program_and_keeps_no_argument() {
        let read = read_service_file(
            "# a note\n[D-BUS Service]\nName=org.example.Desk.Idle\nExec=/opt/example/bin/idle --token=hunter2\nUser=root\nSystemdService=idle.service\nAssumedAppArmorLabel=x\n[Other]\nExec=/bin/other\n",
        )
        .unwrap();
        assert_eq!(read.name, "org.example.Desk.Idle");
        assert_eq!(read.program.as_deref(), Some("/opt/example/bin/idle"));
        assert_eq!(read.user.as_deref(), Some("root"));
        assert_eq!(read.systemd_service.as_deref(), Some("idle.service"));
        assert_eq!(read.other_keys, 1);
        assert!(!format!("{read:?}").contains("hunter2"));
    }

    #[test]
    fn a_service_file_that_is_not_one_is_refused_by_name() {
        assert!(read_service_file("[Other]\nName=a.b\n").is_err());
        assert!(read_service_file("[D-BUS Service]\nExec=/bin/x\n").is_err());
        assert!(read_service_file("[D-BUS Service]\nName=nodots\n").is_err());
        assert!(read_service_file("[D-BUS Service]\nName=a.b\nbroken\n").is_err());
        assert!(read_service_file(&"# x\n".repeat(500)).is_err());
    }

    #[test]
    fn object_paths_are_the_buss() {
        assert!(is_object_path("/"));
        assert!(is_object_path("/org/example/Desk"));
        for bad in ["", "org", "/a/", "//a", "/a b", "/a-b", "/../x"] {
            assert!(!is_object_path(bad), "{bad}");
        }
    }
}
