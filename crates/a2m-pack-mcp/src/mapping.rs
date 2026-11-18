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
/// The surface field that accepts credentials over an unencrypted link to
/// another machine (decision 0188), and the one word it takes.
pub(crate) const UNENCRYPTED_CREDENTIALS: &str = "unencrypted_credentials";
pub(crate) const ACCEPTED: &str = "accepted";

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
            )
            .optional(UNENCRYPTED_CREDENTIALS, JsonSchema::words(&[ACCEPTED])),
    )
    // A door shows a person that the surface sends a credential over an
    // unencrypted link (decisions 0193 and 0194).
    .with_notice(
        UNENCRYPTED_CREDENTIALS,
        ACCEPTED,
        a2m_core::SurfaceNotice::UnencryptedCredentials,
    )
    .with_program("program", Some("digest"))
}

/// Words that name a credential when they end a variable's name. A copy of
/// `CREDENTIAL_WORDS` in `a2m-pack-command` (`src/kind.rs`): the two packs
/// cannot share the code, so the two rules are kept word for word the same.
const CREDENTIAL_WORDS: &[&str] = &[
    "token",
    "secret",
    "password",
    "passwd",
    "passphrase",
    "apikey",
    "credential",
    "credentials",
];

/// Words that name a credential when `key` follows them. A copy of
/// `CREDENTIAL_KEYS` in `a2m-pack-command`.
const CREDENTIAL_KEYS: &[&str] = &[
    "api",
    "access",
    "private",
    "secret",
    "signing",
    "encryption",
];

/// Whether a variable's name says that it holds a credential. A copy of
/// `names_a_credential` in `a2m-pack-command` (`src/kind.rs`), word for
/// word the same rule. Only the end of the name counts.
fn names_a_credential(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect();
    match words.as_slice() {
        [.., last] if CREDENTIAL_WORDS.contains(last) => true,
        [.., before, "key"] => CREDENTIAL_KEYS.contains(before),
        _ => false,
    }
}

/// Why a variable may not be handed to a local server program, when it may
/// not: it is one of the engine's own, or it is named for a credential. A
/// credential for a local server goes through the session keeper, never
/// through the environment.
pub(crate) fn refused_variable(name: &str) -> Option<&'static str> {
    if a2m_provider::is_engine_variable(name) {
        Some("belongs to the engine, whose own variables are never handed to a program")
    } else if names_a_credential(name) {
        Some("is named for a credential, which never goes through a program's environment")
    } else {
        None
    }
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
    /// `accepted` when the surface accepts that its credentials cross an
    /// unencrypted link to another machine.
    #[serde(default)]
    pub unencrypted_credentials: Option<String>,
}

/// Whether an address is plain `http` to an address that is not a loopback
/// address written as an IP address. A name, `localhost` included, is not
/// one: the machine's resolver decides where a name leads. The host is
/// judged by `a2m_core::ScopedHost::may_leave_this_machine`, the one
/// question every check of a plain link asks (decision 0240), as the URL
/// parser wrote it. An address the parser cannot read counts as plain to
/// another machine, whatever it starts with: nothing can be told of it.
pub(crate) fn plain_to_another_machine(address: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(address) else {
        return true;
    };
    url.scheme() == "http"
        && a2m_core::ScopedHost::of_host(url.host_str().unwrap_or_default())
            .may_leave_this_machine()
}

impl SurfaceFields {
    /// Whether the surface accepts that its credentials cross an
    /// unencrypted link to another machine.
    pub(crate) fn accepts_unencrypted(&self) -> bool {
        self.unencrypted_credentials.as_deref() == Some(ACCEPTED)
    }

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

    /// A mapping describes a write when any of its tools is mapped as
    /// anything but `read`. The effect is a person's judgment of the tool.
    /// An effect the drafter left as an open question states nothing: the
    /// file does not compile until a person answers it, and a mapping alone
    /// calls nothing. An effect that is some other word is not known to
    /// read.
    fn writes(&self, doc: &Located<Doc>) -> bool {
        let Some(tools) = doc.get("tools") else {
            return false;
        };
        tools.entries().iter().any(|entry| {
            let effect = entry.value.get("effect");
            let open = effect.is_some_and(|effect| {
                matches!(effect.entries(), [only] if only.key.value == a2m_files::OPEN_QUESTION)
            });
            !open && effect.and_then(Located::as_text) != Some("read")
        })
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use a2m_files::{
        CompilerSet, DraftContext, DraftDoc, DraftValue, Drafter, DraftingRule, FileKind, RelPath,
        TargetRoot, parse_yaml,
    };

    use super::MappingKind;
    use crate::names::{self, Names};

    #[test]
    fn a_mapping_describes_a_write_when_a_tool_is_mapped_as_anything_but_read() {
        let kind = MappingKind::new();
        let file = RelPath::new("mappings/one.yaml").unwrap();
        let writes = |effects: &[&str]| {
            let mut text = String::from("surface: orders\ntools:\n");
            for (n, effect) in effects.iter().enumerate() {
                text.push_str(&format!("  t{n}:\n    effect: {effect}\n"));
            }
            let docs = parse_yaml(&file, &text).unwrap();
            kind.writes(&docs[0])
        };
        assert!(!writes(&["read", "read"]));
        assert!(writes(&["read", "write"]));
        assert!(writes(&["irreversible"]));
        assert!(writes(&["\"maybe\""]));
        // What the drafter leaves for a person states no effect.
        assert!(!writes(&["{open_question: \"Does it write?\"}"]));
    }

    /// A rule of this pack that drafts a tool mapped as a write.
    struct DraftsAWrite(a2m_core::FindingKind);

    impl DraftingRule for DraftsAWrite {
        fn finding_kind(&self) -> a2m_core::FindingKind {
            self.0.clone()
        }
        fn file_kind(&self) -> a2m_core::FileKindName {
            names::MAPPING_KIND
        }
        fn draft(
            &self,
            _finding: &a2m_core::Finding,
            _cx: &DraftContext<'_>,
        ) -> Result<Vec<DraftDoc>, a2m_core::Failure> {
            let tool = DraftValue::map(vec![("effect", DraftValue::text("write"))]);
            Ok(vec![DraftDoc {
                path: RelPath::new("mappings/orders.yaml")?,
                body: DraftValue::map(vec![
                    ("surface", DraftValue::text("orders")),
                    ("tools", DraftValue::map(vec![("orders_delete", tool)])),
                ]),
            }])
        }
    }

    #[test]
    fn a_drafting_rule_that_drafts_a_write_is_refused_and_nothing_is_written() {
        let found = Names::new().unwrap().upstream_tool;
        let mut builder = CompilerSet::builder();
        builder
            .file_kind(names::PACK, Arc::new(MappingKind::new()))
            .unwrap();
        builder
            .drafting_rule(names::PACK, Arc::new(DraftsAWrite(found.clone())))
            .unwrap();
        let set = builder.build();
        let folder =
            std::env::temp_dir().join(format!("a2m-pack-mcp-draft-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let finding = a2m_core::Finding::about(
            a2m_core::Subject::File {
                digest: a2m_core::Digest::of(b"tool"),
                format: "mcp".to_owned(),
                part: None,
            },
            found,
            "{}",
            a2m_core::Source::LiveSystem,
            a2m_core::ProducedBy {
                provider: "mcp".to_owned(),
                version: "0.0.1".to_owned(),
                component: None,
            },
            a2m_core::Timestamp::from_unix_millis(1_000),
        )
        .build()
        .unwrap();
        let root = TargetRoot::new(&folder);
        let report = Drafter::new(&set, &root).draft(&[finding]).unwrap();
        assert!(report.written.is_empty(), "{report:?}");
        assert_eq!(report.refused.len(), 1, "{report:?}");
        assert!(!folder.join("mappings").join("orders.yaml").exists());
        std::fs::remove_dir_all(&folder).unwrap();
    }
}

#[cfg(test)]
mod link_tests {
    use super::plain_to_another_machine;

    /// The corpus of address spellings every check of where an address is
    /// is pinned against (decision 0240).
    const SPELLINGS: &str = include_str!("../../a2m-core/tests/address-spellings.txt");

    /// Each spelling of the corpus, with the verdict of `column` now and
    /// before decision 0240.
    fn corpus(column: &str) -> Vec<(String, String, String)> {
        let mut header: Vec<&str> = Vec::new();
        let mut rows = Vec::new();
        for line in SPELLINGS.lines() {
            if let Some(names) = line.strip_prefix("#spelling") {
                header = std::iter::once("spelling")
                    .chain(names.split_whitespace())
                    .collect();
            }
            if line.starts_with('#') || line.trim().is_empty() {
                continue;
            }
            let cells: Vec<&str> = line.split_whitespace().collect();
            let at = header.iter().position(|name| *name == column).unwrap();
            let spelling = cells[0].replace("<empty>", "").replace("<sp>", " ");
            let (now, before) = cells[at].split_once('<').unwrap_or((cells[at], cells[at]));
            rows.push((spelling, now.to_owned(), before.to_owned()));
        }
        assert!(rows.len() > 50, "the corpus was not read");
        rows
    }

    /// `plain_to_another_machine` as it was before decision 0240, kept to
    /// show that nothing it refused is allowed now.
    fn before(address: &str) -> bool {
        let Ok(url) = reqwest::Url::parse(address) else {
            return address.starts_with("http://");
        };
        if url.scheme() != "http" {
            return false;
        }
        let host = url.host_str().unwrap_or_default();
        let bare = host
            .strip_prefix('[')
            .and_then(|inner| inner.strip_suffix(']'))
            .unwrap_or(host);
        !bare
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
    }

    fn verdict(judge: impl Fn(&str) -> bool, address: &str) -> &'static str {
        if judge(address) { "away" } else { "here" }
    }

    #[test]
    fn every_spelling_of_the_shared_corpus_is_judged_as_it_is_pinned() {
        for (spelling, now, before_then) in corpus("mcp") {
            let address = format!("http://{spelling}/");
            assert_eq!(
                verdict(plain_to_another_machine, &address),
                now,
                "{spelling}"
            );
            assert_eq!(verdict(before, &address), before_then, "{spelling}");
        }
    }

    #[test]
    fn no_spelling_refused_before_the_one_question_is_allowed_after_it() {
        // Every scheme, and an address the parser cannot read that does not
        // start with `http://`, which was allowed before and is refused now.
        for (spelling, _, _) in corpus("mcp") {
            for scheme in ["http://", "https://", "HTTP://", ""] {
                let address = format!("{scheme}{spelling}");
                if before(&address) {
                    assert!(plain_to_another_machine(&address), "{address}");
                }
            }
        }
        for unreadable in ["HTTP://[::1", "http:/[::1", ""] {
            assert!(!before(unreadable), "{unreadable}");
            assert!(plain_to_another_machine(unreadable), "{unreadable}");
        }
    }
}
