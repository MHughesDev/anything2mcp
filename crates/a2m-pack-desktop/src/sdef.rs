//! A reader of an application's scripting dictionary, the `.sdef` file its
//! own property list names (decision 0317), read as a file on every
//! system: no Apple event is sent and no application is asked.
//!
//! What a dictionary says is its suites, and in each its commands and
//! events with their parameters, its classes with their properties, and
//! its enumerations. For each class property it carries the access the
//! dictionary states (`r`, `w`, `rw`), and whether it states one at all: a
//! property that states none is read-write by the format's own default,
//! which is not evidence that it only reads.
//!
//! The format lets a dictionary include a part of another file
//! (`xi:include`), most often the system's standard suite. Reading that
//! file is reading outside the application's bundle, which this reader
//! never does: a dictionary that includes a file outside the bundle is
//! refused, by a name that says which include (`dictionary_include_refused`).
//! An include that stays inside the bundle is listed and not followed.

use crate::xml::{self, Bounds, Element, NotRead};

/// How far a dictionary may go.
const BOUNDS: Bounds = Bounds {
    depth: 24,
    elements: 100_000,
};
/// The most suites, and the most of each kind of thing in one suite.
const MOST: usize = 5_000;
/// The longest an application's own words are kept, in characters.
const LONGEST_WORDS: usize = 200;

/// The one external document type a dictionary names. It is a name, never
/// fetched.
const DOCTYPE: &str = "dictionary SYSTEM \"file://localhost/System/Library/DTDs/sdef.dtd\"";

/// What a property states of who may use it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Access {
    Read,
    Write,
    ReadWrite,
}

impl Access {
    pub(crate) fn word(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
            Self::ReadWrite => "readwrite",
        }
    }
}

/// One parameter of a command or an event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Parameter {
    pub name: String,
    pub ty: Option<String>,
    pub optional: bool,
}

/// One command or event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Command {
    pub name: String,
    pub code: String,
    /// The type of its direct parameter, when it has one.
    pub direct: Option<String>,
    pub parameters: Vec<Parameter>,
    pub result: Option<String>,
}

/// One property of a class.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Property {
    pub name: String,
    pub code: String,
    pub ty: Option<String>,
    pub access: Access,
    /// Whether the dictionary states the access. When it does not, the
    /// format's default, read-write, is what `access` holds.
    pub stated: bool,
}

/// One class.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Class {
    pub name: String,
    pub code: String,
    pub inherits: Option<String>,
    pub properties: Vec<Property>,
    /// The classes of the things it holds.
    pub elements: Vec<String>,
}

/// One enumeration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Enumeration {
    pub name: String,
    pub values: Vec<String>,
}

/// One suite.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Suite {
    pub name: String,
    pub code: String,
    pub commands: Vec<Command>,
    pub events: Vec<Command>,
    pub classes: Vec<Class>,
    pub enumerations: Vec<Enumeration>,
}

/// An include of part of another file that stays inside the bundle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Include {
    /// The file it names, relative to the bundle's folder.
    pub file: String,
}

/// A dictionary, read.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Dictionary {
    pub suites: Vec<Suite>,
    /// The includes that stay inside the bundle, listed and not followed.
    pub includes: Vec<Include>,
    /// How many elements of a kind the reader does not know it passed over.
    pub passed_over: u64,
}

/// Why a dictionary is not taken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Refused {
    /// It is not a dictionary of the format.
    NotRead(NotRead),
    /// It includes a file outside the bundle. The include, as written,
    /// cut.
    IncludeOutsideBundle(String),
}

impl From<NotRead> for Refused {
    fn from(why: NotRead) -> Self {
        Self::NotRead(why)
    }
}

/// Where an include points, relative to the bundle.
fn inside(href: &str, folder: &str) -> Option<String> {
    // Anything that is not a plain relative path inside the bundle is
    // outside it: another scheme, an absolute path, an encoded name, a
    // backslash, a way up past the bundle's top.
    let href = href.strip_prefix("file://localhost").unwrap_or(href);
    if href.contains(':')
        || href.starts_with('/')
        || href.contains('%')
        || href.contains('\\')
        || href.chars().any(char::is_control)
    {
        return None;
    }
    let mut parts: Vec<&str> = folder.split('/').filter(|part| !part.is_empty()).collect();
    for part in href.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            other => parts.push(other),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

fn words(value: Option<&str>) -> Option<String> {
    value.map(|text| {
        text.chars()
            .filter(|c| !c.is_control())
            .take(LONGEST_WORDS)
            .collect()
    })
}

fn named(of: &Element, what: &'static str) -> Result<String, Refused> {
    words(of.attribute("name"))
        .filter(|name| !name.is_empty())
        .ok_or(Refused::NotRead(what))
}

/// The type a thing states: its `type` attribute, or the first `type`
/// element's.
fn type_of(of: &Element) -> Option<String> {
    words(of.attribute("type")).or_else(|| {
        of.children
            .iter()
            .find(|child| child.local == "type")
            .and_then(|child| words(child.attribute("type")))
    })
}

fn command(of: &Element) -> Result<Command, Refused> {
    let mut out = Command {
        name: named(of, "a command or an event has no name")?,
        code: words(of.attribute("code")).unwrap_or_default(),
        direct: None,
        parameters: Vec::new(),
        result: None,
    };
    for child in &of.children {
        match child.local.as_str() {
            "direct-parameter" => out.direct = type_of(child),
            "parameter" => out.parameters.push(Parameter {
                name: named(child, "a parameter has no name")?,
                ty: type_of(child),
                optional: child.attribute("optional") == Some("yes"),
            }),
            "result" => out.result = type_of(child),
            _ => {}
        }
    }
    if out.parameters.len() > MOST {
        return Err(Refused::NotRead("it lists more than the reader takes"));
    }
    Ok(out)
}

fn class(of: &Element) -> Result<Class, Refused> {
    class_of(of, named(of, "a class has no name")?)
}

fn class_of(of: &Element, name: String) -> Result<Class, Refused> {
    let mut out = Class {
        name,
        code: words(of.attribute("code")).unwrap_or_default(),
        inherits: words(of.attribute("inherits")),
        properties: Vec::new(),
        elements: Vec::new(),
    };
    for child in &of.children {
        match child.local.as_str() {
            "property" => {
                let stated = child.attribute("access");
                let access = match stated {
                    Some("r") => Access::Read,
                    Some("w") => Access::Write,
                    Some("rw") | None => Access::ReadWrite,
                    Some(_) => {
                        return Err(Refused::NotRead(
                            "a property states an access the reader does not know",
                        ));
                    }
                };
                out.properties.push(Property {
                    name: named(child, "a property has no name")?,
                    code: words(child.attribute("code")).unwrap_or_default(),
                    ty: type_of(child),
                    access,
                    stated: stated.is_some(),
                });
            }
            "element" => out.elements.extend(words(child.attribute("type"))),
            _ => {}
        }
    }
    if out.properties.len() > MOST {
        return Err(Refused::NotRead("it lists more than the reader takes"));
    }
    Ok(out)
}

fn includes(of: &Element, folder: &str, out: &mut Dictionary) -> Result<(), Refused> {
    for child in &of.children {
        if child.local == "include" {
            let href = child.attribute("href").unwrap_or_default();
            match inside(href, folder) {
                Some(file) => out.includes.push(Include { file }),
                None => {
                    return Err(Refused::IncludeOutsideBundle(
                        href.chars().take(LONGEST_WORDS).collect(),
                    ));
                }
            }
        } else {
            includes(child, folder, out)?;
        }
    }
    Ok(())
}

/// Reads a scripting dictionary. `folder` is where the file is, relative to
/// the bundle's folder, so that an include is judged against the bundle.
///
/// # Errors
///
/// [`Refused`], by name, when the text is not a dictionary of the format
/// or includes a file outside the bundle.
pub(crate) fn read(text: &str, folder: &str) -> Result<Dictionary, Refused> {
    let root = xml::parse(text, BOUNDS, &|said| said.trim() == DOCTYPE)?;
    if root.local != "dictionary" {
        return Err(Refused::NotRead("its root is not a `dictionary`"));
    }
    let mut out = Dictionary::default();
    // The includes first: a dictionary that reads outside the bundle is
    // refused whole, before any of it is taken.
    includes(&root, folder, &mut out)?;
    for suite_element in root.children.iter().filter(|child| child.local == "suite") {
        let mut suite = Suite {
            name: named(suite_element, "a suite has no name")?,
            code: words(suite_element.attribute("code")).unwrap_or_default(),
            ..Suite::default()
        };
        for child in &suite_element.children {
            match child.local.as_str() {
                "command" => suite.commands.push(command(child)?),
                "event" => suite.events.push(command(child)?),
                "class" => suite.classes.push(class(child)?),
                "class-extension" => {
                    let extends = words(child.attribute("extends"))
                        .filter(|name| !name.is_empty())
                        .ok_or(Refused::NotRead("a class extension names no class"))?;
                    suite.classes.push(class_of(child, extends)?);
                }
                "enumeration" => suite.enumerations.push(Enumeration {
                    name: named(child, "an enumeration has no name")?,
                    values: child
                        .children
                        .iter()
                        .filter(|value| value.local == "enumerator")
                        .filter_map(|value| words(value.attribute("name")))
                        .collect(),
                }),
                "include" | "documentation" | "cocoa" | "synonym" | "access-group"
                | "value-type" | "record-type" => {}
                _ => out.passed_over += 1,
            }
        }
        if out.suites.len() >= MOST {
            return Err(Refused::NotRead(
                "it holds more suites than the reader takes",
            ));
        }
        out.suites.push(suite);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEAD: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE dictionary SYSTEM "file://localhost/System/Library/DTDs/sdef.dtd">
<dictionary xmlns:xi="http://www.w3.org/2003/XInclude" title="Example">"#;

    fn dictionary(body: &str) -> String {
        format!("{HEAD}{body}</dictionary>")
    }

    #[test]
    fn a_dictionary_gives_its_suites_and_each_propertys_stated_access() {
        let text = dictionary(
            r#"<suite name="Example Suite" code="Exmp">
                 <command name="open order" code="ExmpOpen"><direct-parameter type="text"/>
                   <parameter name="with" code="with" type="text" optional="yes"/><result type="boolean"/></command>
                 <event name="order changed" code="ExmpChgd"/>
                 <class name="order" code="ordr" inherits="item">
                   <property name="number" code="numb" type="text" access="r"/>
                   <property name="status" code="stat"><type type="text"/></property>
                   <property name="note" code="note" type="text" access="rw"/>
                   <element type="line"/>
                 </class>
                 <enumeration name="state" code="Stte"><enumerator name="open" code="open"/></enumeration>
                 <surprise/>
               </suite>"#,
        );
        let read = read(&text, "Contents/Resources").unwrap();
        let suite = &read.suites[0];
        assert_eq!(suite.name, "Example Suite");
        assert_eq!(suite.commands[0].parameters[0].name, "with");
        assert!(suite.commands[0].parameters[0].optional);
        assert_eq!(suite.commands[0].direct.as_deref(), Some("text"));
        assert_eq!(suite.events[0].name, "order changed");
        let order = &suite.classes[0];
        let access: Vec<_> = order
            .properties
            .iter()
            .map(|p| (p.name.as_str(), p.access, p.stated))
            .collect();
        assert_eq!(
            access,
            [
                ("number", Access::Read, true),
                ("status", Access::ReadWrite, false),
                ("note", Access::ReadWrite, true),
            ]
        );
        assert_eq!(order.properties[1].ty.as_deref(), Some("text"));
        assert_eq!(order.elements, ["line"]);
        assert_eq!(suite.enumerations[0].values, ["open"]);
        assert_eq!(read.passed_over, 1);
    }

    #[test]
    fn an_include_outside_the_bundle_is_refused_by_name_and_one_inside_is_listed() {
        for href in [
            "file://localhost/System/Library/ScriptingDefinitions/CocoaStandard.sdef",
            "file:///System/Library/ScriptingDefinitions/CocoaStandard.sdef",
            "/etc/x.sdef",
            "http://example.invalid/x.sdef",
            "../../../x.sdef",
            "../../../../x.sdef",
            "%2e%2e/x.sdef",
            "a\\b.sdef",
        ] {
            let text = dictionary(&format!(r#"<xi:include href="{href}"/>"#));
            assert!(
                matches!(
                    read(&text, "Contents/Resources"),
                    Err(Refused::IncludeOutsideBundle(_))
                ),
                "{href}"
            );
        }
        let text = dictionary(
            r#"<suite name="a" code="aaaa"><xi:include href="Standard.sdef"/></suite><xi:include href="../Other/Part.sdef"/>"#,
        );
        let read = read(&text, "Contents/Resources").unwrap();
        let files: Vec<_> = read.includes.iter().map(|i| i.file.as_str()).collect();
        assert_eq!(
            files,
            [
                "Contents/Resources/Standard.sdef",
                "Contents/Other/Part.sdef"
            ]
        );
    }

    #[test]
    fn a_file_that_is_not_a_dictionary_is_refused_by_name() {
        for text in [
            "not xml",
            "<other/>",
            &dictionary(r#"<suite code="x"/>"#),
            &dictionary(
                r#"<suite name="a" code="b"><class name="c" code="d"><property name="p" code="e" access="x"/></class></suite>"#,
            ),
            "<!DOCTYPE dictionary [<!ENTITY a 'b'>]><dictionary/>",
            "<!DOCTYPE dictionary SYSTEM \"http://example.invalid/x.dtd\"><dictionary/>",
        ] {
            assert!(
                matches!(read(text, "Contents/Resources"), Err(Refused::NotRead(_))),
                "{text}"
            );
        }
    }
}
