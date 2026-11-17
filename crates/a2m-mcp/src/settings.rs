//! What an endpoint is started with.

use a2m_actions::Settings;
use a2m_core::Failure;

use crate::codes;

/// A revision of MCP that this crate serves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Revision {
    /// 2025-11-25: the revision with the `initialize` handshake.
    Handshake,
    /// 2026-07-28: the stateless revision, where every request names its
    /// revision.
    Stateless,
}

impl Revision {
    /// Every revision served, the newest first.
    pub const ALL: [Self; 2] = [Self::Stateless, Self::Handshake];

    /// The revision's date, as the protocol writes it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Handshake => "2025-11-25",
            Self::Stateless => "2026-07-28",
        }
    }

    /// The revision with this date, when it is one this crate serves.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|known| known.as_str() == text)
    }
}

/// Which of the targets' published tools an endpoint lists up front.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Listing {
    /// Pipelines, up to the limit. Operations are found with the search
    /// tool and called by name.
    Default,
    /// Every operation and pipeline, for clients with their own tool
    /// search.
    Full,
}

/// The settings of one endpoint. Read once, at start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpSettings {
    /// `mcp.revisions`: the revisions served. Never empty.
    pub revisions: Vec<Revision>,
    /// `tools.full_listing`.
    pub listing: Listing,
    /// `tools.listed_limit`: how many tools are listed up front, the
    /// built-in ones included. Ignored by a full listing, which stops at
    /// [`McpSettings::FULL_CEILING`].
    pub listed_max: usize,
    /// The `Host` values a request may carry. Any other is refused before
    /// the request is read. A port may be given; a name without one allows
    /// every port.
    pub allowed_hosts: Vec<String>,
    /// The largest request body read, in bytes.
    pub max_request_bytes: usize,
}

impl McpSettings {
    /// The most tools a full listing holds.
    pub const FULL_CEILING: usize = 200;

    /// The settings of an endpoint on one machine: both revisions, the
    /// default listing, and loopback hosts only.
    #[must_use]
    pub fn local() -> Self {
        Self {
            revisions: Revision::ALL.to_vec(),
            listing: Listing::Default,
            listed_max: 19,
            allowed_hosts: vec![
                "localhost".to_owned(),
                "127.0.0.1".to_owned(),
                "::1".to_owned(),
            ],
            max_request_bytes: 4 * 1024 * 1024,
        }
    }

    /// The same, with `mcp.revisions`, `tools.listed_limit`, and
    /// `tools.full_listing` as the installation's settings give them.
    ///
    /// # Errors
    ///
    /// `input`/`mcp_not_served` when `mcp.revisions` names a revision this
    /// crate does not serve, or none.
    pub fn from_settings(settings: &Settings) -> Result<Self, Failure> {
        let mut out = Self::local();
        out.revisions = Vec::new();
        for text in settings.list("mcp.revisions") {
            let revision = Revision::parse(&text).ok_or_else(|| {
                codes::input(
                    codes::NOT_SERVED,
                    format!("`mcp.revisions` names `{text}`, which is not served."),
                    "Set `mcp.revisions` to `2026-07-28`, `2025-11-25`, or both.",
                )
            })?;
            if !out.revisions.contains(&revision) {
                out.revisions.push(revision);
            }
        }
        if let Some(limit) = settings.int("tools.listed_limit") {
            out.listed_max = usize::try_from(limit).unwrap_or(out.listed_max);
        }
        if settings.flag("tools.full_listing") == Some(true) {
            out.listing = Listing::Full;
        }
        out.checked()
    }

    /// The settings, when they can be served.
    pub(crate) fn checked(self) -> Result<Self, Failure> {
        if self.revisions.is_empty() {
            return Err(codes::input(
                codes::NOT_SERVED,
                "No revision of MCP is set to be served.",
                "Set `mcp.revisions` to `2026-07-28`, `2025-11-25`, or both.",
            ));
        }
        Ok(self)
    }

    /// Whether `revision` is served.
    #[must_use]
    pub fn serves(&self, revision: Revision) -> bool {
        self.revisions.contains(&revision)
    }
}
