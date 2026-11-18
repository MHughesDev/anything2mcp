//! Discovery: what an HTTP surface says about itself, and what a published
//! description claims.
//!
//! **Discovery cannot write.** A discoverer here reaches the system only
//! through a [`ReadClient`], which can build nothing but a `probe` step: a
//! GET, HEAD, or OPTIONS request to a literal path of the surface, with no
//! body and no credential. The step is not a commit point, so it passes
//! the sandbox port, and the driver refuses a `probe` of any other method.
//! A step that names an endpoint which writes is refused by the driver
//! unless it is a commit point, which the port never lets through.

use std::sync::Arc;

use a2m_core::{
    Address, Coverage, EvidenceLevel, FailsAs, Failure, FailureClass, Finding, FindingKind,
    LimitationCode, PackPayload, ProducedBy, Redact, Source, Step, Subject, Value, Values,
};
use a2m_provider::{
    BoxFuture, DescribeForm, Describer, Description, Discoverer, DiscovererName, DiscoveryReport,
    DiscoveryScope, FindingSink, ProviderName, SandboxPort, StepInput, StepOutcome,
    SurfaceDescription,
};
use a2m_telemetry::Clock;

use crate::codes;
use crate::driver::Origin;
use crate::endpoints::HTTP_SURFACE;
use crate::json::Json;
use crate::lower::PROBE;
use crate::openapi::{self, DescribedOperation};
use crate::template::{Method, ProbeStep};

/// The name, among a discovery's checked paths, of a description given as
/// a file: the name the manifest's `http_description` declares under
/// `files`, so the engine hands it a file of the project's `sources`
/// folder under it (decision 0263).
pub const DESCRIPTION_FILE: &str = "description";
/// The name, in a discovery's focus, of the path a description is fetched
/// from on the surface.
pub const DESCRIPTION_PATH: &str = "description_path";
/// Where a description is looked for when the focus names no path, in
/// order: the places the format's own tooling serves one by convention.
/// The first that answers with a success and a body is read; one GET each,
/// and no other place is tried.
const DEFAULT_DESCRIPTION_PATHS: &[&str] = &["/openapi.json", "/api/openapi.json"];

/// The version of the shape of an `endpoint` finding's `found`.
const ENDPOINT_FOUND_FORMAT: &str = "1";

/// The only client a discoverer of this pack has. It builds a `probe` step
/// and nothing else.
pub(crate) struct ReadClient<'a> {
    port: &'a dyn SandboxPort,
    scope: &'a DiscoveryScope<'a>,
}

impl<'a> ReadClient<'a> {
    pub(crate) fn new(port: &'a dyn SandboxPort, scope: &'a DiscoveryScope<'a>) -> Self {
        Self { port, scope }
    }

    async fn send(&self, method: Method, path: &str) -> Result<Values, Failure> {
        let timeout_ms = self.scope.bounds.seconds.saturating_mul(1000);
        let step = Step {
            kind: PROBE,
            refers_to: None,
            args: Vec::new(),
            timeout_ms,
            fails_as: FailsAs {
                class: FailureClass::UnknownVariant,
                code: codes::UNEXPECTED_STATUS,
            },
            commits: false,
            lookup: false,
            idempotent: false,
            when: None,
            payload: PackPayload::encode(&ProbeStep {
                method,
                path: path.to_owned(),
            })?,
        };
        let args = Values::new();
        let input = StepInput::new(&args, self.scope.deadline);
        match self.port.act(&step, &input).await {
            Ok(StepOutcome::Done(observed) | StepOutcome::Absent(observed)) => Ok(observed.values),
            Err(failure) => Err(failure.failure),
        }
    }

    /// A GET of a literal path of the surface.
    pub(crate) async fn get(&self, path: &str) -> Result<Values, Failure> {
        self.send(Method::Get, path).await
    }

    /// An OPTIONS request to a literal path of the surface.
    pub(crate) async fn options(&self, path: &str) -> Result<Values, Failure> {
        self.send(Method::Options, path).await
    }
}

fn produced_by(provider: &ProviderName) -> ProducedBy {
    ProducedBy {
        provider: provider.to_string(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
        component: None,
    }
}

fn suits_http(surface: &SurfaceDescription) -> bool {
    surface.kind == HTTP_SURFACE
        && (surface.address.starts_with("http://")
            || (a2m_auth::tls::enabled() && surface.address.starts_with("https://")))
}

/// `http_surface`: whether the base address answers, and which sign-in
/// schemes it asks for.
pub(crate) struct HttpSurface {
    provider: ProviderName,
    clock: Arc<dyn Clock>,
    kind: FindingKind,
    name: DiscovererName,
}

impl HttpSurface {
    pub(crate) fn new(provider: ProviderName, clock: Arc<dyn Clock>) -> Result<Self, Failure> {
        Ok(Self {
            provider,
            clock,
            kind: FindingKind::new("endpoint")?,
            name: DiscovererName::new("http_surface")?,
        })
    }
}

impl Discoverer for HttpSurface {
    fn name(&self) -> DiscovererName {
        self.name.clone()
    }

    fn suits(&self, surface: &SurfaceDescription) -> bool {
        suits_http(surface)
    }

    fn discover<'a>(
        &'a self,
        port: &'a dyn SandboxPort,
        scope: &'a DiscoveryScope<'a>,
        out: &'a mut dyn FindingSink,
    ) -> BoxFuture<'a, Result<DiscoveryReport, Failure>> {
        Box::pin(async move {
            let origin = Origin::of(&scope.surface.surface.address)?;
            let client = ReadClient::new(port, scope);
            let root = format!("{}/", origin.prefix);
            let mut report = DiscoveryReport {
                coverage: Coverage::Complete,
                ..DiscoveryReport::default()
            };
            for (method, answer) in [
                ("GET", client.get("/").await),
                ("OPTIONS", client.options("/").await),
            ] {
                let Ok(answer) = answer else {
                    // The surface did not answer this request. That is not
                    // a finding; the coverage says the look was partial.
                    report.coverage = Coverage::Partial;
                    report.dropped += 1;
                    continue;
                };
                let text = |name: &str| {
                    answer
                        .get(name)
                        .and_then(Value::as_text)
                        .map_or(Json::Null, |text| Json::Text(text.to_owned()))
                };
                let challenges = answer
                    .get("challenges")
                    .and_then(Value::as_list)
                    .unwrap_or_default()
                    .iter()
                    .filter_map(Value::as_text)
                    .map(|scheme| Json::Text(scheme.to_owned()))
                    .collect();
                let status = answer.get("status").and_then(Value::as_int).unwrap_or(0);
                let found = Json::Object(vec![
                    (
                        "format".to_owned(),
                        Json::Text(ENDPOINT_FOUND_FORMAT.to_owned()),
                    ),
                    ("method".to_owned(), Json::Text(method.to_owned())),
                    ("path".to_owned(), Json::Text(root.clone())),
                    ("status".to_owned(), Json::Number(status.to_string())),
                    ("content_type".to_owned(), text("content_type")),
                    ("allow".to_owned(), text("allow")),
                    ("challenges".to_owned(), Json::List(challenges)),
                ]);
                let finding = Finding::about(
                    Subject::Address(Address::new(&origin.text, &root)?),
                    self.kind.clone(),
                    scope.redact.redact(&found.to_text()),
                    Source::LiveSystem,
                    produced_by(&self.provider),
                    self.clock.now(),
                )
                .level(EvidenceLevel::Discovered)
                .coverage(Coverage::Complete)
                .build()?;
                out.found(finding)?;
            }
            Ok(report)
        })
    }
}

/// `http_description`: what a published OpenAPI description claims, read
/// from a file the developer gave or fetched from the surface.
pub(crate) struct HttpDescription {
    provider: ProviderName,
    clock: Arc<dyn Clock>,
    kind: FindingKind,
    name: DiscovererName,
}

impl HttpDescription {
    pub(crate) fn new(provider: ProviderName, clock: Arc<dyn Clock>) -> Result<Self, Failure> {
        Ok(Self {
            provider,
            clock,
            kind: openapi::described_operation()?,
            name: DiscovererName::new("http_description")?,
        })
    }

    fn finding(
        &self,
        origin: &Origin,
        operation: &DescribedOperation,
        redact: &dyn Redact,
    ) -> Result<Finding, Failure> {
        let path = format!("{}{}", origin.prefix, operation.path);
        // Braces are part of a template; anything else that is not plain
        // makes the address the surface's root, and the path stays in what
        // was found.
        let address =
            Address::new(&origin.text, &path).or_else(|_| Address::new(&origin.text, "/"))?;
        Ok(Finding::about(
            Subject::Address(address),
            self.kind.clone(),
            // Once more as it is written: what was formatted is what goes out.
            redact.redact(&operation.found()),
            Source::PublishedDescription,
            produced_by(&self.provider),
            self.clock.now(),
        )
        .level(EvidenceLevel::Inferred)
        .coverage(Coverage::Complete)
        .limitation(
            LimitationCode::NotExercised,
            "The description says the operation exists; the operation was not called.",
        )
        .build()?)
    }
}

impl Discoverer for HttpDescription {
    fn name(&self) -> DiscovererName {
        self.name.clone()
    }

    fn suits(&self, surface: &SurfaceDescription) -> bool {
        suits_http(surface)
    }

    fn discover<'a>(
        &'a self,
        port: &'a dyn SandboxPort,
        scope: &'a DiscoveryScope<'a>,
        out: &'a mut dyn FindingSink,
    ) -> BoxFuture<'a, Result<DiscoveryReport, Failure>> {
        Box::pin(async move {
            let origin = Origin::of(&scope.surface.surface.address)?;
            let unreadable = |why: &str| {
                codes::failure(
                    FailureClass::Input,
                    codes::DESCRIPTION_UNREADABLE,
                    format!("No description could be read: {why}."),
                    "Put the description in the project's `sources` folder and run `anything2mcp scout run <target> --files '{\"description\": \"sources/<file>\"}'`.",
                )
            };
            let bytes = if let Some(file) = scope.paths.get(DESCRIPTION_FILE) {
                file.read(scope.bounds.bytes)?
            } else {
                let named = scope.focus.get(DESCRIPTION_PATH).and_then(Value::as_text);
                let paths =
                    named.map_or_else(|| DEFAULT_DESCRIPTION_PATHS.to_vec(), |path| vec![path]);
                let client = ReadClient::new(port, scope);
                let mut statuses = Vec::new();
                let mut read = None;
                for path in paths {
                    let answer = client.get(path).await?;
                    let status = answer.get("status").and_then(Value::as_int).unwrap_or(0);
                    let body = answer.get("body").and_then(Value::as_text);
                    match body {
                        // A named path is read whatever its status says; a
                        // place tried by convention only on a success.
                        Some(body) if named.is_some() || (200..300).contains(&status) => {
                            read = Some(body.to_owned());
                            break;
                        }
                        _ => statuses.push(status.to_string()),
                    }
                }
                let body = read.ok_or_else(|| {
                    unreadable(&format!(
                        "the surface answered the request for it with status {}",
                        statuses.join(", then ")
                    ))
                })?;
                if u64::try_from(body.len()).unwrap_or(u64::MAX) > scope.bounds.bytes {
                    return Err(openapi::too_large(&format!(
                        "it is more than the {} bytes the discovery may read",
                        scope.bounds.bytes
                    )));
                }
                body.into_bytes()
            };
            // The redactor comes first, whichever way the description
            // arrived: before the text is parsed, cut, or quoted.
            let text = openapi::redacted(&bytes, scope.redact)?;
            let operations = openapi::operations(text.as_bytes())?;
            for operation in &operations {
                out.found(self.finding(&origin, operation, scope.redact)?)?;
            }
            Ok(DiscoveryReport {
                coverage: Coverage::Complete,
                ..DiscoveryReport::default()
            })
        })
    }
}

/// `http_describer`: endpoints as method and path, with what the system
/// itself supplied kept apart.
pub(crate) struct HttpDescriber;

impl Describer for HttpDescriber {
    fn describe(&self, findings: &[Finding], form: DescribeForm) -> Description {
        let mut lines = Vec::new();
        let mut untrusted = Vec::new();
        let (mut safe, mut unsafe_) = (0_u32, 0_u32);
        for finding in findings {
            let Ok(found) = Json::parse(finding.found().as_bytes()) else {
                continue;
            };
            let word = |name: &str| found.get(name).and_then(Json::as_text).unwrap_or_default();
            let (method, path) = (word("method"), word("path"));
            // A method is one of a closed list; a path is the system's own
            // text, and is shown apart.
            let Some(method) = Method::from_word(method) else {
                continue;
            };
            if method.is_safe() {
                safe += 1;
            } else {
                unsafe_ += 1;
            }
            lines.push(Value::from(
                Values::new()
                    .with("method", method.word())
                    .with("level", finding.level().to_string()),
            ));
            untrusted.push(format!("{} {}", method.word(), codes::shown(path)));
        }
        let text = format!(
            "{} endpoints: {safe} of a method HTTP marks safe, {unsafe_} that may write. Paths are the system's own text and are listed apart.",
            lines.len()
        );
        Description {
            text,
            data: match form {
                DescribeForm::Summary => Values::new().with("endpoints", i64::from(safe + unsafe_)),
                DescribeForm::Full => Values::new().with("endpoints", lines),
            },
            untrusted,
        }
    }
}
