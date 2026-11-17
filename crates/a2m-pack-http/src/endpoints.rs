//! The `endpoints/` file kind: its schema, its compilation into typed
//! request templates, and the lint rules over it.

use std::collections::BTreeMap;

use a2m_core::{FileKindName, LintCode, PackPayload, SurfaceKind};
use a2m_files::{
    CompileMode, CompiledSection, DefinedName, Diagnostic, Diagnostics, Doc, ExprSite, FileKind,
    JsonSchema, KindContext, Level, LintRule, Located, Place, ResolvedView, Stage,
};

use crate::shape::{Kind, Shape};
use crate::template::{
    Answer, Answers, Auth, EndpointSection, EndpointTemplate, Method, PAYLOAD_FORMAT, VersionRule,
    VersionSink, VersionSource, parse_path,
};

/// The name of this pack, as its manifest gives it.
pub const PACK: &str = "http";
/// The name of the file kind.
pub const ENDPOINT_KIND: FileKindName = FileKindName::fixed("endpoint");
/// The surface kind this pack serves.
pub const HTTP_SURFACE: SurfaceKind = SurfaceKind::fixed("http");
/// The surface kind whose pages make requests from inside; its pack reads
/// the endpoints of its surfaces as data.
pub const WEB_SURFACE: SurfaceKind = SurfaceKind::fixed("web");

/// A code of this pack's compiler: its lint rules, and what its file kind
/// and lowering refuse.
pub(crate) fn code(rule: &'static str) -> LintCode {
    // Every rule name in this crate is a lowercase name; a test holds that.
    LintCode::new(&format!("{PACK}.{rule}")).unwrap_or(LintCode::fixed("http.invalid"))
}

pub(crate) fn problem(
    rule: &'static str,
    stage: Stage,
    at: &Place,
    message: impl Into<String>,
    fix: &str,
) -> Diagnostic {
    Diagnostic::error(code(rule), stage, at, message, fix)
}

/// Header names that carry a credential. None may be written in a file.
const CREDENTIAL_HEADERS: &[&str] = &[
    "authorization",
    "proxy-authorization",
    "cookie",
    "set-cookie",
    "x-api-key",
    "api-key",
    "x-auth-token",
    "x-csrf-token",
];

/// Names of query or body entries that carry a credential.
const CREDENTIAL_NAMES: &[&str] = &[
    "access_token",
    "api_key",
    "apikey",
    "auth",
    "client_secret",
    "key",
    "password",
    "passwd",
    "secret",
    "sig",
    "signature",
    "token",
];

/// Whether text written in a file looks like a credential.
pub(crate) fn looks_like_credential(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    if ["bearer ", "basic ", "token ", "negotiate ", "digest "]
        .iter()
        .any(|scheme| lower.starts_with(scheme))
    {
        return true;
    }
    // A long run with no space that mixes letters and digits: a key.
    let dense = text.len() >= 24
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'+' | b'/' | b'=' | b'.'));
    dense
        && text.bytes().any(|b| b.is_ascii_digit())
        && text.bytes().any(|b| b.is_ascii_alphabetic())
}

fn answer_schema() -> JsonSchema {
    JsonSchema::object()
        .required("status", JsonSchema::int_between(100, 599))
        .optional("when", JsonSchema::map_of(JsonSchema::scalar()))
        .optional("current", JsonSchema::text())
}

/// The schema of one endpoint entry.
pub(crate) fn endpoint_schema() -> JsonSchema {
    let status = || JsonSchema::int_between(100, 599);
    JsonSchema::object()
        .required("endpoint", JsonSchema::name())
        .required("surface", JsonSchema::surface())
        .required("method", JsonSchema::words(Method::WORDS))
        .required("path", JsonSchema::text())
        .optional("query", JsonSchema::map_of(JsonSchema::scalar()))
        .optional("headers", JsonSchema::map_of(JsonSchema::text()))
        .optional("auth", JsonSchema::words(&["session", "token", "none"]))
        .optional("anti_forgery", JsonSchema::any())
        .optional("request_schema", JsonSchema::any())
        .optional("response_schema", JsonSchema::any())
        .required("effect", JsonSchema::words(&["read", "write"]))
        .optional(
            "responses",
            JsonSchema::object()
                .optional("ok", JsonSchema::non_empty_list(status()))
                .optional("absent", JsonSchema::list(status()))
                .optional("refused", JsonSchema::list(answer_schema()))
                .optional("conflict", JsonSchema::list(answer_schema()))
                .optional("unavailable", JsonSchema::list(answer_schema())),
        )
        .optional("idempotency_header", JsonSchema::text())
        .optional(
            "version",
            JsonSchema::object()
                .optional(
                    "read",
                    JsonSchema::object()
                        .optional("field", JsonSchema::text())
                        .optional("header", JsonSchema::text()),
                )
                .optional(
                    "send",
                    JsonSchema::object()
                        .optional("body", JsonSchema::text())
                        .optional("header", JsonSchema::text()),
                ),
        )
        .optional("volatile", JsonSchema::list(JsonSchema::text()))
        .optional("signed", JsonSchema::bool())
        .optional("graphql", JsonSchema::any())
        .optional(
            "max_response_bytes",
            JsonSchema::int_between(1, 268_435_456),
        )
}

/// Whether text can be a header's name.
fn is_header_name(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
}

/// Whether text can be a header's value without changing the request.
fn is_header_value(text: &str) -> bool {
    text.bytes().all(|byte| (0x20..0x7f).contains(&byte))
}

fn status_of(doc: &Located<Doc>) -> u16 {
    doc.as_int()
        .and_then(|number| u16::try_from(number).ok())
        .unwrap_or_default()
}

fn answers_of(list: Option<&Located<Doc>>) -> Vec<Answer> {
    list.map(Located::items)
        .unwrap_or_default()
        .iter()
        .map(|item| Answer {
            status: item.get("status").map(status_of).unwrap_or_default(),
            when: item
                .get("when")
                .map(Located::entries)
                .unwrap_or_default()
                .iter()
                .filter_map(|entry| {
                    a2m_files::expr_text(&entry.value).map(|text| (entry.key.value.clone(), text))
                })
                .collect(),
            current: item.text("current").map(str::to_owned),
        })
        .collect()
}

/// Compiles one endpoint entry into its template. Every refusal has the
/// place of the value at fault and says how to correct it.
pub(crate) fn template_of(
    doc: &Located<Doc>,
    surface: &str,
    problems: &mut Diagnostics,
) -> Option<EndpointTemplate> {
    let before = problems.len();
    let mut refuse = |rule: &'static str, at: &Place, message: String, fix: &str| {
        problems.push(problem(rule, Stage::Lower, at, message, fix));
    };

    let method = doc
        .text("method")
        .and_then(Method::from_word)
        .unwrap_or(Method::Get);
    let (path, trailing_slash) = match doc.get("path") {
        Some(written) => match parse_path(written.as_text().unwrap_or_default()) {
            Ok(parsed) => parsed,
            Err(why) => {
                refuse(
                    "path_invalid",
                    &written.at,
                    format!("The endpoint's path is not a template this pack sends: {why}."),
                    "Write the path from the root of the surface, such as `/orders/{order_id}`, with each placeholder as one whole segment.",
                );
                (Vec::new(), false)
            }
        },
        None => (Vec::new(), false),
    };

    let mut headers = BTreeMap::new();
    for entry in doc.get("headers").map(Located::entries).unwrap_or_default() {
        let name = entry.key.value.to_ascii_lowercase();
        let value = entry.value.as_text().unwrap_or_default();
        if !is_header_name(&name) || !is_header_value(value) {
            refuse(
                "header_invalid",
                &entry.key.at,
                "A header's name is letters, digits, and `-`, and its value is visible text on one line.".to_owned(),
                "Correct the header, or remove it.",
            );
            continue;
        }
        if matches!(name.as_str(), "host" | "content-length" | "transfer-encoding" | "connection") {
            refuse(
                "header_invalid",
                &entry.key.at,
                format!("The header `{name}` is set by the connection and cannot be written in an endpoint."),
                "Remove the header.",
            );
            continue;
        }
        headers.insert(name, value.to_owned());
    }
    let query: BTreeMap<String, String> = doc
        .get("query")
        .map(Located::entries)
        .unwrap_or_default()
        .iter()
        .filter_map(|entry| {
            a2m_files::expr_text(&entry.value).map(|text| (entry.key.value.clone(), text))
        })
        .collect();

    for (key, what) in [
        ("anti_forgery", "The anti-forgery bootstrap"),
        ("graphql", "A GraphQL endpoint"),
    ] {
        if let Some(written) = doc.get(key) {
            refuse(
                "not_built",
                &written.at,
                format!("{what} is not built in this version of the HTTP pack."),
                "Remove the key, or bind the operation through another mode.",
            );
        }
    }

    let mut shape_of = |key: &str| -> Option<Shape> {
        let written = doc.get(key)?;
        match Shape::from_doc(written) {
            Ok(shape) => Some(shape),
            Err(wrong) => {
                problems.push(problem(
                    "shape_invalid",
                    Stage::Lower,
                    &wrong.at,
                    wrong.message,
                    "Write the shape with `type`, `properties`, `required`, `items`, `enum`, `nullable`, and `additionalProperties` only.",
                ));
                None
            }
        }
    };
    let request = shape_of("request_schema");
    // An entry without a response shape is refused by the lint rule
    // `http.no_response_schema`; it compiles to a shape that accepts nothing
    // but an absent body, so that nothing unchecked can pass.
    let response = shape_of("response_schema").unwrap_or_else(|| Shape::of(Kind::Null));
    let mut refuse = |rule: &'static str, at: &Place, message: String, fix: &str| {
        problems.push(problem(rule, Stage::Lower, at, message, fix));
    };
    if let (Some(shape), Some(written)) = (&request, doc.get("request_schema")) {
        if shape.kind != Kind::Object {
            refuse(
                "shape_invalid",
                &written.at,
                "A request's shape is an object: a body is built field by field.".to_owned(),
                "Write `type: object` with the body's fields under `properties`.",
            );
        }
        if !method.has_body() {
            refuse(
                "shape_invalid",
                &written.at,
                format!("A {} request carries no body, and the endpoint declares one.", method.word()),
                "Remove `request_schema`, or correct the method.",
            );
        }
    }

    let responses = doc.get("responses");
    let statuses = |key: &str| -> Vec<u16> {
        responses
            .and_then(|responses| responses.get(key))
            .map(Located::items)
            .unwrap_or_default()
            .iter()
            .map(status_of)
            .collect()
    };
    let mut answers = Answers {
        ok: statuses("ok"),
        absent: statuses("absent"),
        refused: answers_of(responses.and_then(|r| r.get("refused"))),
        conflict: answers_of(responses.and_then(|r| r.get("conflict"))),
        unavailable: answers_of(responses.and_then(|r| r.get("unavailable"))),
    };
    if answers.ok.is_empty() {
        answers.ok.push(200);
    }
    if let Some(twice) = answers
        .ok
        .iter()
        .find(|status| answers.absent.contains(status))
    {
        refuse(
            "responses_invalid",
            &responses.map_or_else(|| doc.at.clone(), |r| r.at.clone()),
            format!("The status {twice} is declared both as accepted and as absent."),
            "Declare each status once.",
        );
    }

    let version_doc = doc.get("version");
    let one_of = |part: &str, first: &str, second: &str, refuse: &mut dyn FnMut(&Place)| {
        let written = version_doc.and_then(|version| version.get(part))?;
        match (written.text(first), written.text(second)) {
            (Some(name), None) => Some((true, name.to_owned())),
            (None, Some(name)) => Some((false, name.to_owned())),
            _ => {
                refuse(&written.at);
                None
            }
        }
    };
    let mut version_problem = |at: &Place| {
        problems.push(problem(
            "version_invalid",
            Stage::Lower,
            at,
            "A version rule names exactly one place: `field` or `header` to read it, `body` or `header` to send it.",
            "Name one place.",
        ));
    };
    let version = VersionRule {
        read: one_of("read", "field", "header", &mut version_problem).map(|(field, name)| {
            if field {
                VersionSource::Field(name)
            } else {
                VersionSource::Header(name.to_ascii_lowercase())
            }
        }),
        send: one_of("send", "body", "header", &mut version_problem).map(|(body, name)| {
            if body {
                VersionSink::Body(name)
            } else {
                VersionSink::Header(name.to_ascii_lowercase())
            }
        }),
    };
    let mut refuse = |rule: &'static str, at: &Place, message: String, fix: &str| {
        problems.push(problem(rule, Stage::Lower, at, message, fix));
    };
    for name in [
        match &version.send {
            Some(VersionSink::Header(name)) => Some(name),
            _ => None,
        },
        match &version.read {
            Some(VersionSource::Header(name)) => Some(name),
            _ => None,
        },
    ]
    .into_iter()
    .flatten()
    {
        if !is_header_name(name) {
            refuse(
                "version_invalid",
                &version_doc.map_or_else(|| doc.at.clone(), |v| v.at.clone()),
                "A version rule's header is not a header's name.".to_owned(),
                "Write the header's name, such as `if-match`.",
            );
        }
    }

    let idempotency_header = doc.get("idempotency_header").and_then(|written| {
        let name = written.as_text().unwrap_or_default().to_ascii_lowercase();
        if is_header_name(&name) {
            Some(name)
        } else {
            refuse(
                "header_invalid",
                &written.at,
                "The idempotency header is not a header's name.".to_owned(),
                "Write the header's name, such as `Idempotency-Key`.",
            );
            None
        }
    });

    if problems.len() > before {
        return None;
    }
    Some(EndpointTemplate {
        surface: surface.to_owned(),
        method,
        path,
        trailing_slash,
        query,
        headers,
        auth: match doc.text("auth") {
            Some("none") => Auth::None,
            Some("token") => Auth::Token,
            _ => Auth::Session,
        },
        writes: doc.text("effect") == Some("write"),
        request,
        response,
        answers,
        idempotency_header,
        version,
        signed: doc.get("signed").and_then(Located::as_bool) == Some(true),
        max_response_bytes: doc
            .get("max_response_bytes")
            .and_then(Located::as_int)
            .and_then(|number| u64::try_from(number).ok()),
        volatile: doc
            .get("volatile")
            .map(Located::items)
            .unwrap_or_default()
            .iter()
            .filter_map(|item| item.as_text().map(str::to_owned))
            .collect(),
    })
}

/// The `endpoints/` file kind.
pub(crate) struct EndpointKind {
    schema: JsonSchema,
    kinds: Vec<SurfaceKind>,
}

impl EndpointKind {
    pub(crate) fn new() -> Self {
        Self {
            schema: endpoint_schema(),
            kinds: vec![HTTP_SURFACE, WEB_SURFACE],
        }
    }
}

impl FileKind for EndpointKind {
    fn name(&self) -> FileKindName {
        ENDPOINT_KIND
    }

    fn surface_kinds(&self) -> &[SurfaceKind] {
        &self.kinds
    }

    fn folder(&self) -> &str {
        "endpoints"
    }

    fn schema(&self) -> &JsonSchema {
        &self.schema
    }

    fn names(&self, doc: &Located<Doc>) -> Vec<Located<DefinedName>> {
        doc.get("endpoint")
            .and_then(|name| {
                name.as_text().map(|text| {
                    Located::new(
                        DefinedName {
                            name: text.to_owned(),
                        },
                        name.at.clone(),
                    )
                })
            })
            .into_iter()
            .collect()
    }

    fn expressions(&self, _doc: &Located<Doc>) -> Vec<ExprSite> {
        // An endpoint holds templates and shapes, and no expression: values
        // arrive from a binding, into typed positions.
        Vec::new()
    }

    fn compile(
        &self,
        docs: &[Located<Doc>],
        cx: &KindContext<'_>,
    ) -> Result<CompiledSection, Diagnostics> {
        let mut section = EndpointSection::new();
        let mut problems = Diagnostics::new();
        for doc in docs {
            let name = doc.text("endpoint").unwrap_or_default();
            let Some(surface) = cx.surface_of(doc) else {
                continue;
            };
            if let Some(template) = template_of(doc, surface.name.as_str(), &mut problems) {
                section.insert(name.to_owned(), template);
            }
        }
        if !problems.is_empty() {
            return Err(problems);
        }
        PackPayload::encode(&section)
            .map(|payload| CompiledSection {
                format: PAYLOAD_FORMAT,
                payload,
            })
            .map_err(|failure| {
                Diagnostics::from(problem(
                    "not_encodable",
                    Stage::Lower,
                    &docs
                        .first()
                        .map_or_else(|| Place::start_of(&a2m_files::RelPath::target_file()), |doc| doc.at.clone()),
                    failure.message().to_owned(),
                    "Keep every number in an endpoint whole.",
                ))
            })
    }
}

/// Every endpoint entry of the target, on either surface kind.
fn entries<'a>(view: &ResolvedView<'a>) -> impl Iterator<Item = &'a Located<Doc>> {
    view.entries(&ENDPOINT_KIND, &HTTP_SURFACE)
        .iter()
        .chain(view.entries(&ENDPOINT_KIND, &WEB_SURFACE))
}

/// `http.secret_literal`.
pub(crate) struct SecretLiteral;

impl LintRule for SecretLiteral {
    fn code(&self) -> LintCode {
        code("secret_literal")
    }

    fn level(&self, _mode: CompileMode) -> Level {
        Level::Error
    }

    fn summary(&self) -> &str {
        "No header, query, or body template of an endpoint holds a literal that looks like a credential."
    }

    fn check(&self, view: &ResolvedView<'_>) -> Vec<Diagnostic> {
        let found = |at: &Place, what: &str| {
            problem(
                "secret_literal",
                Stage::Lint,
                at,
                format!("{what} looks like a credential written into a target file."),
                "Remove it. A credential is stored with `anything2mcp secret set` and reaches a request only through the session.",
            )
        };
        let mut out = Vec::new();
        for doc in entries(view) {
            for entry in doc.get("headers").map(Located::entries).unwrap_or_default() {
                let name = entry.key.value.to_ascii_lowercase();
                let value = entry.value.as_text().unwrap_or_default();
                if CREDENTIAL_HEADERS.contains(&name.as_str()) || looks_like_credential(value) {
                    out.push(found(&entry.key.at, "A header of an endpoint"));
                }
            }
            for entry in doc.get("query").map(Located::entries).unwrap_or_default() {
                let name = entry.key.value.to_ascii_lowercase();
                let value = a2m_files::expr_text(&entry.value).unwrap_or_default();
                if CREDENTIAL_NAMES.contains(&name.as_str()) || looks_like_credential(&value) {
                    out.push(found(&entry.key.at, "A query entry of an endpoint"));
                }
            }
        }
        // The literals a binding writes into a query or a body.
        for operation in view.operations() {
            let bindings = operation.doc().get("bindings");
            for binding in bindings.map(Located::items).unwrap_or_default() {
                if binding.text("mode") != Some(PACK) {
                    continue;
                }
                for part in ["query", "body"] {
                    for entry in binding.get(part).map(Located::entries).unwrap_or_default() {
                        let text = a2m_files::expr_text(&entry.value).unwrap_or_default();
                        let literal = text
                            .strip_prefix(['\'', '"'])
                            .and_then(|rest| rest.strip_suffix(['\'', '"']));
                        let Some(literal) = literal else {
                            continue;
                        };
                        let name = entry.key.value.to_ascii_lowercase();
                        if CREDENTIAL_NAMES.contains(&name.as_str())
                            || looks_like_credential(literal)
                        {
                            out.push(found(&entry.key.at, "A literal in a binding"));
                        }
                    }
                }
            }
        }
        out
    }
}

/// `http.no_response_schema`.
pub(crate) struct NoResponseSchema;

impl LintRule for NoResponseSchema {
    fn code(&self) -> LintCode {
        code("no_response_schema")
    }

    fn level(&self, _mode: CompileMode) -> Level {
        Level::Error
    }

    fn summary(&self) -> &str {
        "Every endpoint states the shape of its response."
    }

    fn check(&self, view: &ResolvedView<'_>) -> Vec<Diagnostic> {
        entries(view)
            .filter(|doc| doc.get("response_schema").is_none())
            .map(|doc| {
                problem(
                    "no_response_schema",
                    Stage::Lint,
                    &doc.at,
                    format!(
                        "The endpoint `{}` states no shape for its response, so nothing it answers could be checked.",
                        crate::codes::shown(doc.text("endpoint").unwrap_or_default())
                    ),
                    "Add `response_schema:` with the shape of the body; `{type: any}` for a body that is not read.",
                )
            })
            .collect()
    }
}

/// `http.effect_method`.
pub(crate) struct EffectMethod;

impl LintRule for EffectMethod {
    fn code(&self) -> LintCode {
        code("effect_method")
    }

    fn level(&self, _mode: CompileMode) -> Level {
        Level::Warning
    }

    fn summary(&self) -> &str {
        "An endpoint marked `read` uses a method HTTP marks safe."
    }

    fn check(&self, view: &ResolvedView<'_>) -> Vec<Diagnostic> {
        entries(view)
            .filter(|doc| doc.text("effect") == Some("read"))
            .filter_map(|doc| {
                let method = doc.get("method")?;
                let safe = method
                    .as_text()
                    .and_then(Method::from_word)
                    .is_some_and(Method::is_safe);
                (!safe).then(|| {
                    problem(
                        "effect_method",
                        Stage::Lint,
                        &method.at,
                        "The endpoint is marked `read`, and HTTP does not mark its method as safe.",
                        "Mark the endpoint `write`, or confirm with the system's owner that the request changes nothing.",
                    )
                })
            })
            .collect()
    }
}

/// `http.signed_endpoint`.
pub(crate) struct SignedEndpoint;

impl LintRule for SignedEndpoint {
    fn code(&self) -> LintCode {
        code("signed_endpoint")
    }

    fn level(&self, _mode: CompileMode) -> Level {
        Level::Error
    }

    fn summary(&self) -> &str {
        "No `http` binding names an endpoint whose requests the front end computes or signs."
    }

    fn check(&self, view: &ResolvedView<'_>) -> Vec<Diagnostic> {
        let signed: Vec<&str> = entries(view)
            .filter(|doc| doc.get("signed").and_then(Located::as_bool) == Some(true))
            .filter_map(|doc| doc.text("endpoint"))
            .collect();
        let mut out = Vec::new();
        for operation in view.operations() {
            let bindings = operation.doc().get("bindings");
            for binding in bindings.map(Located::items).unwrap_or_default() {
                if binding.text("mode") != Some(PACK) {
                    continue;
                }
                let named = [
                    binding.get("endpoint"),
                    binding.get("read_back").and_then(|r| r.get("endpoint")),
                ];
                for endpoint in named.into_iter().flatten() {
                    if endpoint.as_text().is_some_and(|name| signed.contains(&name)) {
                        out.push(problem(
                            "signed_endpoint",
                            Stage::Lint,
                            &endpoint.at,
                            "The endpoint's requests are computed or signed by the front end, and an `http` binding would send them without that.",
                            "Bind the operation through a mode that acts inside the page.",
                        ));
                    }
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::looks_like_credential;

    #[test]
    fn a_literal_that_looks_like_a_credential_is_recognized() {
        for secret in [
            "Bearer abc",
            "basic dXNlcjpwYXNz",
            "example-key-51h8abcdefghijklmnopqrstu",
            "0123456789abcdef0123456789abcdef",
        ] {
            assert!(looks_like_credential(secret), "{secret}");
        }
        for plain in ["application/json", "en-US", "draft", "2026-11-17", "no-cache"] {
            assert!(!looks_like_credential(plain), "{plain}");
        }
    }
}
