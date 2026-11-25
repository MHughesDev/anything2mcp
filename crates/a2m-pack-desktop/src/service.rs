//! The discoverer `desktop_service`: what a D-Bus service says of itself
//! (decision 0276, point 1; decision 0316). It reads three things, each by
//! a means that cannot change the service:
//!
//! - the introspection of a service that is running and that the surface
//!   names, asked with the flag that forbids the bus to start it, through
//!   the driver's `introspect` step. A service that is not running is
//!   reported as not running, and nothing is started;
//! - a `.service` file a person hands the scout, under `service_file`,
//!   read as text: the bus name it is for and the program it starts, with
//!   no argument kept. The program is never started;
//! - an interface description a person hands the scout, under
//!   `interface_file`, read as text.
//!
//! Each finding carries, for every property, the access its document
//! states, which is what the drafting rule reads (`read`, `readwrite`,
//! `write`). A finding holds names, types, and structure; never a value.

use std::sync::Arc;

use a2m_core::{
    Address, Coverage, Digest, EvidenceLevel, FailsAs, Failure, Finding, FindingKind,
    LimitationCode, ProducedBy, Source, Step, Subject, UnknownReason, Value, Values,
};
use a2m_logs::Clock;
use a2m_provider::{
    BoxFuture, DescribeForm, Describer, Description, Discoverer, DiscovererName, DiscoveryReport,
    DiscoveryScope, FindingSink, SandboxPort, StepInput, StepOutcome, SurfaceDescription,
};
use serde_json::{Value as Json, json};

use crate::codes;
use crate::dbus::{self, Interface, Object};
use crate::driver::SNAPSHOT_FORMAT;
use crate::lower::INTROSPECT;
use crate::model::SurfaceFields;
use crate::screens::{DESKTOP_SURFACE, PACK};

/// The finding kind: one service's interfaces, or one file's.
pub(crate) const SERVICE_FINDING: &str = "dbus_service";
/// The format of a `dbus_service` finding.
pub(crate) const FORMAT: &str = "1";
/// The name under which a `.service` file is handed to the scout.
pub const SERVICE_FILE: &str = "service_file";
/// The name under which an interface description is handed to the scout.
pub const INTERFACE_FILE: &str = "interface_file";

/// How long the one step of a discovery may take, in milliseconds.
const STEP_TIMEOUT_MS: u32 = 20_000;

/// The discoverer `desktop_service`.
pub(crate) struct DesktopService {
    pub(crate) name: DiscovererName,
    pub(crate) kind: FindingKind,
    pub(crate) version: String,
    pub(crate) clock: Arc<dyn Clock>,
}

fn invalid(why: &str) -> Failure {
    codes::failure(
        &codes::row::STEP_NOT_USABLE,
        format!(
            "The desktop driver's introspection is not in the form this discoverer reads: {why}."
        ),
        "Run the discovery again with the version of the desktop pack that is enabled.",
    )
}

fn unreadable(name: &str, why: &str) -> Failure {
    codes::failure(
        &a2m_core::codes::row::FINDING_INVALID,
        format!("The file handed under `{name}` is not read: {why}."),
        "Hand a file of that kind, or leave it out of the discovery.",
    )
}

/// The argument types of a member, by direction.
fn types(member: &dbus::Member, direction: &str) -> Vec<String> {
    member
        .args
        .iter()
        .filter(|arg| arg.direction.as_deref().unwrap_or(direction) == direction)
        .map(|arg| arg.ty.clone())
        .collect()
}

fn interface_json(interface: &Interface) -> Json {
    json!({
        "name": interface.name,
        "methods": interface.methods.iter().map(|method| json!({
            "name": method.name,
            "in": types(method, "in"),
            "out": types(method, "out"),
        })).collect::<Vec<_>>(),
        "signals": interface.signals.iter().map(|signal| json!({
            "name": signal.name,
            "args": signal.args.iter().map(|arg| arg.ty.clone()).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "properties": interface.properties.iter().map(|property| json!({
            "name": property.name,
            "type": property.ty,
            "access": property.access.word(),
        })).collect::<Vec<_>>(),
    })
}

/// One object, as a finding states it. The bus's own interfaces, which
/// every object answers, are counted and not listed.
fn object_json(path: Option<&str>, object: &Object) -> Json {
    let own: Vec<Json> = object
        .interfaces
        .iter()
        .filter(|interface| !interface.is_standard())
        .map(interface_json)
        .collect();
    json!({
        "path": path,
        "standard": object.interfaces.len() - own.len(),
        "interfaces": own,
    })
}

/// What the person handed, read.
struct Handed {
    bytes_digest: Digest,
    text: String,
}

fn handed(scope: &DiscoveryScope<'_>, name: &'static str) -> Result<Option<Handed>, Failure> {
    let Some(path) = scope.paths.get(name) else {
        return Ok(None);
    };
    let bytes = path.read(scope.bounds.bytes)?;
    let text =
        String::from_utf8(bytes.clone()).map_err(|_| unreadable(name, "it is not UTF-8 text"))?;
    Ok(Some(Handed {
        bytes_digest: Digest::of(&bytes),
        text: scope.redact.redact(&text),
    }))
}

impl Discoverer for DesktopService {
    fn name(&self) -> DiscovererName {
        self.name.clone()
    }

    fn suits(&self, surface: &SurfaceDescription) -> bool {
        surface.kind == DESKTOP_SURFACE
    }

    #[allow(
        clippy::too_many_lines,
        reason = "one pass: the live service, then each handed file, in a fixed order"
    )]
    fn discover<'a>(
        &'a self,
        port: &'a dyn SandboxPort,
        scope: &'a DiscoveryScope<'a>,
        out: &'a mut dyn FindingSink,
    ) -> BoxFuture<'a, Result<DiscoveryReport, Failure>> {
        Box::pin(async move {
            let surface = &scope.surface.surface;
            // Every file is read before anything is asked of the bus: a
            // file that cannot be read stops the discovery with nothing
            // sent.
            let service_file = handed(scope, SERVICE_FILE)?
                .map(|file| {
                    dbus::read_service_file(&file.text)
                        .map(|read| (file, read))
                        .map_err(|why| unreadable(SERVICE_FILE, why))
                })
                .transpose()?;
            let interface_file = handed(scope, INTERFACE_FILE)?
                .map(|file| {
                    dbus::read_object(&file.text)
                        .map(|read| (file, read))
                        .map_err(|why| unreadable(INTERFACE_FILE, why))
                })
                .transpose()?;
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
            let now_subject = |part: &str| {
                Address::new(
                    &format!("desktop://{}", surface.name.as_str()),
                    &format!("/dbus/{part}"),
                )
                .map_or(
                    Subject::Unknown {
                        reason: UnknownReason::NotSupported,
                    },
                    Subject::Address,
                )
            };
            if let Some(service) = fields.service.as_deref() {
                let step = Step {
                    kind: INTROSPECT,
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
                let running = shape
                    .get("running")
                    .and_then(Json::as_bool)
                    .ok_or_else(|| invalid("it does not say whether the service runs"))?;
                let cut = shape.get("cut").and_then(Json::as_str);
                let mut objects = Vec::new();
                let mut unread: Vec<Json> = Vec::new();
                for one in shape
                    .get("objects")
                    .and_then(Json::as_array)
                    .map(Vec::as_slice)
                    .unwrap_or_default()
                {
                    let (Some(path), Some(xml)) = (
                        one.get("path").and_then(Json::as_str),
                        one.get("xml").and_then(Json::as_str),
                    ) else {
                        return Err(invalid("an object has no path or no text"));
                    };
                    match dbus::read_object(&scope.redact.redact(xml)) {
                        Ok(object) => objects.push(object_json(Some(path), &object)),
                        Err(why) => unread.push(json!({"path": path, "why": why})),
                    }
                }
                let found = json!({
                    "format": FORMAT,
                    "from": "live",
                    "program": surface.address,
                    "service": service,
                    "running": running,
                    "cut": cut,
                    "objects": objects,
                    "unread": unread,
                });
                let mut finding = Finding::about(
                    now_subject(service),
                    self.kind.clone(),
                    scope.redact.redact(&found.to_string()),
                    Source::LiveSystem,
                    self.produced_by(),
                    self.clock.now(),
                )
                .level(EvidenceLevel::Discovered)
                .limitation(
                    LimitationCode::NotExercised,
                    "The service was asked what it publishes and no method was called: what a method does is not in it.",
                );
                if !running {
                    finding = finding.coverage(Coverage::Partial).limitation(
                        LimitationCode::NotExercised,
                        "No connection owns the service's name, so it was not asked, and nothing was started.",
                    );
                    report.coverage = Coverage::Partial;
                } else if let Some(bound) = cut {
                    finding = finding.coverage(Coverage::Partial).limitation(
                        LimitationCode::Truncated,
                        format!(
                            "The walk of the service's objects stopped at its bound on {bound}."
                        ),
                    );
                    report.coverage = Coverage::Partial;
                    report.bounds_hit.push(bound.to_owned());
                } else if !unread.is_empty() {
                    finding = finding.coverage(Coverage::Partial).limitation(
                        LimitationCode::Truncated,
                        "An object's introspection is in a form the reader does not know, and what it holds is not here.",
                    );
                    report.coverage = Coverage::Partial;
                } else {
                    finding = finding.coverage(Coverage::Complete);
                }
                out.found(finding.build()?)?;
            }
            if let Some((file, read)) = &service_file {
                let found = json!({
                    "format": FORMAT,
                    "from": SERVICE_FILE,
                    "program": surface.address,
                    "service": read.name,
                    "starts": read.program,
                    "user": read.user,
                    "systemd_service": read.systemd_service,
                    "other_keys": read.other_keys,
                });
                let finding = self.file_finding(file, "service_file", &found, scope)?;
                out.found(finding)?;
            }
            if let Some((file, read)) = &interface_file {
                let found = json!({
                    "format": FORMAT,
                    "from": INTERFACE_FILE,
                    "program": surface.address,
                    "objects": [object_json(None, read)],
                });
                let finding = self.file_finding(file, "interface_description", &found, scope)?;
                out.found(finding)?;
            }
            Ok(report)
        })
    }
}

impl DesktopService {
    fn produced_by(&self) -> ProducedBy {
        ProducedBy {
            provider: PACK.to_owned(),
            version: self.version.clone(),
            component: None,
        }
    }

    /// The finding about one handed file: at the level `inferred`, since
    /// what a file says is not what the running service does.
    fn file_finding(
        &self,
        file: &Handed,
        format: &str,
        found: &Json,
        scope: &DiscoveryScope<'_>,
    ) -> Result<Finding, Failure> {
        Finding::about(
            Subject::File {
                digest: file.bytes_digest,
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
            "A file says what a service is meant to publish, not what the running service does.",
        )
        .build()
        .map_err(Failure::from)
    }
}

/// Puts the discoverer's findings so that an agent can read them.
pub(crate) struct ServiceDescriber {
    pub(crate) kind: FindingKind,
}

impl Describer for ServiceDescriber {
    fn describe(&self, findings: &[Finding], form: DescribeForm) -> Description {
        let (mut objects, mut interfaces, mut methods, mut properties, mut reads) =
            (0_i64, 0_i64, 0_i64, 0_i64, 0_i64);
        let mut not_running = 0_i64;
        let mut untrusted = Vec::new();
        for finding in findings
            .iter()
            .filter(|finding| *finding.kind() == self.kind)
        {
            let found: Json = serde_json::from_str(finding.found()).unwrap_or(Json::Null);
            if found.get("running").and_then(Json::as_bool) == Some(false) {
                not_running += 1;
            }
            for object in found
                .get("objects")
                .and_then(Json::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default()
            {
                objects += 1;
                for interface in object
                    .get("interfaces")
                    .and_then(Json::as_array)
                    .map(Vec::as_slice)
                    .unwrap_or_default()
                {
                    interfaces += 1;
                    methods += interface
                        .get("methods")
                        .and_then(Json::as_array)
                        .map_or(0, |list| i64::try_from(list.len()).unwrap_or(i64::MAX));
                    for property in interface
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
                    if form == DescribeForm::Full
                        && let Some(name) = interface.get("name").and_then(Json::as_str)
                    {
                        untrusted.push(name.to_owned());
                    }
                }
            }
        }
        Description {
            text: format!(
                "The service publishes {objects} objects with {interfaces} interfaces of its own, {methods} methods, and {properties} properties, of which {reads} state that they can only be read; {not_running} service was not running when asked, and none was started."
            ),
            data: Values::new()
                .with("objects", objects)
                .with("interfaces", interfaces)
                .with("methods", methods)
                .with("properties", properties)
                .with("read_only_properties", reads)
                .with("not_running", not_running),
            untrusted,
        }
    }
}
