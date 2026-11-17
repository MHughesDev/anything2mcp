//! Discovery over an upstream server's tool list: the discoverer
//! `mcp_tools`, its describer, and the drafting rule from a listed tool to
//! a mapping entry.
//!
//! Listing tools is a read the protocol defines. The discoverer sends one
//! `list_tools` step through the sandbox port and calls no tool. What a
//! tool's title and description say is a claim by the server: it is kept in
//! the finding under `untrusted`, and the describer shows it only as
//! untrusted text.

use std::sync::Arc;

use a2m_core::{
    Address, Coverage, EvidenceLevel, FailsAs, Failure, FailureClass, FailureCode, FileKindName,
    Finding, FindingKind, PackPayload, ProducedBy, Source, Step, Subject, Values,
};
use a2m_files::{DraftContext, DraftDoc, DraftValue, DraftingRule, RelPath};
use a2m_provider::{
    DescribeForm, Describer, Description, Discoverer, DiscovererName, DiscoveryReport,
    DiscoveryScope, FindingSink, SandboxPort, StepInput, StepOutcome, SurfaceDescription,
};
use a2m_telemetry::Clock;
use serde_json::{Value as Json, json};

use crate::codes;
use crate::json::{canonical, parse_digest};
use crate::mapping::LOCAL_ADDRESS;
use crate::names::{self, Names};

/// How long the one `list_tools` step may take, within the job's deadline.
const LIST_TIMEOUT_MS: u32 = 60_000;

/// The discoverer `mcp_tools`. It lists, and never calls.
pub struct McpTools {
    names: Names,
    clock: Arc<dyn Clock>,
}

impl McpTools {
    pub(crate) fn new(names: Names, clock: Arc<dyn Clock>) -> Self {
        Self { names, clock }
    }
}

fn invalid(what: &str) -> Failure {
    Failure::new(
        FailureClass::Input,
        FailureCode::FINDING_INVALID,
        format!("A finding about an upstream server could not be made: {what}."),
        "Report this as a defect of the MCP pack.",
    )
}

/// The identity of what was observed: the server's address, or the local
/// server program by its content.
fn subject(address: &str, listing: &Json) -> Result<Subject, Failure> {
    if address == LOCAL_ADDRESS {
        let digest = listing
            .get("program_digest")
            .and_then(Json::as_str)
            .and_then(parse_digest)
            .ok_or_else(|| invalid("the local server program has no digest"))?;
        return Ok(Subject::File {
            digest,
            format: "mcp_server_program".to_owned(),
            part: None,
        });
    }
    let (scheme, rest) = address
        .split_once("://")
        .ok_or_else(|| invalid("the surface's address has no scheme"))?;
    let (host, path) = match rest.split_once('/') {
        Some((host, path)) => (host, format!("/{path}")),
        None => (rest, "/".to_owned()),
    };
    Address::new(&format!("{scheme}://{host}"), &path)
        .map(Subject::Address)
        .map_err(|_| invalid("the surface's address is not a plain origin and path"))
}

impl Discoverer for McpTools {
    fn name(&self) -> DiscovererName {
        self.names.discoverer.clone()
    }

    fn suits(&self, surface: &SurfaceDescription) -> bool {
        surface.kind == names::SURFACE_KIND
    }

    fn discover<'a>(
        &'a self,
        port: &'a dyn SandboxPort,
        scope: &'a DiscoveryScope<'a>,
        out: &'a mut dyn FindingSink,
    ) -> a2m_provider::BoxFuture<'a, Result<DiscoveryReport, Failure>> {
        Box::pin(async move {
            let step = Step {
                kind: names::LIST_TOOLS,
                refers_to: None,
                args: Vec::new(),
                timeout_ms: LIST_TIMEOUT_MS,
                fails_as: FailsAs {
                    class: FailureClass::UnknownVariant,
                    code: codes::UNEXPECTED_STATUS,
                },
                commits: false,
                lookup: false,
                idempotent: true,
                when: None,
                payload: PackPayload::none(),
            };
            let args = Values::new();
            let input = StepInput {
                args: &args,
                record: None,
                key: None,
                stamp: None,
                deadline: scope.deadline,
            };
            let observed = match port.act(&step, &input).await {
                Ok(StepOutcome::Done(observed) | StepOutcome::Absent(observed)) => observed,
                Err(failure) => return Err(failure.failure),
            };
            let listing: Json = observed
                .values
                .get("listing")
                .and_then(|listing| listing.as_text())
                .and_then(|text| serde_json::from_str(text).ok())
                .ok_or_else(|| invalid("the driver returned no tool list"))?;
            let subject = subject(&scope.surface.surface.address, &listing)?;
            let produced_by = || ProducedBy {
                provider: names::PACK.to_owned(),
                version: env!("CARGO_PKG_VERSION").to_owned(),
                component: None,
            };
            let now = self.clock.now();
            let finding = |kind: &FindingKind, found: &Json| {
                Finding::about(
                    subject.clone(),
                    kind.clone(),
                    scope.redact.redact(&canonical(found)),
                    Source::LiveSystem,
                    produced_by(),
                    now,
                )
                .level(EvidenceLevel::Discovered)
                .coverage(Coverage::Complete)
                .build()
                .map_err(|_| invalid("its level or its subject was refused"))
            };

            let surface = scope.surface.surface.name.as_str();
            let seen = listing.get("server").filter(|server| !server.is_null());
            let mapped = listing
                .get("mapped_server")
                .filter(|server| !server.is_null());
            out.found(finding(
                &self.names.upstream_server,
                &json!({
                    "surface": surface,
                    "server": seen,
                    "mapped_server": mapped,
                    "version_changed": match (seen, mapped) {
                        (Some(seen), Some(mapped)) => Json::Bool(seen != mapped),
                        _ => Json::Null,
                    },
                    "gone": listing.get("gone"),
                }),
            )?)?;
            let tools = listing
                .get("tools")
                .and_then(Json::as_array)
                .ok_or_else(|| invalid("the driver's tool list has no tools"))?;
            for tool in tools {
                let part = |key: &str| tool.get(key).cloned().unwrap_or(Json::Null);
                out.found(finding(
                    &self.names.upstream_tool,
                    &json!({
                        "surface": surface,
                        "tool": part("name"),
                        "digest": part("digest"),
                        "pin": part("pin"),
                        "read_only": part("read_only"),
                        "free_text": part("free_text"),
                        "unchecked_keywords": part("unchecked_keywords"),
                        "input_schema": part("input_schema"),
                        "output_schema": part("output_schema"),
                        "annotations": part("annotations"),
                        // The server's own words about the tool: claims.
                        "untrusted": {"title": part("title"), "description": part("description")},
                    }),
                )?)?;
            }
            Ok(DiscoveryReport {
                coverage: Coverage::Complete,
                bounds_hit: Vec::new(),
                dropped: 0,
            })
        })
    }
}

/// What a finding of this pack holds, read back from its text.
fn found(finding: &Finding) -> Option<Json> {
    serde_json::from_str(finding.found()).ok()
}

/// Whether a name the server chose is plain enough to stand in text the
/// pack writes: the characters a tool's name is made of, and not long.
fn plain_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 96
        && name.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.')
        })
}

/// The describer of `mcp_tools` findings. Pure: it reads findings only.
pub struct McpDescriber {
    names: Names,
}

impl McpDescriber {
    pub(crate) fn new(names: Names) -> Self {
        Self { names }
    }
}

impl Describer for McpDescriber {
    fn describe(&self, findings: &[Finding], form: DescribeForm) -> Description {
        let tools: Vec<Json> = findings
            .iter()
            .filter(|finding| finding.kind() == &self.names.upstream_tool)
            .filter_map(found)
            .collect();
        let count = |key: &str, value: &Json| {
            tools
                .iter()
                .filter(|tool| tool.get(key) == Some(value))
                .count()
        };
        let pinned = |state: &str| count("pin", &Json::String(state.to_owned()));
        let mut text = format!(
            "The server lists {} tools: {} it marks read-only, {} it does not. Of them {} match their pin in the mapping, {} have changed, and {} are not mapped. {} take one free-text string.",
            tools.len(),
            count("read_only", &Json::Bool(true)),
            count("read_only", &Json::Bool(false)),
            pinned("matches"),
            pinned("changed"),
            pinned("unmapped") + pinned("unpinned"),
            count("free_text", &Json::Bool(true)),
        );
        let mut untrusted = Vec::new();
        let mut data = Values::new();
        for server in findings
            .iter()
            .filter(|finding| finding.kind() == &self.names.upstream_server)
            .filter_map(found)
        {
            match server.get("version_changed").and_then(Json::as_bool) {
                Some(true) => text.push_str(
                    " The server reports another name or version than the mapping was written against.",
                ),
                Some(false) => text.push_str(
                    " The server reports the name and version the mapping was written against.",
                ),
                None => {}
            }
            let gone = server
                .get("gone")
                .and_then(Json::as_array)
                .map_or(0, Vec::len);
            if gone > 0 {
                text.push_str(&format!(
                    " {gone} mapped tools are no longer on the server's list."
                ));
            }
            if let Some(seen) = server.get("server").filter(|seen| !seen.is_null()) {
                untrusted.push(format!("server: {}", canonical(seen)));
            }
        }
        let mut names = Vec::new();
        for tool in &tools {
            let name = tool.get("tool").and_then(Json::as_str).unwrap_or_default();
            let shown = if plain_name(name) {
                names.push(a2m_core::Value::from(name));
                name.to_owned()
            } else {
                untrusted.push(format!("tool name: {name}"));
                "(a tool whose name is not a plain name)".to_owned()
            };
            if form == DescribeForm::Full {
                let word = |key: &str| tool.get(key).and_then(Json::as_str).unwrap_or("unknown");
                text.push_str(&format!(
                    "\n- {shown}: pin {}, digest {}.",
                    word("pin"),
                    word("digest")
                ));
            }
            for key in ["title", "description"] {
                if let Some(words) = tool
                    .get("untrusted")
                    .and_then(|claims| claims.get(key))
                    .and_then(Json::as_str)
                {
                    untrusted.push(format!("{shown} {key}: {words}"));
                }
            }
        }
        data.insert("tools", a2m_core::Value::List(names));
        data.insert("tool_count", i64::try_from(tools.len()).unwrap_or(i64::MAX));
        Description {
            text,
            data,
            untrusted,
        }
    }
}

/// From a listed tool to a mapping entry.
pub(crate) struct ToolToMapping {
    names: Names,
}

impl ToolToMapping {
    pub(crate) fn new(names: Names) -> Self {
        Self { names }
    }
}

/// The string members of an output schema that hold free text: every
/// string property with no closed list of values.
fn free_text_members(schema: &Json) -> Vec<String> {
    schema
        .get("properties")
        .and_then(Json::as_object)
        .into_iter()
        .flatten()
        .filter(|(_, property)| {
            let string = match property.get("type") {
                Some(Json::String(name)) => name == "string",
                Some(Json::Array(names)) => names.iter().any(|name| name == "string"),
                _ => false,
            };
            string && property.get("enum").is_none() && property.get("const").is_none()
        })
        .map(|(name, _)| name.clone())
        .collect()
}

impl DraftingRule for ToolToMapping {
    fn finding_kind(&self) -> FindingKind {
        self.names.upstream_tool.clone()
    }

    fn file_kind(&self) -> FileKindName {
        names::MAPPING_KIND
    }

    /// One mapping document for the tool, in the surface's mapping file. A
    /// tool the server marks read-only is drafted with `effect: read`, for a
    /// person to review; any other tool's effect is left as a question.
    fn draft(&self, finding: &Finding, cx: &DraftContext<'_>) -> Result<Vec<DraftDoc>, Failure> {
        let unreadable = || {
            Failure::new(
                FailureClass::Input,
                FailureCode::FINDING_INVALID,
                "An `upstream_tool` finding states a surface, a tool, and the digest of its definition.",
                "Correct the provider that made the finding.",
            )
        };
        let found = found(finding).ok_or_else(unreadable)?;
        let text = |key: &str| found.get(key).and_then(Json::as_str);
        let (Some(surface), Some(tool), Some(digest)) =
            (text("surface"), text("tool"), text("digest"))
        else {
            return Err(unreadable());
        };
        let effect = if found.get("read_only").and_then(Json::as_bool) == Some(true) {
            DraftValue::text("read")
        } else {
            cx.open_question(&format!(
                "Does the tool `{}` read, write, or do something that cannot be taken back? The server does not mark it read-only.",
                codes::shown(tool)
            ))
        };
        let mut entry = vec![("digest", DraftValue::text(digest)), ("effect", effect)];
        let untrusted: Vec<DraftValue> = found
            .get("output_schema")
            .map(free_text_members)
            .unwrap_or_default()
            .iter()
            .map(|member| DraftValue::text(member))
            .collect();
        if !untrusted.is_empty() {
            entry.push(("untrusted_fields", DraftValue::List(untrusted)));
        }
        if found.get("output_schema").is_none_or(Json::is_null) {
            entry.push(("result", DraftValue::text("text")));
        }
        // Whether the server runs the one string such a tool takes is not
        // in its definition. It is asked, never guessed either way.
        if found.get("free_text").and_then(Json::as_bool) == Some(true) {
            entry.push((
                "free_text",
                cx.open_question(&format!(
                    "The whole input of the tool `{}` is one string. Does the server run it, as a command, a query, or a prompt? State `true` if it does, and the tool is never called; `false` if the string is a key or plain data.",
                    codes::shown(tool)
                )),
            ));
        }
        Ok(vec![DraftDoc {
            path: RelPath::new(&format!("{}/{surface}.yaml", names::MAPPING_FOLDER))?,
            body: DraftValue::map(vec![
                ("surface", DraftValue::text(surface)),
                (
                    "tools",
                    DraftValue::Map(vec![(tool.to_owned(), DraftValue::map(entry))]),
                ),
            ]),
        }])
    }
}
