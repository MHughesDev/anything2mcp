//! The names the pack registers.

use a2m_core::{Failure, FileKindName, FindingKind, LintCode, Mode, StepKind, SurfaceKind, Verb};
use a2m_provider::DiscovererName;

/// The pack's name: the owner of its file kind, its lint codes, and its
/// section of a compiled target.
pub const PACK: &str = "mcp";

/// The folder of mapping files.
pub(crate) const MAPPING_FOLDER: &str = "mappings";

/// The surface kind `mcp`.
pub const SURFACE_KIND: SurfaceKind = SurfaceKind::fixed("mcp");
/// The file kind `mapping`.
pub const MAPPING_KIND: FileKindName = FileKindName::fixed("mapping");
/// The step kind that calls one mapped tool.
pub const CALL_TOOL: StepKind = StepKind::fixed("call_tool");
/// The step kind that lists the server's tools and calls none.
pub const LIST_TOOLS: StepKind = StepKind::fixed("list_tools");

/// The verbs the mode offers, where the server has a tool and the mapping
/// names it. `call` is the verb of a binding that states none.
pub(crate) const VERBS: [Verb; 10] = [
    Verb::fixed("call"),
    Verb::fixed("read"),
    Verb::fixed("list"),
    Verb::fixed("search"),
    Verb::fixed("create"),
    Verb::fixed("update"),
    Verb::fixed("upsert"),
    Verb::fixed("delete"),
    Verb::fixed("download"),
    Verb::fixed("submit"),
];

/// The verbs that read: they may be bound only to a tool mapped as a read.
pub(crate) const READ_VERBS: &[&str] = &["read", "list", "search", "download"];

pub(crate) const TOOL_UNMAPPED: LintCode = LintCode::fixed("mcp.tool_unmapped");
pub(crate) const TOOL_UNPINNED: LintCode = LintCode::fixed("mcp.tool_unpinned");
pub(crate) const PROGRAM_UNPINNED: LintCode = LintCode::fixed("mcp.program_unpinned");
pub(crate) const WRITE_NO_READBACK: LintCode = LintCode::fixed("mcp.write_no_readback");
pub(crate) const DISCOVERY_BOUND: LintCode = LintCode::fixed("mcp.discovery_bound");
pub(crate) const FREE_TEXT: LintCode = LintCode::fixed("mcp.free_text");
pub(crate) const ENVIRONMENT_REFUSED: LintCode = LintCode::fixed("mcp.environment_refused");
pub(crate) const UNENCRYPTED_CREDENTIALS: LintCode = LintCode::fixed("mcp.unencrypted_credentials");
pub(crate) const PACKAGE_RUNNER: LintCode = LintCode::fixed("mcp.package_runner");
pub(crate) const LAUNCHER_FORM: LintCode = LintCode::fixed("mcp.launcher_form");
/// Codes of the lowering and of the mapping's own compile step.
pub(crate) const EFFECT_UNDERSTATED: LintCode = LintCode::fixed("mcp.effect_understated");
pub(crate) const BINDING_INCOMPLETE: LintCode = LintCode::fixed("mcp.binding_incomplete");
pub(crate) const UNTRUSTED_UNMARKED: LintCode = LintCode::fixed("mcp.untrusted_unmarked");
pub(crate) const MAPPING_CONFLICT: LintCode = LintCode::fixed("mcp.mapping_conflict");

/// The surface kind `mcp`.
#[must_use]
pub fn surface_kind() -> SurfaceKind {
    SURFACE_KIND
}

/// The file kind `mapping`.
#[must_use]
pub fn mapping_kind() -> FileKindName {
    MAPPING_KIND
}

/// The names that have no constant form: the mode, the finding kinds, and
/// the discoverer. Made once, when the pack registers.
#[derive(Debug, Clone)]
pub struct Names {
    /// The mode `mcp`.
    pub mode: Mode,
    /// The finding kind of one listed tool.
    pub upstream_tool: FindingKind,
    /// The finding kind of the server itself.
    pub upstream_server: FindingKind,
    /// The discoverer `mcp_tools`.
    pub discoverer: DiscovererName,
}

impl Names {
    /// The pack's names.
    ///
    /// # Errors
    ///
    /// Never in practice: every name is written in this file and a test
    /// builds them. The error is the kernel's, for a name that is not one.
    pub fn new() -> Result<Self, Failure> {
        Ok(Self {
            mode: Mode::new("mcp")?,
            upstream_tool: FindingKind::new("upstream_tool")?,
            upstream_server: FindingKind::new("upstream_server")?,
            discoverer: DiscovererName::new("mcp_tools")?,
        })
    }
}

/// The name of the auth provider.
#[must_use]
pub fn auth_provider() -> a2m_auth::ProviderName {
    a2m_auth::ProviderName::fixed("upstream_mcp")
}

#[cfg(test)]
mod tests {
    use super::Names;

    #[test]
    fn every_name_of_the_pack_is_well_formed() {
        let names = Names::new().unwrap();
        assert_eq!(names.mode.as_str(), "mcp");
        assert_eq!(names.discoverer.as_str(), "mcp_tools");
        assert_eq!(super::auth_provider().as_str(), "upstream_mcp");
    }
}
