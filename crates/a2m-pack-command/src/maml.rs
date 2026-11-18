//! A reader of PowerShell's XML help files, the form named MAML
//! (`<module>-help.xml`), handed to the scout as a file of the project and
//! read as data (decision 0290).
//!
//! The file is parsed into a small tree, bounded in depth, in elements, and
//! in attributes, and then walked. Nothing in it is run, and nothing it
//! names is fetched: a file that declares a document type is refused before
//! any element is read, so no entity it defines is expanded; of references,
//! only characters and the five names XML itself defines are read. What a
//! command's help says is taken from its name, its synopsis, its syntax
//! items (one for each parameter set), and its parameters; prose sections
//! are passed over whole. An element or an attribute the reader does not
//! know, inside a part it reads, leaves that command unread, counted by
//! reason: it is never guessed at.

use std::collections::BTreeMap;

use a2m_core::Redact;
use quick_xml::NsReader;
use quick_xml::XmlVersion;
use quick_xml::escape::resolve_predefined_entity;
use quick_xml::events::{BytesStart, Event};
use quick_xml::name::ResolveResult;

use crate::metadata::about;

/// The deepest a help file's elements may nest.
const DEEPEST: usize = 32;
/// The most elements one help file may hold.
const MOST_ELEMENTS: usize = 100_000;
/// The most attributes one element may carry.
const MOST_ATTRIBUTES: usize = 32;
/// The longest name of an element or an attribute, in bytes.
const LONGEST_NAME: usize = 128;
/// The most commands read from one file; the rest are counted `bound`.
const MOST_COMMANDS: usize = 500;
/// The most parameter sets of one command.
const MOST_SETS: usize = 32;
/// The most parameters of one parameter set.
const MOST_PARAMETERS: usize = 128;

/// Why a file is not read at all.
pub(crate) type NotHelp = &'static str;

/// The namespaces of the help format. They are names, never fetched.
const MSH: &str = "http://msh";
const MAML: &str = "http://schemas.microsoft.com/maml/2004/10";
const COMMAND: &str = "http://schemas.microsoft.com/maml/dev/command/2004/10";
const DEV: &str = "http://schemas.microsoft.com/maml/dev/2004/10";

/// Which of the format's namespaces an element is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Space {
    /// The root's own, or none.
    Msh,
    Maml,
    Command,
    Dev,
    /// Any other.
    Other,
}

/// One element of the file, with its attributes and its own text.
#[derive(Debug)]
struct Node {
    space: Space,
    local: String,
    /// Attributes without a prefix, by their local name. An attribute in a
    /// namespace is kept under the name `:` and counts as unknown.
    attributes: Vec<(String, String)>,
    children: Vec<Node>,
    text: String,
}

impl Node {
    fn is(&self, space: Space, local: &str) -> bool {
        self.space == space && self.local == local
    }

    fn child(&self, space: Space, local: &str) -> Option<&Node> {
        self.children.iter().find(|child| child.is(space, local))
    }

    fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    /// The text of this element and every element under it, in order.
    fn all_text(&self, out: &mut String) {
        out.push_str(&self.text);
        out.push(' ');
        for child in &self.children {
            child.all_text(out);
        }
    }
}

/// How a parameter takes its value from the pipeline, as the help says.
pub(crate) fn pipeline(text: &str) -> Option<Vec<&'static str>> {
    let lower: String = text
        .to_ascii_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    Some(match lower.as_str() {
        "false" => Vec::new(),
        "true" => vec!["not_stated"],
        "true (byvalue)" => vec!["by_value"],
        "true (bypropertyname)" => vec!["by_property_name"],
        "true (byvalue, bypropertyname)" | "true (bypropertyname, byvalue)" => {
            vec!["by_value", "by_property_name"]
        }
        _ => return None,
    })
}

/// One parameter of one parameter set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PsParameter {
    /// Its name, without the dash.
    pub name: String,
    /// Its .NET type, as the help writes it.
    pub type_name: Option<String>,
    /// Whether it is a switch: its type is `SwitchParameter`.
    pub switch: bool,
    /// Whether the help says it is required; `None` when it does not say.
    pub required: Option<bool>,
    /// Its position: `named`, or a number from 0.
    pub position: Option<String>,
    /// How it takes a value from the pipeline: empty for not at all.
    pub pipeline: Vec<&'static str>,
    /// What the help says of it, redacted.
    pub about: Option<String>,
    /// Whether the type came from `dev:type`, the full name, rather than
    /// from the text of the parameter's value.
    pub full_type: bool,
}

/// One command a help file describes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PsCommand {
    /// Its name, such as `Get-ExampleItem`.
    pub name: String,
    /// The help's synopsis, redacted.
    pub synopsis: Option<String>,
    /// Its parameter sets, one for each syntax item, each with its
    /// parameters in the order the help writes them.
    pub sets: Vec<Vec<PsParameter>>,
}

/// What the reader made of one help file.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct HelpRead {
    /// The commands read whole.
    pub commands: Vec<PsCommand>,
    /// How many commands were read.
    pub read: u64,
    /// How many were not, by reason.
    pub unread: BTreeMap<&'static str, u64>,
}

impl HelpRead {
    fn not_read(&mut self, why: &'static str) {
        *self.unread.entry(why).or_default() += 1;
    }
}

/// Parses the file into a tree, within the bounds.
fn tree(text: &str) -> Result<Node, NotHelp> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut reader = NsReader::from_str(text);
    reader.resolver_mut().set_max_namespace_bindings(64);
    let mut version = XmlVersion::Implicit1_0;
    let mut stack: Vec<Node> = Vec::new();
    let mut root: Option<Node> = None;
    let mut elements = 0_usize;
    let mut first = true;
    loop {
        let event = reader
            .read_event()
            .map_err(|_| "it is not well-formed XML")?;
        let at_start = std::mem::take(&mut first);
        let (start, empty) = match event {
            Event::Eof => break,
            Event::DocType(_) => {
                return Err(
                    "it declares a document type, which the reader refuses so that no entity is expanded and nothing is fetched",
                );
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
                match stack.last_mut() {
                    Some(open) => open.text.push_str(&part.xml_content(version)),
                    None if part.xml_content(version).trim().is_empty() => {}
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
        if elements > MOST_ELEMENTS {
            return Err("it holds more elements than the reader takes from one file");
        }
        if stack.len() >= DEEPEST {
            return Err("its elements nest deeper than the reader takes");
        }
        let node = node(&reader, &start, version)?;
        if empty {
            match stack.last_mut() {
                Some(parent) => parent.children.push(node),
                None => root = Some(node),
            }
        } else {
            stack.push(node);
        }
    }
    if !stack.is_empty() {
        return Err("it ends inside an element");
    }
    root.ok_or("it holds no element")
}

/// One element that opens, with its attributes.
fn node(
    reader: &NsReader<&[u8]>,
    start: &BytesStart<'_>,
    version: XmlVersion,
) -> Result<Node, NotHelp> {
    if start.name().as_ref().len() > LONGEST_NAME {
        return Err("an element's name is longer than the reader takes");
    }
    let (namespace, local) = reader.resolver().resolve_element(start.name());
    let space = match namespace {
        ResolveResult::Unbound => Space::Msh,
        ResolveResult::Bound(uri) => match uri.as_ref() {
            uri if uri == MSH => Space::Msh,
            uri if uri == MAML => Space::Maml,
            uri if uri == COMMAND => Space::Command,
            uri if uri == DEV => Space::Dev,
            _ => Space::Other,
        },
        ResolveResult::Unknown(_) => {
            return Err("an element uses a prefix the file does not declare");
        }
    };
    let local = local.into_inner().to_owned();
    let mut attributes = Vec::new();
    for attribute in start.attributes() {
        let attribute = attribute.map_err(|_| "an attribute is not written as XML defines")?;
        if attribute.key.as_ref().len() > LONGEST_NAME {
            return Err("an attribute's name is longer than the reader takes");
        }
        if attribute.key.as_namespace_binding().is_some() {
            continue;
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
        let key = if attribute.key.prefix().is_some() {
            ":".to_owned()
        } else {
            attribute.key.as_ref().to_owned()
        };
        attributes.push((key, value));
    }
    Ok(Node {
        space,
        local,
        attributes,
        children: Vec::new(),
        text: String::new(),
    })
}

/// Why one command is left unread.
type Skip = &'static str;

/// Whether every child of `node` is one of `known`.
fn only(node: &Node, known: &[(Space, &str)]) -> Result<(), Skip> {
    for child in &node.children {
        if !known.iter().any(|(space, local)| child.is(*space, local)) {
            return Err("unknown_element");
        }
    }
    Ok(())
}

/// Whether a command's name is plain: a letter, then letters, digits, `-`,
/// `_`, or `.`, at most 128 characters.
pub(crate) fn plain_command(name: &str) -> bool {
    name.len() <= 128
        && name.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
}

/// Whether a parameter's name is plain: a letter, then letters, digits, or
/// `_`, at most 64 characters.
fn plain_parameter(name: &str) -> bool {
    name.len() <= 64
        && name.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Whether a type's name is plain: letters, digits, and `._[]`, a backtick
/// and a comma, at most 160 characters.
fn plain_type(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 160
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._[]`, ".contains(c))
}

/// What the paragraphs under a `maml:description` say, redacted and cut.
fn description(node: Option<&Node>, redact: &dyn Redact) -> Option<String> {
    let mut text = String::new();
    node?.all_text(&mut text);
    about(&text, redact)
}

/// One `command:parameter`, in a syntax item or in the parameters list.
fn parameter(node: &Node, redact: &dyn Redact) -> Result<PsParameter, Skip> {
    for (key, _) in &node.attributes {
        if !matches!(
            key.as_str(),
            "required" | "variableLength" | "globbing" | "pipelineInput" | "position" | "aliases"
        ) {
            return Err("unknown_attribute");
        }
    }
    only(
        node,
        &[
            (Space::Maml, "name"),
            (Space::Maml, "description"),
            (Space::Command, "parameterValue"),
            (Space::Command, "parameterValueGroup"),
            (Space::Dev, "type"),
            (Space::Dev, "defaultValue"),
        ],
    )?;
    let name = node
        .child(Space::Maml, "name")
        .map(|name| name.text.trim().to_owned())
        .filter(|name| plain_parameter(name))
        .ok_or("not_plain_name")?;
    let required = match node.attribute("required") {
        None => None,
        Some(text) if text.trim().eq_ignore_ascii_case("true") => Some(true),
        Some(text) if text.trim().eq_ignore_ascii_case("false") => Some(false),
        Some(_) => return Err("unknown_value"),
    };
    let position = match node.attribute("position").map(str::trim) {
        None => None,
        Some(text) if text.eq_ignore_ascii_case("named") => Some("named".to_owned()),
        Some(text)
            if !text.is_empty() && text.len() <= 3 && text.bytes().all(|b| b.is_ascii_digit()) =>
        {
            Some(text.to_owned())
        }
        Some(_) => return Err("unknown_value"),
    };
    let pipeline = match node.attribute("pipelineInput") {
        None => Vec::new(),
        Some(text) => pipeline(text).ok_or("unknown_value")?,
    };
    // The type: `dev:type`'s name where the help gives one, else the text
    // of the parameter's own value.
    let type_node = node.child(Space::Dev, "type");
    if let Some(type_node) = type_node {
        only(
            type_node,
            &[
                (Space::Maml, "name"),
                (Space::Maml, "uri"),
                (Space::Maml, "description"),
            ],
        )?;
    }
    let full = type_node
        .and_then(|node| node.child(Space::Maml, "name"))
        .map(|name| name.text.trim().to_owned())
        .filter(|name| !name.is_empty());
    let full_type = full.is_some();
    let type_name = full.or_else(|| {
        node.child(Space::Command, "parameterValue")
            .map(|value| value.text.trim().to_owned())
            .filter(|name| !name.is_empty())
    });
    if type_name.as_deref().is_some_and(|name| !plain_type(name)) {
        return Err("unknown_value");
    }
    let switch = type_name.as_deref().is_some_and(|name| {
        name.rsplit('.')
            .next()
            .is_some_and(|last| last.eq_ignore_ascii_case("SwitchParameter"))
    });
    Ok(PsParameter {
        name,
        type_name,
        switch,
        required,
        position,
        pipeline,
        about: description(node.child(Space::Maml, "description"), redact),
        full_type,
    })
}

/// One `command:command`.
fn command(node: &Node, redact: &dyn Redact) -> Result<PsCommand, Skip> {
    only(
        node,
        &[
            (Space::Command, "details"),
            (Space::Maml, "description"),
            (Space::Command, "syntax"),
            (Space::Command, "parameters"),
            (Space::Command, "inputTypes"),
            (Space::Command, "returnValues"),
            (Space::Command, "terminatingErrors"),
            (Space::Command, "nonTerminatingErrors"),
            (Space::Maml, "alertSet"),
            (Space::Command, "examples"),
            (Space::Command, "relatedLinks"),
            (Space::Maml, "relatedLinks"),
        ],
    )?;
    let details = node.child(Space::Command, "details").ok_or("no_name")?;
    only(
        details,
        &[
            (Space::Command, "name"),
            (Space::Maml, "description"),
            (Space::Command, "verb"),
            (Space::Command, "noun"),
            (Space::Maml, "copyright"),
            (Space::Dev, "version"),
            (Space::Command, "synonyms"),
        ],
    )?;
    let name = details
        .child(Space::Command, "name")
        .map(|name| name.text.trim().to_owned())
        .ok_or("no_name")?;
    if !plain_command(&name) {
        return Err("not_plain_name");
    }
    // What each parameter says, from the list of parameters.
    let mut listed: Vec<PsParameter> = Vec::new();
    if let Some(parameters) = node.child(Space::Command, "parameters") {
        only(parameters, &[(Space::Command, "parameter")])?;
        for child in &parameters.children {
            if listed.len() == MOST_PARAMETERS {
                return Err("bound");
            }
            let found = parameter(child, redact)?;
            if listed
                .iter()
                .any(|known| known.name.eq_ignore_ascii_case(&found.name))
            {
                return Err("twice");
            }
            listed.push(found);
        }
    }
    // Each syntax item is one parameter set. What the item leaves unsaid
    // of a parameter is taken from the list.
    let mut sets: Vec<Vec<PsParameter>> = Vec::new();
    if let Some(syntax) = node.child(Space::Command, "syntax") {
        only(syntax, &[(Space::Command, "syntaxItem")])?;
        for item in &syntax.children {
            if sets.len() == MOST_SETS {
                return Err("bound");
            }
            only(
                item,
                &[(Space::Maml, "name"), (Space::Command, "parameter")],
            )?;
            let mut set: Vec<PsParameter> = Vec::new();
            for child in item
                .children
                .iter()
                .filter(|child| child.is(Space::Command, "parameter"))
            {
                if set.len() == MOST_PARAMETERS {
                    return Err("bound");
                }
                let mut found = parameter(child, redact)?;
                if set
                    .iter()
                    .any(|known| known.name.eq_ignore_ascii_case(&found.name))
                {
                    return Err("twice");
                }
                if let Some(known) = listed
                    .iter()
                    .find(|known| known.name.eq_ignore_ascii_case(&found.name))
                {
                    if found.type_name.is_none() || (!found.full_type && known.full_type) {
                        found.type_name.clone_from(&known.type_name);
                        found.switch = known.switch;
                    }
                    if found.about.is_none() {
                        found.about.clone_from(&known.about);
                    }
                }
                set.push(found);
            }
            sets.push(set);
        }
    }
    if sets.is_empty() {
        sets.push(listed);
    }
    Ok(PsCommand {
        name,
        synopsis: description(details.child(Space::Maml, "description"), redact),
        sets,
    })
}

/// Reads a PowerShell XML help file. Its text was redacted whole before.
///
/// # Errors
///
/// Why the file is no help file the reader takes: it is not well-formed
/// XML, it declares a document type, it passes a bound, or its root is not
/// the help format's `helpItems`.
pub(crate) fn read(text: &str, redact: &dyn Redact) -> Result<HelpRead, NotHelp> {
    let root = tree(text)?;
    if !root.is(Space::Msh, "helpItems") {
        return Err("it is no PowerShell help file: its root element is not `helpItems`");
    }
    let mut read = HelpRead::default();
    for child in &root.children {
        if !child.is(Space::Command, "command") {
            read.not_read("not_command");
            continue;
        }
        if read.commands.len() == MOST_COMMANDS {
            read.not_read("bound");
            continue;
        }
        match command(child, redact) {
            Ok(found) => {
                if read
                    .commands
                    .iter()
                    .any(|known| known.name.eq_ignore_ascii_case(&found.name))
                {
                    read.not_read("twice");
                    continue;
                }
                read.read += 1;
                read.commands.push(found);
            }
            Err(why) => read.not_read(why),
        }
    }
    Ok(read)
}

#[cfg(test)]
mod tests {
    use a2m_core::NoSecrets;

    use super::{pipeline, read};

    const HEAD: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<helpItems schema="maml" xmlns="http://msh">
"#;

    fn command(body: &str) -> String {
        format!(
            r#"{HEAD}<command:command xmlns:maml="http://schemas.microsoft.com/maml/2004/10" xmlns:command="http://schemas.microsoft.com/maml/dev/command/2004/10" xmlns:dev="http://schemas.microsoft.com/maml/dev/2004/10">
<command:details><command:name>Get-ExampleItem</command:name><maml:description><maml:para>Gets one item.</maml:para></maml:description></command:details>
{body}
</command:command></helpItems>"#
        )
    }

    #[test]
    fn a_parameter_is_read_with_its_type_and_how_it_is_bound() {
        let text = command(
            r#"<command:parameters>
<command:parameter required="true" variableLength="false" globbing="false" pipelineInput="True (ByValue, ByPropertyName)" position="0" aliases="none">
<maml:name>Sku</maml:name><maml:description><maml:para>The item&apos;s code.</maml:para></maml:description>
<command:parameterValue required="true" variableLength="false">String</command:parameterValue>
<dev:type><maml:name>System.String</maml:name><maml:uri /></dev:type><dev:defaultValue>None</dev:defaultValue>
</command:parameter>
<command:parameter required="false" pipelineInput="False" position="named"><maml:name>Force</maml:name>
<dev:type><maml:name>System.Management.Automation.SwitchParameter</maml:name></dev:type></command:parameter>
</command:parameters>"#,
        );
        let read = read(&text, &NoSecrets).unwrap();
        assert_eq!((read.read, read.unread.len()), (1, 0));
        let found = &read.commands[0];
        assert_eq!(found.name, "Get-ExampleItem");
        assert_eq!(found.synopsis.as_deref(), Some("Gets one item."));
        // No syntax item: the list of parameters is the one set.
        let [set] = found.sets.as_slice() else {
            panic!("{found:?}")
        };
        assert_eq!(set[0].name, "Sku");
        assert_eq!(set[0].type_name.as_deref(), Some("System.String"));
        assert_eq!(set[0].required, Some(true));
        assert_eq!(set[0].position.as_deref(), Some("0"));
        assert_eq!(set[0].pipeline, ["by_value", "by_property_name"]);
        assert_eq!(set[0].about.as_deref(), Some("The item's code."));
        assert!(!set[0].switch && set[1].switch);
    }

    #[test]
    fn what_the_reader_does_not_know_leaves_a_command_unread_or_the_file_refused() {
        // An element it does not know, inside a parameter, leaves the
        // command unread; a prose section is passed over whole.
        let text = command(
            r#"<maml:description><maml:para>Prose with <maml:unknownProse/> inside.</maml:para></maml:description>
<command:parameters><command:parameter required="true"><maml:name>Sku</maml:name><command:mandatoryInSet>A</command:mandatoryInSet></command:parameter></command:parameters>"#,
        );
        let read = read(&text, &NoSecrets).unwrap();
        assert_eq!(read.read, 0);
        assert_eq!(read.unread.get("unknown_element"), Some(&1));
        let odd = command(
            r#"<command:parameters><command:parameter required="maybe"><maml:name>Sku</maml:name></command:parameter></command:parameters>"#,
        );
        assert_eq!(
            super::read(&odd, &NoSecrets)
                .unwrap()
                .unread
                .get("unknown_value"),
            Some(&1)
        );
        assert_eq!(pipeline("TRUE (ByValue)"), Some(vec!["by_value"]));
        assert_eq!(pipeline("sometimes"), None);
        for refused in [
            "<root/>",
            "<helpItems>",
            "not xml at all",
            "<helpItems/><helpItems/>",
            "<?xml version=\"1.0\" encoding=\"utf-16\"?><helpItems/>",
            "<helpItems>&custom;</helpItems>",
        ] {
            assert!(super::read(refused, &NoSecrets).is_err(), "{refused}");
        }
        let deep = format!(
            "<helpItems>{}{}</helpItems>",
            "<a>".repeat(40),
            "</a>".repeat(40)
        );
        assert!(super::read(&deep, &NoSecrets).is_err());
    }
}
