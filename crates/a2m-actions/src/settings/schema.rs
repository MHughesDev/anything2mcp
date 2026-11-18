//! The one schema of every setting (spec section 226).
//!
//! Each setting is declared once: its key, type, default for each profile,
//! scope, whether it is protected and which direction is safer, when a
//! change takes effect, and its description. Start-up, `config.show`, and
//! the reference pages all read this table.

use a2m_core::Value;

/// The type of a setting's value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// True or false.
    Bool,
    /// A whole number within a range.
    Int {
        /// The least it may be.
        min: i64,
        /// The most it may be.
        max: i64,
    },
    /// Text.
    Text,
    /// One of a closed list of words.
    Word(&'static [&'static str]),
    /// A list of texts.
    List,
    /// A list of words from a closed list.
    Words(&'static [&'static str]),
}

impl Kind {
    /// The type in words, for a message.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::Bool => "true or false".to_owned(),
            Self::Int { min, max } => format!("a whole number from {min} to {max}"),
            Self::Text => "text".to_owned(),
            Self::Word(words) => format!("one of: {}", words.join(", ")),
            Self::List => "a list of texts".to_owned(),
            Self::Words(words) => format!("a list of: {}", words.join(", ")),
        }
    }

    /// Whether `value` is one this type takes.
    #[must_use]
    pub fn fits(&self, value: &Value) -> bool {
        match (self, value) {
            (Self::Bool, Value::Bool(_)) | (Self::Text, Value::Text(_)) => true,
            (Self::Int { min, max }, Value::Int(number)) => number >= min && number <= max,
            (Self::Word(words), Value::Text(word)) => words.contains(&word.as_str()),
            (Self::List, Value::List(items)) => items.iter().all(|item| item.as_text().is_some()),
            (Self::Words(words), Value::List(items)) => items
                .iter()
                .all(|item| item.as_text().is_some_and(|word| words.contains(&word))),
            _ => false,
        }
    }
}

/// Where a setting may be set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Scope {
    /// The installation only: the installation file, and the flags of
    /// `serve`.
    Installation,
    /// A user and above: not the project file.
    User,
    /// A project and above: every layer.
    Project,
}

impl Scope {
    /// The scope's letter in the tables of spec section 226.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Installation => "installation",
            Self::User => "user",
            Self::Project => "project",
        }
    }
}

/// Which direction of a protected setting is safer. A value on the other
/// side is a loosening, which only a person can choose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Safer {
    /// A lower number is safer.
    Lower,
    /// A higher number is safer.
    Higher,
    /// False is safer.
    Off,
    /// True is safer.
    On,
    /// A list with fewer entries is safer: adding one loosens.
    Fewer,
    /// Empty text is safer: any other value loosens.
    Empty,
    /// The words in order, safest first.
    Rank(&'static [&'static str]),
    /// Unknown: any change is treated as a loosening.
    Unknown,
}

impl Safer {
    /// The direction a pack's manifest names in a word.
    #[must_use]
    pub fn from_word(word: &str) -> Self {
        match word {
            "lower" => Self::Lower,
            "higher" => Self::Higher,
            "off" | "false" => Self::Off,
            "on" | "true" => Self::On,
            "fewer" => Self::Fewer,
            "empty" => Self::Empty,
            _ => Self::Unknown,
        }
    }

    /// Whether moving from `from` to `to` loosens.
    #[must_use]
    pub fn loosens(&self, from: &Value, to: &Value) -> bool {
        if from == to {
            return false;
        }
        match (self, from, to) {
            (Self::Lower, Value::Int(from), Value::Int(to)) => to > from,
            (Self::Higher, Value::Int(from), Value::Int(to)) => to < from,
            (Self::Off, _, Value::Bool(to)) => *to,
            (Self::On, _, Value::Bool(to)) => !*to,
            (Self::Fewer, Value::List(from), Value::List(to)) => {
                to.iter().any(|item| !from.contains(item))
            }
            (Self::Empty, _, Value::Text(to)) => !to.is_empty(),
            (Self::Rank(order), Value::Text(from), Value::Text(to)) => {
                let place = |word: &str| order.iter().position(|known| *known == word);
                match (place(from), place(to)) {
                    (Some(from), Some(to)) => to > from,
                    _ => true,
                }
            }
            _ => true,
        }
    }
}

/// When a change to a setting takes effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effective {
    /// At once.
    AtOnce,
    /// At the next start of the engine.
    NextStart,
}

impl Effective {
    /// The words.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AtOnce => "at once",
            Self::NextStart => "at the next start of the engine",
        }
    }
}

/// A setting's default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefaultValue {
    /// False or true.
    Bool(bool),
    /// A whole number.
    Int(i64),
    /// Text or a word.
    Text(&'static str),
    /// A list.
    List(&'static [&'static str]),
}

impl DefaultValue {
    /// The default as a value.
    #[must_use]
    pub fn value(&self) -> Value {
        match self {
            Self::Bool(flag) => Value::Bool(*flag),
            Self::Int(number) => Value::Int(*number),
            Self::Text(text) => Value::Text((*text).to_owned()),
            Self::List(items) => Value::List(
                items
                    .iter()
                    .map(|item| Value::Text((*item).to_owned()))
                    .collect(),
            ),
        }
    }
}

/// One setting. A key may hold one `*` part, which stands for a name that
/// arrives with a pack, such as a mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettingDef {
    /// The dotted key.
    pub key: &'static str,
    /// The type of its value.
    pub kind: Kind,
    /// The value when nothing sets it.
    pub default: DefaultValue,
    /// Where it may be set.
    pub scope: Scope,
    /// Its safer direction, when it is protected.
    pub protected: Option<Safer>,
    /// When a change takes effect.
    pub effective: Effective,
    /// What it does, in a sentence.
    pub doc: &'static str,
}

impl SettingDef {
    /// Whether `key` is this setting: the same parts, with `*` standing for
    /// any one name.
    #[must_use]
    pub fn matches(&self, key: &str) -> bool {
        let mut own = self.key.split('.');
        let mut asked = key.split('.');
        loop {
            match (own.next(), asked.next()) {
                (None, None) => return true,
                (Some("*"), Some(part)) if is_name(part) => {}
                (Some(own), Some(asked)) if own == asked => {}
                _ => return false,
            }
        }
    }

    /// Whether the key holds a `*` part, and so has no default of its own.
    #[must_use]
    pub fn is_pattern(&self) -> bool {
        self.key.split('.').any(|part| part == "*")
    }
}

fn is_name(part: &str) -> bool {
    part.bytes().next().is_some_and(|b| b.is_ascii_lowercase())
        && part
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// The deployment profiles (spec section 601).
pub const PROFILES: &[&str] = &[
    "solo",
    "workstation",
    "team",
    "company",
    "hybrid",
    "cluster",
    "embedded",
];

const fn def(
    key: &'static str,
    kind: Kind,
    default: DefaultValue,
    scope: Scope,
    effective: Effective,
    doc: &'static str,
) -> SettingDef {
    SettingDef {
        key,
        kind,
        default,
        scope,
        protected: None,
        effective,
        doc,
    }
}

const fn protected(mut setting: SettingDef, safer: Safer) -> SettingDef {
    setting.protected = Some(safer);
    setting
}

const COUNT: Kind = Kind::Int {
    min: 0,
    max: 1_000_000,
};
const BYTES: Kind = Kind::Int {
    min: 0,
    max: i64::MAX,
};
use DefaultValue::{Bool as B, Int as N, List as L, Text as T};
use Effective::{AtOnce, NextStart};
use Scope::{Installation as I, Project as P, User as U};

/// Every setting of the kernel, in the order of spec section 226.
pub const SCHEMA: &[SettingDef] = &[
    // Profile and roles.
    def(
        "profile.name",
        Kind::Word(PROFILES),
        T("solo"),
        I,
        NextStart,
        "The deployment profile: the shape this installation runs in.",
    ),
    def(
        "profile.roles",
        Kind::Words(&["control_plane", "runner"]),
        L(&["control_plane", "runner"]),
        I,
        NextStart,
        "Which roles this process takes.",
    ),
    def(
        "profile.preset",
        Kind::Text,
        T("standard"),
        U,
        AtOnce,
        "A named bundle of values: a built-in preset, or a site's own.",
    ),
    // Storage, the vault, and files.
    def(
        "store.database",
        Kind::Text,
        T(""),
        I,
        NextStart,
        "Where the store is. Empty means one file in the data directory.",
    ),
    def(
        "store.blobs",
        Kind::Text,
        T(""),
        I,
        NextStart,
        "Where evidence blobs are kept. Empty means the data directory.",
    ),
    def(
        "store.free_space_floor",
        BYTES,
        N(1_073_741_824),
        I,
        AtOnce,
        "The free space, in bytes, below which the engine is degraded. A draft.",
    ),
    def(
        "vault.backend",
        Kind::Text,
        // A fact of the build, never chosen at run time (decision 0220).
        T(a2m_vault::DEFAULT_BACKEND),
        I,
        NextStart,
        "Where the vault's root key is kept: the operating system's key store, a key derived from a passphrase (the default where this build has no key store), or a key service by reference.",
    ),
    def(
        "vault.root_key",
        Kind::Text,
        T(""),
        I,
        NextStart,
        "A reference to the root key. Empty means the key store's own.",
    ),
    protected(
        def(
            "files.allowed_roots",
            Kind::List,
            L(&["<project>"]),
            U,
            AtOnce,
            "The named folders files may be read from and written to.",
        ),
        Safer::Fewer,
    ),
    def(
        "files.max_upload_bytes",
        BYTES,
        N(104_857_600),
        U,
        AtOnce,
        "The largest file a caller may hand in, in bytes. A draft.",
    ),
    // Policy.
    protected(
        def(
            "policy.approve_writes",
            Kind::Word(&["named_approver", "person_each_run", "caller_confirms"]),
            T("person_each_run"),
            U,
            AtOnce,
            "Who must approve a write.",
        ),
        Safer::Rank(&["named_approver", "person_each_run", "caller_confirms"]),
    ),
    def(
        "policy.two_person_irreversible",
        Kind::Bool,
        B(false),
        U,
        AtOnce,
        "Whether an irreversible operation needs two people.",
    ),
    protected(
        def(
            "policy.max_records_per_run",
            Kind::Int {
                min: 1,
                max: 1_000_000,
            },
            N(50),
            U,
            AtOnce,
            "The most records one run may write.",
        ),
        Safer::Lower,
    ),
    protected(
        def(
            "policy.approval_window_minutes",
            // One day: the policy engine's own ceiling, so that no value the
            // schema takes is one the policy engine refuses (decision 0145).
            Kind::Int { min: 1, max: 1_440 },
            N(15),
            U,
            AtOnce,
            "How long an approval may be used after it is given.",
        ),
        Safer::Lower,
    ),
    protected(
        def(
            "policy.preview_freshness_seconds",
            Kind::Int {
                min: 1,
                max: 86_400,
            },
            N(60),
            U,
            AtOnce,
            "How old a preview may be when its writes begin.",
        ),
        Safer::Lower,
    ),
    protected(
        def(
            "policy.agent_effect_ceiling",
            Kind::Word(&["read", "write", "irreversible"]),
            T("irreversible"),
            U,
            AtOnce,
            "The highest effect class a grant gives an agent, whatever the grant says.",
        ),
        Safer::Rank(&["read", "write", "irreversible"]),
    ),
    protected(
        def(
            "policy.pre_approval",
            Kind::Bool,
            B(true),
            U,
            AtOnce,
            "Whether a pre-approval rule may let a write proceed without a person each run.",
        ),
        Safer::Off,
    ),
    protected(
        def(
            "policy.writes_outside_tests",
            Kind::Bool,
            B(true),
            U,
            AtOnce,
            "Whether writes may go anywhere but declared test records in a test environment.",
        ),
        Safer::Off,
    ),
    // Identity and sessions.
    def(
        "identity.default_mode",
        Kind::Word(&[
            "per_user_token",
            "per_user_session",
            "per_user_device",
            "service_account",
        ]),
        T("per_user_session"),
        I,
        NextStart,
        "How the identity a run acts as is obtained.",
    ),
    def(
        "identity.allowed_modes",
        Kind::Words(&[
            "per_user_token",
            "per_user_session",
            "per_user_device",
            "service_account",
        ]),
        L(&["per_user_session"]),
        I,
        NextStart,
        "The identity modes a toolset may use.",
    ),
    def(
        "sessions.keep_alive_hours",
        Kind::Int { min: 0, max: 8_760 },
        N(8),
        U,
        AtOnce,
        "How long after its last use a session is still checked.",
    ),
    def(
        "sessions.probe_minutes",
        Kind::Int { min: 1, max: 1_440 },
        N(15),
        U,
        AtOnce,
        "How often a session is probed when its auth file declares no idle timeout.",
    ),
    def(
        "sessions.login_wait_minutes",
        Kind::Int { min: 0, max: 1_440 },
        N(5),
        U,
        AtOnce,
        "How long an interactive call waits for a sign-in.",
    ),
    def(
        "sessions.attempt_limit",
        Kind::Int { min: 1, max: 3 },
        N(1),
        I,
        AtOnce,
        "How many times a stored credential is tried.",
    ),
    // The engine.
    def(
        "engine.idle_stop_minutes",
        Kind::Int {
            min: 0,
            max: 10_080,
        },
        N(30),
        U,
        AtOnce,
        "How long the local engine stays up with nothing to do.",
    ),
    def(
        "engine.in_process",
        Kind::Bool,
        B(false),
        U,
        NextStart,
        "Whether a command runs the engine inside its own process.",
    ),
    def(
        "engine.person",
        Kind::Word(&["present", "absent"]),
        T("present"),
        U,
        AtOnce,
        "Whether work that needs a person waits for one, or stops at once with its named state.",
    ),
    def(
        "engine.retries.transient",
        Kind::Int { min: 0, max: 5 },
        N(2),
        U,
        AtOnce,
        "How often a read or an idempotent step is tried again after a `transient` failure.",
    ),
    def(
        "engine.time_limit_ceiling_seconds",
        Kind::Int {
            min: 1,
            max: 86_400,
        },
        N(600),
        I,
        AtOnce,
        "The longest step time limit a target may declare here. A draft.",
    ),
    protected(
        def(
            "engine.shadow.matches_to_promote.read",
            Kind::Int {
                min: 1,
                max: 100_000,
            },
            N(5),
            I,
            AtOnce,
            "The matching reads a shadow binding needs before a person may promote it.",
        ),
        Safer::Higher,
    ),
    protected(
        def(
            "engine.shadow.matches_to_promote.write",
            Kind::Int {
                min: 1,
                max: 100_000,
            },
            N(20),
            I,
            AtOnce,
            "The matching writes a shadow binding needs before a person may promote it.",
        ),
        Safer::Higher,
    ),
    def(
        "engine.shadow.time_limit_ms",
        Kind::Int {
            min: 100,
            max: 60_000,
        },
        N(2_000),
        U,
        NextStart,
        "How long a binding in shadow may read beside a live read before it is left uncompared.",
    ),
    def(
        "engine.promoted_writes",
        Kind::Word(&["after_promotion", "never"]),
        T("after_promotion"),
        U,
        AtOnce,
        "Whether writes may use a binding promoted from shadow.",
    ),
    def(
        "ledger.intent_window.*",
        Kind::Int { min: 1, max: 1_000 },
        N(1),
        U,
        AtOnce,
        "How many intents of one mode may be open at once, up to its manifest's value.",
    ),
    def(
        "modes.disabled",
        Kind::List,
        L(&[]),
        U,
        AtOnce,
        "Modes that may not be used here.",
    ),
    // Scheduling.
    def(
        "scheduler.*.session_reads",
        COUNT,
        N(0),
        U,
        AtOnce,
        "Reads at once on one session of a mode, up to its manifest's value.",
    ),
    def(
        "scheduler.*.session_writes",
        COUNT,
        N(0),
        U,
        AtOnce,
        "Writes at once on one session of a mode, up to its manifest's value.",
    ),
    def(
        "scheduler.queue_length_per_target",
        Kind::Int {
            min: 1,
            max: 100_000,
        },
        N(100),
        U,
        AtOnce,
        "How many jobs may wait for one target.",
    ),
    def(
        "scheduler.priority",
        Kind::Word(&["interactive_first", "fifo"]),
        T("interactive_first"),
        I,
        AtOnce,
        "Which waiting job is served first.",
    ),
    def(
        "runners.lease_seconds",
        Kind::Int { min: 5, max: 3_600 },
        N(30),
        I,
        NextStart,
        "How long a job's lease lasts without a heartbeat.",
    ),
    def(
        "runners.heartbeat_seconds",
        Kind::Int { min: 1, max: 600 },
        N(10),
        I,
        NextStart,
        "How often a runner sends a heartbeat.",
    ),
    def(
        "runners.quarantine_seconds",
        Kind::Int {
            min: 0,
            max: 86_400,
        },
        N(60),
        I,
        NextStart,
        "How long the locks of a lost run are kept from others.",
    ),
    def(
        "runners.certificate_hours",
        Kind::Int { min: 1, max: 8_760 },
        N(24),
        I,
        NextStart,
        "How long a runner's certificate lasts.",
    ),
    // Authoring and the scout.
    def(
        "authoring.scout.run",
        Kind::Word(&["off", "on_request", "on_add"]),
        T("on_add"),
        P,
        AtOnce,
        "When the scout runs: never, when asked, or when a target is added.",
    ),
    def(
        "authoring.scout.try_reads",
        Kind::Bool,
        B(true),
        P,
        AtOnce,
        "Whether the scout tries the reads it drafted.",
    ),
    def(
        "authoring.scout.budget.seconds",
        Kind::Int {
            min: 1,
            max: 86_400,
        },
        N(300),
        P,
        AtOnce,
        "The time one scout may take. A target until measured.",
    ),
    def(
        "authoring.scout.budget.requests",
        Kind::Int {
            min: 1,
            max: 1_000_000,
        },
        N(500),
        P,
        AtOnce,
        "The requests one scout may send. A target until measured.",
    ),
    def(
        "authoring.scout.budget.depth",
        Kind::Int { min: 1, max: 100 },
        N(3),
        P,
        AtOnce,
        "How deep one scout follows what it finds. A target until measured.",
    ),
    def(
        "authoring.scout.providers",
        Kind::List,
        L(&[]),
        P,
        AtOnce,
        "Optional discoverers, by name. First-party discoverers are always available.",
    ),
    def(
        "authoring.capture.enabled",
        Kind::Bool,
        B(true),
        P,
        AtOnce,
        "Whether recording may be started at all.",
    ),
    protected(
        def(
            "authoring.capture.values",
            Kind::Bool,
            B(false),
            P,
            AtOnce,
            "Whether a recording keeps values, not only shapes.",
        ),
        Safer::Off,
    ),
    def(
        "authoring.capture.limits.seconds",
        Kind::Int {
            min: 1,
            max: 86_400,
        },
        N(600),
        P,
        AtOnce,
        "The longest one recording may run. A draft.",
    ),
    def(
        "authoring.capture.limits.bytes",
        BYTES,
        N(52_428_800),
        P,
        AtOnce,
        "The most one recording may keep, in bytes. A draft.",
    ),
    def(
        "authoring.capture.limits.exchanges",
        Kind::Int {
            min: 1,
            max: 1_000_000,
        },
        N(2_000),
        P,
        AtOnce,
        "The most exchanges one recording may keep. A draft.",
    ),
    // Evidence and telemetry.
    def(
        "evidence.capture",
        Kind::Word(&["minimal", "standard", "full"]),
        T("minimal"),
        U,
        AtOnce,
        "How much evidence a read keeps.",
    ),
    protected(
        def(
            "evidence.capture_on_writes",
            Kind::Word(&["standard", "full"]),
            T("standard"),
            U,
            AtOnce,
            "How much evidence a write keeps.",
        ),
        Safer::Rank(&["full", "standard"]),
    ),
    def(
        "evidence.trace_retention_days",
        Kind::Int {
            min: 1,
            max: 36_500,
        },
        N(30),
        U,
        AtOnce,
        "How long traces are kept.",
    ),
    protected(
        def(
            "evidence.audit_retention_days",
            Kind::Int {
                min: 1,
                max: 36_500,
            },
            N(365),
            I,
            AtOnce,
            "How long the audit log is kept.",
        ),
        Safer::Higher,
    ),
    def(
        "evidence.redaction.extra",
        Kind::List,
        L(&[]),
        U,
        AtOnce,
        "Further patterns and field names to mask, added to the built-in rules.",
    ),
    protected(
        def(
            "telemetry.export",
            Kind::Text,
            T(""),
            I,
            NextStart,
            "An OpenTelemetry endpoint spans and metrics are sent to. Empty sends nothing.",
        ),
        Safer::Empty,
    ),
    protected(
        def(
            "telemetry.crash_reports",
            Kind::Bool,
            B(false),
            I,
            NextStart,
            "Whether a crash report is sent to the project.",
        ),
        Safer::Off,
    ),
    // Front doors.
    def(
        "server.listen",
        Kind::Text,
        T(""),
        I,
        NextStart,
        "The address the server listens on. Empty opens no port.",
    ),
    def(
        "server.tls.certificate",
        Kind::Text,
        T(""),
        I,
        NextStart,
        "A reference to the server's certificate.",
    ),
    def(
        "server.tls.key",
        Kind::Text,
        T(""),
        I,
        NextStart,
        "A reference to the certificate's key.",
    ),
    def(
        "server.base_url",
        Kind::Text,
        T(""),
        I,
        NextStart,
        "The address callers reach the server by.",
    ),
    def(
        "server.trusted_proxies",
        Kind::List,
        L(&[]),
        I,
        NextStart,
        "Proxies whose forwarding headers are believed.",
    ),
    def(
        "server.front_doors",
        Kind::Words(&["mcp_http", "rest", "console"]),
        L(&[]),
        I,
        NextStart,
        "The network front doors that are served.",
    ),
    protected(
        def(
            "server.local_http",
            Kind::Bool,
            B(false),
            I,
            NextStart,
            "Whether a workstation opens a local port.",
        ),
        Safer::Off,
    ),
    protected(
        def(
            "server.webhooks",
            Kind::List,
            L(&[]),
            I,
            AtOnce,
            "Destinations run events are sent to, each checked against egress.",
        ),
        Safer::Fewer,
    ),
    def(
        "mcp.revisions",
        Kind::List,
        L(&["2026-07-28", "2025-11-25"]),
        I,
        NextStart,
        "The revisions of MCP that are served.",
    ),
    def(
        "auth.callers.issuer",
        Kind::Text,
        T(""),
        I,
        NextStart,
        "The identity provider that issues callers' tokens.",
    ),
    def(
        "auth.callers.audience",
        Kind::Text,
        T(""),
        I,
        NextStart,
        "The audience a caller's token must name.",
    ),
    def(
        "auth.callers.api_keys",
        Kind::Bool,
        B(false),
        I,
        NextStart,
        "Whether API keys are accepted.",
    ),
    def(
        "tools.listed_limit",
        Kind::Int { min: 1, max: 200 },
        N(19),
        U,
        AtOnce,
        "How many tools are listed up front.",
    ),
    def(
        "tools.full_listing",
        Kind::Bool,
        B(false),
        U,
        AtOnce,
        "Whether every tool is listed, for clients with their own tool search.",
    ),
    def(
        "tools.results",
        Kind::Word(&["concise", "detailed"]),
        T("concise"),
        U,
        AtOnce,
        "How much a result says by default.",
    ),
    def(
        "tools.result_cap_bytes",
        BYTES,
        N(262_144),
        U,
        AtOnce,
        "The largest result returned, in bytes. A draft.",
    ),
    // The network.
    def(
        "network.egress",
        Kind::Word(&["declared_surfaces"]),
        T("declared_surfaces"),
        I,
        NextStart,
        "Where the engine may connect to.",
    ),
    protected(
        def(
            "network.allowed_hosts",
            Kind::List,
            L(&[]),
            I,
            NextStart,
            "Further hosts the engine may connect to.",
        ),
        Safer::Fewer,
    ),
    protected(
        def(
            "network.proxy",
            Kind::Text,
            T(""),
            I,
            NextStart,
            "An outbound proxy.",
        ),
        Safer::Empty,
    ),
    protected(
        def(
            "network.trust_roots",
            Kind::List,
            L(&[]),
            I,
            NextStart,
            "Further certificate authorities, beside the operating system's.",
        ),
        Safer::Fewer,
    ),
    def(
        "network.private_addresses",
        Kind::Word(&["refused", "declared"]),
        T("refused"),
        I,
        NextStart,
        "Whether a private address may be reached when a target declares it.",
    ),
    // Providers, packs, and plug-ins.
    def(
        "providers.disabled",
        Kind::List,
        L(&[]),
        I,
        NextStart,
        "First-party packs switched off.",
    ),
    protected(
        def(
            "providers.enabled",
            Kind::List,
            L(&[]),
            I,
            NextStart,
            "Optional providers switched on.",
        ),
        Safer::Fewer,
    ),
    def(
        "providers.mode.*",
        Kind::Text,
        T(""),
        I,
        NextStart,
        "The provider that drives a mode two ready providers offer.",
    ),
    protected(
        def(
            "providers.attach",
            Kind::List,
            L(&[]),
            U,
            AtOnce,
            "Processes a person has named that a provider may attach to.",
        ),
        Safer::Fewer,
    ),
    protected(
        def(
            "plugins.allow_unsigned",
            Kind::List,
            L(&[]),
            I,
            NextStart,
            "Unsigned plug-ins that may run, by digest.",
        ),
        Safer::Fewer,
    ),
    // Updates, logging, and locale.
    protected(
        def(
            "updates.check",
            Kind::Bool,
            B(false),
            I,
            NextStart,
            "Whether the engine asks whether a newer release exists.",
        ),
        Safer::Off,
    ),
    def(
        "updates.channel",
        Kind::Word(&["stable", "beta"]),
        T("stable"),
        I,
        NextStart,
        "Which releases an update takes.",
    ),
    protected(
        def(
            "alerts.destinations",
            Kind::List,
            L(&[]),
            I,
            AtOnce,
            "Where alerts are sent.",
        ),
        Safer::Fewer,
    ),
    def(
        "logging.level",
        Kind::Word(&["error", "warn", "info", "debug"]),
        T("info"),
        U,
        AtOnce,
        "How much is logged.",
    ),
    def(
        "logging.format",
        Kind::Word(&["text", "json"]),
        T("text"),
        U,
        AtOnce,
        "The form of a log line.",
    ),
    def(
        "logging.to",
        Kind::Word(&["file", "stderr", "stdout"]),
        T("file"),
        U,
        AtOnce,
        "Where log lines go: the logs directory, standard error, or standard output.",
    ),
    def(
        "locale.messages",
        Kind::Text,
        T("en"),
        U,
        AtOnce,
        "The language of messages and the console.",
    ),
    def(
        "locale.display_timezone",
        Kind::Text,
        T(""),
        U,
        AtOnce,
        "The time zone of times shown to people. Empty means the machine's.",
    ),
];

/// The setting a key names, if the schema declares it.
#[must_use]
pub fn find(key: &str) -> Option<&'static SettingDef> {
    SCHEMA
        .iter()
        .find(|setting| setting.key == key)
        .or_else(|| {
            SCHEMA
                .iter()
                .find(|setting| setting.is_pattern() && setting.matches(key))
        })
}

/// What a profile sets that differs from the built-in defaults.
#[must_use]
pub fn profile_values(profile: &str) -> Vec<(&'static str, Value)> {
    match profile {
        "solo" | "workstation" => Vec::new(),
        _ => vec![("authoring.scout.run", Value::Text("on_request".to_owned()))],
    }
}

/// A built-in preset: its name, what it is for, and the TOML it sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuiltInPreset {
    /// Its name.
    pub name: &'static str,
    /// What it is for.
    pub purpose: &'static str,
    /// The values it sets, as a settings file.
    pub toml: &'static str,
}

/// The built-in presets. Each only tightens.
pub const PRESETS: &[BuiltInPreset] = &[
    BuiltInPreset {
        name: "standard",
        purpose: "The default: one person, at the keyboard. It sets nothing.",
        toml: "",
    },
    BuiltInPreset {
        name: "cautious",
        purpose: "A first look at a system, or a person who wants every step in view.",
        toml: r#"
[policy]
agent_effect_ceiling = "read"
pre_approval = false
max_records_per_run = 10

[authoring.scout]
run = "on_request"
try_reads = false

[evidence]
capture = "standard"

[sessions]
keep_alive_hours = 1
"#,
    },
    BuiltInPreset {
        name: "ci",
        purpose: "A job in continuous integration, with no person present.",
        toml: r#"
[engine]
in_process = true
person = "absent"

[policy]
writes_outside_tests = false

[authoring.scout]
run = "off"

[authoring.capture]
enabled = false

[logging]
format = "json"
to = "stdout"

[sessions]
login_wait_minutes = 0
"#,
    },
];

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use a2m_core::Value;

    use super::{Kind, SCHEMA, Safer, find};

    #[test]
    fn every_setting_is_declared_once_with_a_default_that_fits_and_a_description() {
        let mut keys = BTreeSet::new();
        for setting in SCHEMA {
            assert!(
                keys.insert(setting.key),
                "{} is declared twice",
                setting.key
            );
            assert!(
                setting.kind.fits(&setting.default.value()),
                "the default of {} does not fit its type",
                setting.key
            );
            assert!(
                setting.doc.ends_with('.'),
                "{} has no description",
                setting.key
            );
        }
    }

    #[test]
    fn a_key_with_a_registered_name_in_it_is_found_by_its_pattern() {
        assert_eq!(find("logging.level").unwrap().key, "logging.level");
        assert_eq!(
            find("ledger.intent_window.http").unwrap().key,
            "ledger.intent_window.*"
        );
        assert_eq!(
            find("scheduler.ui.session_reads").unwrap().key,
            "scheduler.*.session_reads"
        );
        assert!(find("logging.levle").is_none());
        assert!(find("scheduler.Bad Name.session_reads").is_none());
    }

    #[test]
    fn each_direction_knows_which_move_loosens() {
        let int = Value::Int;
        assert!(Safer::Lower.loosens(&int(50), &int(51)));
        assert!(!Safer::Lower.loosens(&int(50), &int(10)));
        assert!(Safer::Higher.loosens(&int(365), &int(30)));
        assert!(Safer::Off.loosens(&Value::Bool(false), &Value::Bool(true)));
        assert!(!Safer::Off.loosens(&Value::Bool(true), &Value::Bool(false)));
        let list = |items: &[&str]| {
            Value::List(
                items
                    .iter()
                    .map(|item| Value::Text((*item).to_owned()))
                    .collect(),
            )
        };
        assert!(Safer::Fewer.loosens(&list(&["a"]), &list(&["a", "b"])));
        assert!(!Safer::Fewer.loosens(&list(&["a", "b"]), &list(&["b"])));
        let rank = Safer::Rank(&["named_approver", "person_each_run", "caller_confirms"]);
        let word = |text: &str| Value::Text(text.to_owned());
        assert!(rank.loosens(&word("person_each_run"), &word("caller_confirms")));
        assert!(!rank.loosens(&word("person_each_run"), &word("named_approver")));
        assert!(Safer::Unknown.loosens(&int(1), &int(0)));
        assert!(Kind::Word(&["a"]).fits(&word("a")));
    }
}
