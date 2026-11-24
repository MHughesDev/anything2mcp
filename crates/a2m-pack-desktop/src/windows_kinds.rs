//! The two discoverers of what a Windows program publishes about itself
//! (decision 0276, point 1; decision 0318):
//!
//! - `desktop_management`: the definition of the management class the
//!   surface names (`management_class`), asked of the system's management
//!   service through the driver's `schema` step. On Windows only; any other
//!   system reports an unsupported system, a named stop, with nothing sent.
//! - `desktop_com`: a COM type library a person hands the scout, under
//!   `type_library`, read as bytes on every system and never loaded by the
//!   system. A form the reader does not know is a named stop.
//!
//! A finding holds names, types, and structure; never a value. For each
//! class property it says whether the definition marks it writable; for
//! each type-library member, how the library declares it to be invoked.

use std::sync::Arc;

use a2m_core::{
    Address, Coverage, Digest, EvidenceLevel, FailsAs, Failure, Finding, FindingKind,
    LimitationCode, ProducedBy, Source, Step, Subject, UnknownReason, Value, Values,
};
use a2m_provider::{
    BoxFuture, DescribeForm, Describer, Description, Discoverer, DiscovererName, DiscoveryReport,
    DiscoveryScope, FindingSink, SandboxPort, StepInput, StepOutcome, SurfaceDescription,
};
use a2m_telemetry::Clock;
use serde_json::{Value as Json, json};

use crate::codes;
use crate::driver::SNAPSHOT_FORMAT;
use crate::lower::SCHEMA;
use crate::model::SurfaceFields;
use crate::screens::{DESKTOP_SURFACE, PACK};
use crate::typelib::{self, Library, Refused};

/// The finding kind of a management class's definition.
pub(crate) const MANAGEMENT_FINDING: &str = "management_class";
/// The finding kind of a COM type library.
pub(crate) const TYPELIB_FINDING: &str = "com_type_library";
/// The format of both.
pub(crate) const FORMAT: &str = "1";
/// The name under which a COM type library is handed to the scout.
pub const TYPE_LIBRARY: &str = "type_library";
/// The management namespace asked when a surface names none.
#[cfg(windows)]
pub(crate) const DEFAULT_NAMESPACE: &str = "ROOT\\CIMV2";

/// How long the one step of a discovery may take, in milliseconds.
const STEP_TIMEOUT_MS: u32 = 20_000;

fn produced_by(version: &str) -> ProducedBy {
    ProducedBy {
        provider: PACK.to_owned(),
        version: version.to_owned(),
        component: None,
    }
}

fn invalid(why: &str) -> Failure {
    codes::failure(
        &codes::row::STEP_NOT_USABLE,
        format!("The desktop driver's schema is not in the form this discoverer reads: {why}."),
        "Run the discovery again with the version of the desktop pack that is enabled.",
    )
}

/// The CIM type numbers a definition uses, as words; `None` for one the
/// reader does not name.
pub(crate) fn cim_word(number: i64) -> &'static str {
    // The array flag is part of the number.
    match number & 0x1FFF {
        2 => "sint16",
        3 => "sint32",
        4 => "real32",
        5 => "real64",
        8 => "string",
        11 => "boolean",
        13 => "object",
        16 => "sint8",
        17 => "uint8",
        18 => "uint16",
        19 => "uint32",
        20 => "sint64",
        21 => "uint64",
        101 => "datetime",
        102 => "reference",
        103 => "char16",
        _ => "other",
    }
}

// ---- the management class ----

/// The discoverer `desktop_management`.
pub(crate) struct DesktopManagement {
    pub(crate) name: DiscovererName,
    pub(crate) kind: FindingKind,
    pub(crate) version: String,
    pub(crate) clock: Arc<dyn Clock>,
}

impl Discoverer for DesktopManagement {
    fn name(&self) -> DiscovererName {
        self.name.clone()
    }

    fn suits(&self, surface: &SurfaceDescription) -> bool {
        surface.kind == DESKTOP_SURFACE
    }

    fn discover<'a>(
        &'a self,
        port: &'a dyn SandboxPort,
        scope: &'a DiscoveryScope<'a>,
        out: &'a mut dyn FindingSink,
    ) -> BoxFuture<'a, Result<DiscoveryReport, Failure>> {
        Box::pin(async move {
            let surface = &scope.surface.surface;
            let fields = if surface.payload.is_none() {
                SurfaceFields::default()
            } else {
                surface
                    .payload
                    .decode::<SurfaceFields>()
                    .map_err(|_| invalid("the surface's fields do not decode"))?
            };
            let mut report = DiscoveryReport {
                coverage: Coverage::Complete,
                ..DiscoveryReport::default()
            };
            if fields.management_class.is_none() {
                return Ok(report);
            }
            let step = Step {
                kind: SCHEMA,
                refers_to: None,
                args: Vec::new(),
                timeout_ms: STEP_TIMEOUT_MS,
                fails_as: FailsAs::from(&codes::row::UNEXPECTED_PAGE),
                commits: false,
                lookup: false,
                idempotent: true,
                when: None,
                payload: a2m_core::PackPayload::none(),
            };
            let none = Values::new();
            let input = StepInput::new(&none, scope.deadline);
            let outcome = port
                .act(&step, &input)
                .await
                .map_err(|failed| failed.failure)?;
            let (StepOutcome::Done(observed) | StepOutcome::Absent(observed)) = outcome;
            let shape = observed
                .values
                .get("shape")
                .and_then(Value::as_text)
                .ok_or_else(|| invalid("it holds no shape"))?;
            let shape: Json = serde_json::from_str(shape).map_err(|_| invalid("not JSON"))?;
            if shape.get("format").and_then(Json::as_str) != Some(SNAPSHOT_FORMAT) {
                return Err(invalid("another format"));
            }
            let properties: Vec<Json> = shape
                .get("properties")
                .and_then(Json::as_array)
                .ok_or_else(|| invalid("no properties"))?
                .iter()
                .map(|property| {
                    let (Some(name), Some(number), Some(writable)) = (
                        property.get("name").and_then(Json::as_str),
                        property.get("cim_type").and_then(Json::as_i64),
                        property.get("writable").and_then(Json::as_bool),
                    ) else {
                        return Err(invalid("a property is not in the form"));
                    };
                    Ok(json!({
                        "name": name,
                        "type": cim_word(number),
                        "array": number & 0x2000 != 0,
                        // A definition that does not mark a property writable
                        // says it can only be read.
                        "access": if writable { "readwrite" } else { "read" },
                    }))
                })
                .collect::<Result<_, _>>()?;
            let cut = shape.get("cut").and_then(Json::as_bool).unwrap_or(false);
            let found = json!({
                "format": FORMAT,
                "from": "management_schema",
                "program": surface.address,
                "namespace": shape.get("namespace").cloned().unwrap_or(Json::Null),
                "class": shape.get("class").cloned().unwrap_or(Json::Null),
                "singleton": shape.get("singleton").and_then(Json::as_bool).unwrap_or(false),
                "properties": properties,
                "methods": shape.get("methods").cloned().unwrap_or_else(|| json!([])),
            });
            let subject = Address::new(
                &format!("desktop://{}", surface.name.as_str()),
                "/management/class",
            )
            .map_or(
                Subject::Unknown {
                    reason: UnknownReason::NotSupported,
                },
                Subject::Address,
            );
            let mut finding = Finding::about(
                subject,
                self.kind.clone(),
                scope.redact.redact(&found.to_string()),
                Source::LiveSystem,
                produced_by(&self.version),
                self.clock.now(),
            )
            .level(EvidenceLevel::Discovered)
            .limitation(
                LimitationCode::NotExercised,
                "The class's definition was read and no instance was asked and no method called: what a property holds, and what a method does, is not in it.",
            );
            if cut {
                finding = finding.coverage(Coverage::Partial).limitation(
                    LimitationCode::Truncated,
                    "The enumeration of the class's properties or methods stopped at its bound.",
                );
                report.coverage = Coverage::Partial;
                report.bounds_hit.push("objects".to_owned());
            } else {
                finding = finding.coverage(Coverage::Complete);
            }
            out.found(finding.build()?)?;
            Ok(report)
        })
    }
}

// ---- the type library ----

fn type_json(ty: &typelib::Type) -> Json {
    json!({"vt": ty.vt, "type": ty.word})
}

fn library_json(library: &Library, program: &str) -> Json {
    json!({
        "format": FORMAT,
        "from": "type_library",
        "program": program,
        "library": {
            "name": library.name,
            "guid": library.guid,
            "version": format!("{}.{}", library.version.0, library.version.1),
        },
        "interfaces": library.interfaces.iter().map(|interface| json!({
            "name": interface.name,
            "guid": interface.guid,
            "kind": interface.kind,
            "members": interface.members.iter().map(|member| json!({
                "name": member.name,
                "memid": member.memid,
                "invoke": member.invoke.word(),
                "returns": type_json(&member.returns),
                "parameters": member.parameters.iter().map(|parameter| json!({
                    "name": parameter.name,
                    "type": type_json(&parameter.ty),
                    "in": parameter.input,
                    "out": parameter.output,
                    "optional": parameter.optional,
                    "retval": parameter.retval,
                })).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "coclasses": library.coclasses,
        "passed_over": library.passed_over,
    })
}

/// The discoverer `desktop_com`.
pub(crate) struct DesktopCom {
    pub(crate) name: DiscovererName,
    pub(crate) kind: FindingKind,
    pub(crate) version: String,
    pub(crate) clock: Arc<dyn Clock>,
}

impl Discoverer for DesktopCom {
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
            let Some(path) = scope.paths.get(TYPE_LIBRARY) else {
                return Ok(report);
            };
            let bytes = path.read(scope.bounds.bytes)?;
            let library = typelib::read(&bytes).map_err(|refused| match refused {
                Refused::UnknownFormat(why) => codes::failure(
                    &codes::row::TYPE_LIBRARY_FORMAT_UNKNOWN,
                    format!("The file handed under `{TYPE_LIBRARY}` is not read: {why}."),
                    "Hand a type library in the MSFT form, or leave it out of the discovery.",
                ),
                Refused::Malformed(why) => codes::failure(
                    &a2m_core::codes::row::FINDING_INVALID,
                    format!(
                        "The file handed under `{TYPE_LIBRARY}` is not read: it is a type library that does not hold together ({}).",
                        codes::shown(&why)
                    ),
                    "Hand a type library as the system keeps it, or leave it out of the discovery.",
                ),
            })?;
            let program = scope.surface.surface.address.as_str();
            let found = library_json(&library, program);
            let mut finding = Finding::about(
                Subject::File {
                    digest: Digest::of(&bytes),
                    format: "type_library".to_owned(),
                    part: None,
                },
                self.kind.clone(),
                scope.redact.redact(&found.to_string()),
                Source::Artifact,
                produced_by(&self.version),
                self.clock.now(),
            )
            .level(EvidenceLevel::Inferred)
            .limitation(
                LimitationCode::NotExercised,
                "A type library says what a component is meant to offer, not what the running component does. It was read as bytes and no part of it was loaded or run.",
            );
            if library.cut {
                finding = finding.coverage(Coverage::Partial).limitation(
                    LimitationCode::Truncated,
                    "The reading of the library stopped at a bound.",
                );
                report.coverage = Coverage::Partial;
                report.bounds_hit.push("objects".to_owned());
            } else {
                finding = finding.coverage(Coverage::Complete);
            }
            out.found(finding.build()?)?;
            Ok(report)
        })
    }
}

/// Puts the findings of both discoverers so that an agent can read them.
pub(crate) struct WindowsDescriber {
    pub(crate) kind: FindingKind,
}

impl Describer for WindowsDescriber {
    fn describe(&self, findings: &[Finding], form: DescribeForm) -> Description {
        let (mut properties, mut reads, mut members, mut gets, mut interfaces) =
            (0_i64, 0_i64, 0_i64, 0_i64, 0_i64);
        let mut untrusted = Vec::new();
        for finding in findings
            .iter()
            .filter(|finding| *finding.kind() == self.kind)
        {
            let found: Json = serde_json::from_str(finding.found()).unwrap_or(Json::Null);
            for property in found
                .get("properties")
                .and_then(Json::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default()
            {
                properties += 1;
                if property.get("access").and_then(Json::as_str) == Some("read") {
                    reads += 1;
                }
            }
            for interface in found
                .get("interfaces")
                .and_then(Json::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default()
            {
                interfaces += 1;
                if form == DescribeForm::Full
                    && let Some(name) = interface.get("name").and_then(Json::as_str)
                {
                    untrusted.push(name.to_owned());
                }
                for member in interface
                    .get("members")
                    .and_then(Json::as_array)
                    .map(Vec::as_slice)
                    .unwrap_or_default()
                {
                    members += 1;
                    if member.get("invoke").and_then(Json::as_str) == Some("property_get") {
                        gets += 1;
                    }
                }
            }
        }
        Description {
            text: format!(
                "The definitions give {properties} class properties, of which {reads} are not marked writable; and {interfaces} interfaces with {members} members, of which {gets} are property gets. Nothing was loaded or run, and no instance was asked."
            ),
            data: Values::new()
                .with("properties", properties)
                .with("not_writable", reads)
                .with("interfaces", interfaces)
                .with("members", members)
                .with("property_gets", gets),
            untrusted,
        }
    }
}
