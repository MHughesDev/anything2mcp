//! The discoverer `desktop_bundle`: what a macOS application says of itself
//! in its own files (decision 0276, point 1; decision 0317). A person hands
//! the scout the application's bundle folder, under `bundle`, from the
//! project's `sources` folder; the discoverer reads two files of it, as
//! bytes, on every system, and sends nothing to any application:
//!
//! - `Contents/Info.plist`, in its XML or binary form: the application's
//!   identity, the URL schemes it takes, the document types it opens, the
//!   services it offers, and the name of its scripting dictionary;
//! - the scripting dictionary that property list names, in
//!   `Contents/Resources`: its suites, commands, events, classes, and each
//!   class property with the access the dictionary states.
//!
//! A dictionary that includes a file outside the bundle is refused by name
//! before any of it is taken. A finding holds names, types, and structure;
//! never a value the application holds.

use std::sync::Arc;

use a2m_core::{
    Coverage, Digest, EvidenceLevel, Failure, Finding, FindingKind, LimitationCode, ProducedBy,
    Source, Subject, Values,
};
use a2m_provider::{
    BoxFuture, DescribeForm, Describer, Description, Discoverer, DiscovererName, DiscoveryReport,
    DiscoveryScope, FindingSink, SandboxPort, SurfaceDescription,
};
use a2m_telemetry::Clock;
use serde_json::{Value as Json, json};

use crate::codes;
use crate::plist::{self, Plist};
use crate::screens::{DESKTOP_SURFACE, PACK};
use crate::sdef::{self, Dictionary, Refused};

/// The finding kind: one file of a bundle, read.
pub(crate) const BUNDLE_FINDING: &str = "application_bundle";
/// The format of an `application_bundle` finding.
pub(crate) const FORMAT: &str = "1";
/// The name under which an application's bundle folder is handed to the
/// scout.
pub const BUNDLE: &str = "bundle";

/// The property list of a bundle, relative to its folder.
const INFO_PLIST: &str = "Contents/Info.plist";
/// Where a bundle keeps its scripting dictionary.
const RESOURCES: &str = "Contents/Resources";
/// The longest name of the dictionary file a property list may give.
const LONGEST_FILE_NAME: usize = 100;
/// The longest of an application's own words a finding holds.
const LONGEST_WORDS: usize = 200;
/// The most entries of one list a finding holds.
const MOST: usize = 500;

/// The discoverer `desktop_bundle`.
pub(crate) struct DesktopBundle {
    pub(crate) name: DiscovererName,
    pub(crate) kind: FindingKind,
    pub(crate) version: String,
    pub(crate) clock: Arc<dyn Clock>,
}

fn unreadable(what: &str, why: &str) -> Failure {
    codes::failure(
        &a2m_core::codes::row::FINDING_INVALID,
        format!("{} is not read: {why}.", codes::shown(what)),
        "Hand the application's bundle folder, as the system keeps it, or leave it out of the discovery.",
    )
}

/// An application's own word, cut.
fn words(value: Option<&Plist>) -> Json {
    value.and_then(Plist::text).map_or(Json::Null, |text| {
        json!(text.chars().take(LONGEST_WORDS).collect::<String>())
    })
}

fn texts(value: Option<&Plist>) -> Vec<Json> {
    value
        .map(Plist::list)
        .unwrap_or_default()
        .iter()
        .filter_map(Plist::text)
        .take(MOST)
        .map(|text| json!(text.chars().take(LONGEST_WORDS).collect::<String>()))
        .collect()
}

fn dicts(value: Option<&Plist>) -> impl Iterator<Item = &Plist> {
    value
        .map(Plist::list)
        .unwrap_or_default()
        .iter()
        .filter(|one| matches!(one, Plist::Dict(_)))
        .take(MOST)
}

/// What a property list says of an application.
fn info_json(info: &Plist, program: &str) -> Json {
    let url_schemes: Vec<Json> = dicts(info.get("CFBundleURLTypes"))
        .map(|one| {
            json!({
                "name": words(one.get("CFBundleURLName")),
                "role": words(one.get("CFBundleTypeRole")),
                "schemes": texts(one.get("CFBundleURLSchemes")),
            })
        })
        .collect();
    let document_types: Vec<Json> = dicts(info.get("CFBundleDocumentTypes"))
        .map(|one| {
            json!({
                "name": words(one.get("CFBundleTypeName")),
                "role": words(one.get("CFBundleTypeRole")),
                "extensions": texts(one.get("CFBundleTypeExtensions")),
                "content_types": texts(one.get("LSItemContentTypes")),
            })
        })
        .collect();
    let services: Vec<Json> = dicts(info.get("NSServices"))
        .map(|one| {
            json!({
                "menu_item": words(one.get("NSMenuItem").and_then(|item| item.get("default"))),
                "message": words(one.get("NSMessage")),
                "port": words(one.get("NSPortName")),
                "send_types": texts(one.get("NSSendTypes")),
                "return_types": texts(one.get("NSReturnTypes")),
            })
        })
        .collect();
    json!({
        "format": FORMAT,
        "from": "info_plist",
        "program": program,
        "bundle_id": words(info.get("CFBundleIdentifier")),
        "name": words(info.get("CFBundleName")),
        "executable": words(info.get("CFBundleExecutable")),
        "scripting_definition": words(info.get("OSAScriptingDefinition")),
        "scriptable": info.get("NSAppleScriptEnabled").and_then(Plist::flag),
        "url_schemes": url_schemes,
        "document_types": document_types,
        "services": services,
    })
}

fn dictionary_json(dictionary: &Dictionary, file: &str, program: &str) -> Json {
    let suites: Vec<Json> = dictionary
        .suites
        .iter()
        .map(|suite| {
            let command = |command: &sdef::Command| {
                json!({
                    "name": command.name,
                    "code": command.code,
                    "direct": command.direct,
                    "parameters": command.parameters.iter().map(|parameter| json!({
                        "name": parameter.name,
                        "type": parameter.ty,
                        "optional": parameter.optional,
                    })).collect::<Vec<_>>(),
                    "result": command.result,
                })
            };
            json!({
                "name": suite.name,
                "code": suite.code,
                "commands": suite.commands.iter().map(command).collect::<Vec<_>>(),
                "events": suite.events.iter().map(command).collect::<Vec<_>>(),
                "classes": suite.classes.iter().map(|class| json!({
                    "name": class.name,
                    "code": class.code,
                    "inherits": class.inherits,
                    "elements": class.elements,
                    "properties": class.properties.iter().map(|property| json!({
                        "name": property.name,
                        "code": property.code,
                        "type": property.ty,
                        "access": property.access.word(),
                        "access_stated": property.stated,
                    })).collect::<Vec<_>>(),
                })).collect::<Vec<_>>(),
                "enumerations": suite.enumerations.iter().map(|enumeration| json!({
                    "name": enumeration.name,
                    "values": enumeration.values,
                })).collect::<Vec<_>>(),
            })
        })
        .collect();
    json!({
        "format": FORMAT,
        "from": "scripting_dictionary",
        "program": program,
        "file": file,
        "suites": suites,
        "includes": dictionary.includes.iter().map(|include| include.file.clone()).collect::<Vec<_>>(),
        "passed_over": dictionary.passed_over,
    })
}

/// A dictionary's file name as a property list gives it, made the name of a
/// file in the bundle's `Resources`; `None` for a name that is not a plain
/// one.
fn dictionary_file(name: &str) -> Option<String> {
    let plain = !name.is_empty()
        && name.len() <= LONGEST_FILE_NAME
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '_' | '-' | '.'));
    plain.then(|| {
        if name.contains('.') {
            name.to_owned()
        } else {
            format!("{name}.sdef")
        }
    })
}

impl DesktopBundle {
    fn produced_by(&self) -> ProducedBy {
        ProducedBy {
            provider: PACK.to_owned(),
            version: self.version.clone(),
            component: None,
        }
    }

    fn finding(
        &self,
        digest: Digest,
        format: &str,
        found: &Json,
        scope: &DiscoveryScope<'_>,
    ) -> Result<Finding, Failure> {
        Finding::about(
            Subject::File {
                digest,
                format: format.to_owned(),
                part: None,
            },
            self.kind.clone(),
            scope.redact.redact(&found.to_string()),
            Source::Artifact,
            self.produced_by(),
            self.clock.now(),
        )
        .level(EvidenceLevel::Inferred)
        .coverage(Coverage::Complete)
        .limitation(
            LimitationCode::NotExercised,
            "A file of the bundle says what the application is meant to offer, not what the running application does.",
        )
        .build()
        .map_err(Failure::from)
    }
}

impl Discoverer for DesktopBundle {
    fn name(&self) -> DiscovererName {
        self.name.clone()
    }

    fn suits(&self, surface: &SurfaceDescription) -> bool {
        surface.kind == DESKTOP_SURFACE
    }

    fn discover<'a>(
        &'a self,
        _port: &'a dyn SandboxPort,
        scope: &'a DiscoveryScope<'a>,
        out: &'a mut dyn FindingSink,
    ) -> BoxFuture<'a, Result<DiscoveryReport, Failure>> {
        Box::pin(async move {
            let mut report = DiscoveryReport {
                coverage: Coverage::Complete,
                ..DiscoveryReport::default()
            };
            let Some(bundle) = scope.paths.get(BUNDLE) else {
                return Ok(report);
            };
            let program = scope.surface.surface.address.as_str();
            let limit = scope.bounds.bytes;
            // Every file is read and checked before any finding is made: a
            // bundle that cannot be read stops the discovery with nothing
            // stored.
            let info_path = bundle
                .join(INFO_PLIST)
                .map_err(|_| unreadable(BUNDLE, "it holds no `Contents/Info.plist`"))?;
            let info_bytes = info_path.read(limit).map_err(|_| {
                unreadable(INFO_PLIST, "it cannot be read, or is larger than the bound")
            })?;
            let info = plist::read(&info_bytes).map_err(|why| unreadable(INFO_PLIST, why))?;
            if !matches!(info, Plist::Dict(_)) {
                return Err(unreadable(INFO_PLIST, "it is not a dictionary of keys"));
            }
            let dictionary = match info.get("OSAScriptingDefinition").and_then(Plist::text) {
                None => None,
                Some(named) => {
                    let file = dictionary_file(named).ok_or_else(|| {
                        unreadable(
                            INFO_PLIST,
                            "the dictionary it names is not a plain file name of the bundle's Resources",
                        )
                    })?;
                    let relative = format!("{RESOURCES}/{file}");
                    let bytes = bundle
                        .join(&relative)
                        .and_then(|path| path.read(limit))
                        .map_err(|_| {
                            unreadable(
                                &relative,
                                "the property list names it and it cannot be read",
                            )
                        })?;
                    let text = String::from_utf8(bytes.clone())
                        .map_err(|_| unreadable(&relative, "it is not UTF-8 text"))?;
                    let read = sdef::read(&scope.redact.redact(&text), RESOURCES).map_err(
                        |refused| match refused {
                            Refused::NotRead(why) => unreadable(&relative, why),
                            Refused::IncludeOutsideBundle(href) => codes::failure(
                                &codes::row::DICTIONARY_INCLUDE_REFUSED,
                                format!(
                                    "The scripting dictionary {} includes `{}`, which is outside the application's bundle. The pack reads the files of the bundle only.",
                                    codes::shown(&relative),
                                    codes::shown(&href)
                                ),
                                "Hand a dictionary that includes nothing outside its bundle, or leave it out of the discovery.",
                            ),
                        },
                    )?;
                    Some((relative, Digest::of(&bytes), read))
                }
            };
            out.found(self.finding(
                Digest::of(&info_bytes),
                "info_plist",
                &info_json(&info, program),
                scope,
            )?)?;
            if let Some((relative, digest, read)) = &dictionary {
                out.found(self.finding(
                    *digest,
                    "scripting_dictionary",
                    &dictionary_json(read, relative, program),
                    scope,
                )?)?;
                if !read.includes.is_empty() || read.passed_over > 0 {
                    report.coverage = Coverage::Partial;
                }
            }
            Ok(report)
        })
    }
}

/// Puts the discoverer's findings so that an agent can read them.
pub(crate) struct BundleDescriber {
    pub(crate) kind: FindingKind,
}

impl Describer for BundleDescriber {
    fn describe(&self, findings: &[Finding], form: DescribeForm) -> Description {
        let (mut schemes, mut documents, mut services, mut commands, mut properties, mut reads) =
            (0_i64, 0_i64, 0_i64, 0_i64, 0_i64, 0_i64);
        let mut untrusted = Vec::new();
        let count = |value: Option<&Json>| {
            value
                .and_then(Json::as_array)
                .map_or(0, |list| i64::try_from(list.len()).unwrap_or(i64::MAX))
        };
        for finding in findings
            .iter()
            .filter(|finding| *finding.kind() == self.kind)
        {
            let found: Json = serde_json::from_str(finding.found()).unwrap_or(Json::Null);
            for one in found
                .get("url_schemes")
                .and_then(Json::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default()
            {
                schemes += count(one.get("schemes"));
                if form == DescribeForm::Full {
                    untrusted.extend(
                        one.get("schemes")
                            .and_then(Json::as_array)
                            .map(Vec::as_slice)
                            .unwrap_or_default()
                            .iter()
                            .filter_map(Json::as_str)
                            .map(str::to_owned),
                    );
                }
            }
            documents += count(found.get("document_types"));
            services += count(found.get("services"));
            for suite in found
                .get("suites")
                .and_then(Json::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default()
            {
                commands += count(suite.get("commands")) + count(suite.get("events"));
                for class in suite
                    .get("classes")
                    .and_then(Json::as_array)
                    .map(Vec::as_slice)
                    .unwrap_or_default()
                {
                    for property in class
                        .get("properties")
                        .and_then(Json::as_array)
                        .map(Vec::as_slice)
                        .unwrap_or_default()
                    {
                        properties += 1;
                        if property.get("access").and_then(Json::as_str) == Some("read")
                            && property.get("access_stated").and_then(Json::as_bool) == Some(true)
                        {
                            reads += 1;
                        }
                    }
                }
            }
        }
        Description {
            text: format!(
                "The bundle takes {schemes} URL schemes, opens {documents} kinds of document, and offers {services} services; its scripting dictionary has {commands} commands and events and {properties} class properties, of which {reads} state that they can only be read. Nothing was sent to the application."
            ),
            data: Values::new()
                .with("url_schemes", schemes)
                .with("document_types", documents)
                .with("services", services)
                .with("commands", commands)
                .with("properties", properties)
                .with("read_only_properties", reads),
            untrusted,
        }
    }
}
