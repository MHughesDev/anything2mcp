//! The mapping file kind: per `mcp` surface, the only tools that can be
//! reached, each pinned by the digest of its definition. It is the upstream
//! server's manifest (spec section 304).
//!
//! Also here: the `mcp` surface kind's own fields, and what both compile to.

use std::collections::BTreeMap;

use a2m_core::{FileKindName, PackPayload, Plan, SurfaceKind, SurfacePlan};
use a2m_files::{
    CompiledSection, DefinedName, Diagnostic, Diagnostics, Doc, ExprSite, FileKind, JsonSchema,
    KindContext, Located, Place, RelPath, Stage, SurfaceKindSpec,
};
use serde::{Deserialize, Serialize};

use crate::names;

/// The version of the compiled mapping's shape.
pub(crate) const SECTION_FORMAT: u16 = 1;

/// The revision whose connection starts with the `initialize` handshake.
pub(crate) const REVISION_HANDSHAKE: &str = "2025-11-25";
/// The revision with no handshake: every request names it.
pub(crate) const REVISION_NO_HANDSHAKE: &str = "2026-07-28";
/// What a local server program's surface writes as its address.
pub(crate) const LOCAL_ADDRESS: &str = "stdio";

/// The `mcp` surface kind: its address, and for a local server program the
/// program by path and digest with its arguments and its environment.
pub(crate) fn surface_kind_spec() -> SurfaceKindSpec {
    SurfaceKindSpec::new(
        names::surface_kind(),
        "address",
        JsonSchema::object()
            .required("address", JsonSchema::text())
            .optional("program", JsonSchema::text())
            .optional("digest", JsonSchema::text())
            .optional("args", JsonSchema::list(JsonSchema::text()))
            .optional("environment", JsonSchema::map_of(JsonSchema::text()))
            .optional("pass_environment", JsonSchema::list(JsonSchema::text()))
            .optional(
                "revision",
                JsonSchema::words(&[REVISION_HANDSHAKE, REVISION_NO_HANDSHAKE]),
            ),
    )
    .with_program("program", Some("digest"))
}

/// What an `mcp` surface states beside its address, as the compiler put it
/// in the surface's payload.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SurfaceFields {
    /// The local server program's absolute path.
    #[serde(default)]
    pub program: Option<String>,
    /// The digest the program must have.
    #[serde(default)]
    pub digest: Option<String>,
    /// Its arguments. Fixed; nothing from a caller is ever added.
    #[serde(default)]
    pub args: Vec<String>,
    /// Variables it is started with. Never a secret.
    #[serde(default)]
    pub environment: BTreeMap<String, String>,
    /// Names passed through from the engine's own environment.
    #[serde(default)]
    pub pass_environment: Vec<String>,
    /// The protocol revision the connection speaks.
    #[serde(default)]
    pub revision: Option<String>,
}

impl SurfaceFields {
    /// Reads the fields of a surface of the plan.
    pub(crate) fn of(surface: &SurfacePlan) -> Result<Self, a2m_core::Failure> {
        if surface.payload.is_none() {
            Ok(Self::default())
        } else {
            surface.payload.decode()
        }
    }
}

/// One mapped tool, as the plan holds it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ToolEntry {
    /// The digest that pins the tool's definition.
    pub digest: Option<String>,
    /// The effect class the developer judged: `read`, `write`, or
    /// `irreversible`.
    pub effect: String,
    /// The mapped read tool that reads back what this tool wrote.
    pub read_back: Option<String>,
    /// The members of a structured result that hold text and may be
    /// passed on. A member that can hold text and is not named here, or
    /// under `key` or `records`, is left out of what a step returns.
    pub untrusted_fields: Vec<String>,
    /// `structured` or `text`: where the result's values are.
    pub result: String,
    /// An error result whose text starts with this means the record is
    /// absent.
    pub absent_error: Option<String>,
    /// The member of the result that lists records, for a lookup.
    pub records: Option<String>,
    /// The members of the result that hold the key the server assigned.
    pub key: Vec<String>,
    /// The argument that carries the record's stamp.
    pub stamp: Option<String>,
    /// The arguments that take the parts of the record's key, in order.
    pub key_args: Vec<String>,
    /// What a reviewer found the server does with the text the tool is
    /// given. `false`: it does not run it, so a tool whose whole input is
    /// one string may be called. `true`: it runs it, as a command, a
    /// query, or a prompt, so the tool is never called. Nothing when no one
    /// has said: a tool whose whole input is one string is then not called.
    pub free_text: Option<bool>,
}

impl ToolEntry {
    /// Whether a call to the tool changes anything.
    pub(crate) fn writes(&self) -> bool {
        self.effect != "read"
    }

    /// Whether the mapping says how an absent record shows.
    pub(crate) fn can_be_absent(&self) -> bool {
        self.absent_error.is_some() || self.records.is_some()
    }
}

/// The server a mapping was written against.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ServerSeen {
    pub name: String,
    pub version: String,
}

/// One surface's mapping, as the plan holds it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Mapping {
    /// `tools`, or `discovery` for a server whose tools are never bound.
    pub purpose: String,
    /// The name and version the server reported when the mapping was
    /// written.
    pub server: Option<ServerSeen>,
    /// The only tools that can be reached.
    pub tools: BTreeMap<String, ToolEntry>,
}

impl Default for Mapping {
    fn default() -> Self {
        Self {
            purpose: "tools".to_owned(),
            server: None,
            tools: BTreeMap::new(),
        }
    }
}

/// Every mapping of a target, by surface: the pack's one section of the
/// plan.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Section {
    pub surfaces: BTreeMap<String, Mapping>,
}

impl Section {
    /// The pack's section of a published plan. A plan with no mapping file
    /// has none, and nothing is reachable.
    pub(crate) fn of(plan: &Plan) -> Result<Self, a2m_core::Failure> {
        let found = plan.packs.iter().find(|section| {
            section.pack == names::PACK
                && section.kind == names::mapping_kind()
                && section.surface_kind == names::surface_kind()
        });
        match found {
            None => Ok(Self::default()),
            Some(section) if section.format == SECTION_FORMAT => section.payload.decode(),
            Some(_) => Err(crate::codes::plan_unreadable(
                "its mapping section has a format this build does not read",
            )),
        }
    }
}

/// What a step of the kind `call_tool` carries: which tool, on which
/// surface. Everything else about the tool is read from the mapping, so it
/// is written once.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CallPayload {
    pub surface: String,
    pub tool: String,
}

fn tool_schema() -> JsonSchema {
    JsonSchema::object()
        .optional("digest", JsonSchema::text())
        .required(
            "effect",
            JsonSchema::words(&["read", "write", "irreversible"]),
        )
        .optional("read_back", JsonSchema::text())
        .optional("untrusted_fields", JsonSchema::list(JsonSchema::text()))
        .optional("result", JsonSchema::words(&["structured", "text"]))
        .optional("absent_error", JsonSchema::text())
        .optional("records", JsonSchema::text())
        .optional("key", JsonSchema::list(JsonSchema::text()))
        .optional("stamp", JsonSchema::text())
        .optional("key_args", JsonSchema::list(JsonSchema::text()))
        .optional("free_text", JsonSchema::bool())
}

/// The schema of one mapping document.
fn mapping_schema() -> JsonSchema {
    JsonSchema::object()
        .required("surface", JsonSchema::surface())
        .optional(
            "server",
            JsonSchema::object()
                .required("name", JsonSchema::text())
                .required("version", JsonSchema::scalar()),
        )
        .optional("purpose", JsonSchema::words(&["tools", "discovery"]))
        .required("tools", JsonSchema::map_of(tool_schema()))
}

/// The file kind `mapping`, in the folder `mappings/`.
pub(crate) struct MappingKind {
    schema: JsonSchema,
    kinds: Vec<SurfaceKind>,
}

impl MappingKind {
    pub(crate) fn new() -> Self {
        Self {
            schema: mapping_schema(),
            kinds: vec![names::surface_kind()],
        }
    }
}

fn texts(doc: Option<&Located<Doc>>) -> Vec<String> {
    doc.map(|list| {
        list.items()
            .iter()
            .filter_map(|item| item.as_text().map(str::to_owned))
            .collect()
    })
    .unwrap_or_default()
}

fn scalar_text(doc: &Located<Doc>) -> Option<String> {
    a2m_files::expr_text(doc)
}

/// Reads one tool's entry of a mapping document.
pub(crate) fn tool_entry(entry: &Located<Doc>) -> ToolEntry {
    let text = |key: &str| entry.text(key).map(str::to_owned);
    ToolEntry {
        digest: text("digest"),
        effect: text("effect").unwrap_or_else(|| "write".to_owned()),
        read_back: text("read_back"),
        untrusted_fields: texts(entry.get("untrusted_fields")),
        result: text("result").unwrap_or_else(|| "structured".to_owned()),
        absent_error: text("absent_error"),
        records: text("records"),
        key: texts(entry.get("key")),
        stamp: text("stamp"),
        key_args: texts(entry.get("key_args")),
        free_text: entry.get("free_text").and_then(|flag| flag.as_bool()),
    }
}

fn problem(at: &Place, message: String, fix: &str) -> Diagnostic {
    Diagnostic::error(names::MAPPING_CONFLICT, Stage::Lower, at, message, fix)
}

impl FileKind for MappingKind {
    fn name(&self) -> FileKindName {
        names::mapping_kind()
    }

    fn surface_kinds(&self) -> &[SurfaceKind] {
        &self.kinds
    }

    fn folder(&self) -> &str {
        names::MAPPING_FOLDER
    }

    fn schema(&self) -> &JsonSchema {
        &self.schema
    }

    /// Each mapped tool is a name, `surface/tool`, so the kernel refuses a
    /// tool mapped twice for one surface.
    fn names(&self, doc: &Located<Doc>) -> Vec<Located<DefinedName>> {
        let surface = doc.text("surface").unwrap_or_default();
        doc.get("tools")
            .map(|tools| {
                tools
                    .entries()
                    .iter()
                    .map(|entry| {
                        Located::new(
                            DefinedName {
                                name: format!("{surface}/{}", entry.key.value),
                            },
                            entry.key.at.clone(),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn expressions(&self, _doc: &Located<Doc>) -> Vec<ExprSite> {
        Vec::new()
    }

    fn compile(
        &self,
        docs: &[Located<Doc>],
        cx: &KindContext<'_>,
    ) -> Result<CompiledSection, Diagnostics> {
        let mut section = Section::default();
        let mut problems = Diagnostics::new();
        for doc in docs {
            let Some(surface) = cx.surface_of(doc) else {
                continue;
            };
            let mapping = section
                .surfaces
                .entry(surface.name.as_str().to_owned())
                .or_default();
            if let Some(purpose) = doc.get("purpose")
                && let Some(word) = purpose.as_text()
            {
                if mapping.purpose != word && !mapping.tools.is_empty() {
                    problems.push(problem(
                        &purpose.at,
                        format!(
                            "Two mapping documents of the surface `{}` state different purposes.",
                            surface.name
                        ),
                        "State one purpose for the surface: `tools` or `discovery`.",
                    ));
                }
                word.clone_into(&mut mapping.purpose);
            }
            if let Some(server) = doc.get("server") {
                mapping.server = Some(ServerSeen {
                    name: server.text("name").unwrap_or_default().to_owned(),
                    version: server
                        .get("version")
                        .and_then(scalar_text)
                        .unwrap_or_default(),
                });
            }
            for entry in doc.get("tools").map(Located::entries).unwrap_or_default() {
                mapping
                    .tools
                    .insert(entry.key.value.clone(), tool_entry(&entry.value));
            }
        }
        if !problems.is_empty() {
            return Err(problems);
        }
        PackPayload::encode(&section)
            .map(|payload| CompiledSection {
                format: SECTION_FORMAT,
                payload,
            })
            .map_err(|failure| {
                Diagnostics::from(problem(
                    &Place::start_of(&RelPath::target_file()),
                    failure.message().to_owned(),
                    "Keep every value of a mapping text, a truth value, or a list of text.",
                ))
            })
    }
}
