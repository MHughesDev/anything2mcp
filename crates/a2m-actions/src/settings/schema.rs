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

/// When a change to a setting takes effect. Settings are loaded once, when
/// the engine starts, and nothing reloads them, so every change takes
/// effect at the next start; a value that takes effect while the engine
/// runs arrives with a reload, which is not built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effective {
    /// At the next start of the engine.
    NextStart,
}

impl Effective {
    /// The words.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NextStart => "at the next start of the engine",
        }
    }
}

/// Whether the part a setting governs is built (decision 0236).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Built {
    /// Code reads the setting and honours its values; where it honours
    /// only part, the setting's description says which.
    Yes,
    /// Nothing reads it yet. A value written for it, from any layer, is
    /// refused by name.
    No {
        /// What happens instead, as a sentence.
        instead: &'static str,
        /// What it arrives with: a roadmap package, a milestone, or a
        /// decision.
        arrives_with: &'static str,
    },
}

impl Built {
    /// Whether it is built.
    #[must_use]
    pub const fn is_built(&self) -> bool {
        matches!(self, Self::Yes)
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
    /// Whether the part it governs is built.
    pub built: Built,
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
        built: Built::Yes,
    }
}

const fn protected(mut setting: SettingDef, safer: Safer) -> SettingDef {
    setting.protected = Some(safer);
    setting
}

/// A setting whose part is not built: what happens instead, and what it
/// arrives with.
const fn not_built(
    mut setting: SettingDef,
    instead: &'static str,
    arrives_with: &'static str,
) -> SettingDef {
    setting.built = Built::No {
        instead,
        arrives_with,
    };
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
use Effective::NextStart;
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
    not_built(
        def(
            "profile.roles",
            Kind::Words(&["control_plane", "runner"]),
            L(&["control_plane", "runner"]),
            I,
            NextStart,
            "Which roles this process takes.",
        ),
        "One process takes both roles.",
        "remote runners, with the server (milestone 9 of the roadmap)",
    ),
    def(
        "profile.preset",
        Kind::Text,
        T("standard"),
        U,
        NextStart,
        "A named bundle of values: a built-in preset, or a site's own.",
    ),
    // Storage, the vault, and files.
    not_built(
        def(
            "store.database",
            Kind::Text,
            T(""),
            I,
            NextStart,
            "Where the store is. Empty means one file in the data directory.",
        ),
        "The store is one file in the data directory.",
        "a slice of its own, as decision 0236 orders them",
    ),
    not_built(
        def(
            "store.blobs",
            Kind::Text,
            T(""),
            I,
            NextStart,
            "Where evidence blobs are kept. Empty means the data directory.",
        ),
        "Evidence blobs are kept in the store's folder in the data directory.",
        "a slice of its own, as decision 0236 orders them",
    ),
    not_built(
        def(
            "store.free_space_floor",
            BYTES,
            N(1_073_741_824),
            I,
            NextStart,
            "The free space, in bytes, below which the engine is degraded. A draft.",
        ),
        "No free space is checked.",
        "a slice of its own, as decision 0236 orders them",
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
    not_built(
        def(
            "vault.root_key",
            Kind::Text,
            T(""),
            I,
            NextStart,
            "A reference to the root key. Empty means the key store's own.",
        ),
        "The root key is the key store's own, or the passphrase's.",
        "a key service by reference, with the server (milestone 9 of the roadmap)",
    ),
    protected(
        def(
            "files.allowed_roots",
            Kind::List,
            L(&["<project>"]),
            U,
            NextStart,
            "The named folders files may be read from and written to.",
        ),
        Safer::Fewer,
    ),
    def(
        "files.max_upload_bytes",
        BYTES,
        N(104_857_600),
        U,
        NextStart,
        "The largest file a caller may hand in, in bytes. A draft.",
    ),
    // Policy.
    protected(
        def(
            "policy.approve_writes",
            Kind::Word(&["named_approver", "person_each_run", "caller_confirms"]),
            T("person_each_run"),
            U,
            NextStart,
            "Who must approve a write. On one machine only `person_each_run` is in force: `named_approver` and `caller_confirms` stop the kernel by name.",
        ),
        Safer::Rank(&["named_approver", "person_each_run", "caller_confirms"]),
    ),
    protected(
        def(
            "policy.two_person_irreversible",
            Kind::Bool,
            B(false),
            U,
            NextStart,
            "Whether an irreversible operation needs two people.",
        ),
        // Turning it off is the loosening (decision 0236).
        Safer::On,
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
            NextStart,
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
            NextStart,
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
            NextStart,
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
            NextStart,
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
            NextStart,
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
            NextStart,
            "Whether writes may go anywhere but declared test records in a test environment.",
        ),
        Safer::Off,
    ),
    // Identity and sessions.
    not_built(
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
        "Each target's auth file says how its identity is obtained.",
        "the server, milestone 9 of the roadmap",
    ),
    not_built(
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
        "Each target's auth file says how its identity is obtained; no toolset limits it.",
        "the server, milestone 9 of the roadmap",
    ),
    def(
        "sessions.keep_alive_hours",
        Kind::Int { min: 0, max: 8_760 },
        N(8),
        U,
        NextStart,
        "How long after its last use a session is still checked.",
    ),
    def(
        "sessions.probe_minutes",
        Kind::Int { min: 1, max: 1_440 },
        N(15),
        U,
        NextStart,
        "How often a session is probed when its auth file declares no idle timeout.",
    ),
    def(
        "sessions.login_wait_minutes",
        Kind::Int { min: 0, max: 1_440 },
        N(5),
        U,
        NextStart,
        "How long an interactive call waits for a sign-in.",
    ),
    not_built(
        def(
            "sessions.attempt_limit",
            Kind::Int { min: 1, max: 3 },
            N(1),
            I,
            NextStart,
            "How many times a stored credential is tried.",
        ),
        "Each target's auth file sets how often a stored credential is tried, 1 by default; no ceiling of the installation is applied.",
        "a slice of its own, as decision 0236 orders them",
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
        NextStart,
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
        NextStart,
        "Whether work that needs a person waits for one, or stops at once with its named state. Only the wait for a sign-in honours `absent` yet; no other wait for a person does.",
    ),
    def(
        "engine.retries.transient",
        Kind::Int { min: 0, max: 5 },
        N(2),
        U,
        NextStart,
        "How often a read or an idempotent step is tried again after a `transient` failure.",
    ),
    not_built(
        def(
            "engine.time_limit_ceiling_seconds",
            Kind::Int {
                min: 1,
                max: 86_400,
            },
            N(600),
            I,
            NextStart,
            "The longest step time limit a target may declare here. A draft.",
        ),
        "A target's step time limits are bounded by each pack's own ceilings; no ceiling of the installation is checked.",
        "a slice of its own, as decision 0236 orders them",
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
            NextStart,
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
            NextStart,
            "The matching writes a shadow binding needs before a person may promote it.",
        ),
        Safer::Higher,
    ),
    protected(
        def(
            "engine.shadow.time_limit_ms",
            Kind::Int {
                min: 100,
                max: 60_000,
            },
            N(2_000),
            U,
            NextStart,
            "The one budget of time all of a call's shadow bindings share, their reads and the keeping of their comparisons together.",
        ),
        Safer::Lower,
    ),
    not_built(
        def(
            "engine.promoted_writes",
            Kind::Word(&["after_promotion", "never"]),
            T("after_promotion"),
            U,
            NextStart,
            "Whether writes may use a binding promoted from shadow.",
        ),
        "A write may use a binding promoted from shadow.",
        "a slice of its own, as decision 0236 orders them",
    ),
    not_built(
        def(
            "ledger.intent_window.*",
            Kind::Int { min: 1, max: 1_000 },
            N(1),
            U,
            NextStart,
            "How many intents of one mode may be open at once, up to its manifest's value.",
        ),
        "Each mode's manifest sets how many of its intents may be open at once.",
        "a slice of its own, as decision 0236 orders them",
    ),
    not_built(
        def(
            "modes.disabled",
            Kind::List,
            L(&[]),
            U,
            NextStart,
            "Modes that may not be used here.",
        ),
        "Every mode a ready provider offers may be used.",
        "a slice of its own, as decision 0236 orders them",
    ),
    // Scheduling.
    not_built(
        def(
            "scheduler.*.session_reads",
            COUNT,
            N(0),
            U,
            NextStart,
            "Reads at once on one session of a mode, up to its manifest's value.",
        ),
        "Each mode's manifest sets how many reads run at once on one session.",
        "a slice of its own, as decision 0236 orders them",
    ),
    not_built(
        def(
            "scheduler.*.session_writes",
            COUNT,
            N(0),
            U,
            NextStart,
            "Writes at once on one session of a mode, up to its manifest's value.",
        ),
        "Each mode's manifest sets how many writes run at once on one session.",
        "a slice of its own, as decision 0236 orders them",
    ),
    def(
        "scheduler.queue_length_per_target",
        Kind::Int {
            min: 1,
            max: 100_000,
        },
        N(100),
        U,
        NextStart,
        "How many jobs may wait for one target.",
    ),
    not_built(
        def(
            "scheduler.priority",
            Kind::Word(&["interactive_first", "fifo"]),
            T("interactive_first"),
            I,
            NextStart,
            "Which waiting job is served first.",
        ),
        "A person's interactive call is served before other waiting work.",
        "the server, milestone 9 of the roadmap",
    ),
    def(
        "runners.lease_seconds",
        Kind::Int { min: 5, max: 3_600 },
        N(30),
        I,
        NextStart,
        "How long a job's lease lasts without a heartbeat. In one process the lease is at least 600 seconds, whatever the value.",
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
    not_built(
        def(
            "runners.certificate_hours",
            Kind::Int { min: 1, max: 8_760 },
            N(24),
            I,
            NextStart,
            "How long a runner's certificate lasts.",
        ),
        "Runners are in the engine's own process and hold no certificate.",
        "remote runners, with the server (milestone 9 of the roadmap)",
    ),
    // Authoring and the scout.
    def(
        "authoring.scout.run",
        Kind::Word(&["off", "on_request", "on_add"]),
        T("on_add"),
        P,
        NextStart,
        "When the scout runs: never, when asked, or when a target is added. Only `off` is told apart yet: `on_request` and `on_add` both run a scout when one is asked for.",
    ),
    def(
        "authoring.scout.try_reads",
        Kind::Bool,
        B(true),
        P,
        NextStart,
        "Whether the scout tries the reads it drafted.",
    ),
    not_built(
        def(
            "authoring.scout.budget.seconds",
            Kind::Int {
                min: 1,
                max: 86_400,
            },
            N(300),
            P,
            NextStart,
            "The time one scout may take. A target until measured.",
        ),
        "A scout has no time budget, and its report says the budget is not in force.",
        "the scout's budget, roadmap package E5",
    ),
    not_built(
        def(
            "authoring.scout.budget.requests",
            Kind::Int {
                min: 1,
                max: 1_000_000,
            },
            N(500),
            P,
            NextStart,
            "The requests one scout may send. A target until measured.",
        ),
        "A scout has no budget of requests, and its report says the budget is not in force.",
        "the scout's budget, roadmap package E5",
    ),
    not_built(
        def(
            "authoring.scout.budget.depth",
            Kind::Int { min: 1, max: 100 },
            N(3),
            P,
            NextStart,
            "How deep one scout follows what it finds. A target until measured.",
        ),
        "A scout has no budget of depth, and its report says the budget is not in force.",
        "the scout's budget, roadmap package E5",
    ),
    not_built(
        def(
            "authoring.scout.providers",
            Kind::List,
            L(&[]),
            P,
            NextStart,
            "Optional discoverers, by name. First-party discoverers are always available.",
        ),
        "Only first-party discoverers run; no optional one exists.",
        "optional discoverers, roadmap package E6",
    ),
    not_built(
        def(
            "authoring.capture.enabled",
            Kind::Bool,
            B(true),
            P,
            NextStart,
            "Whether recording may be started at all.",
        ),
        "Recording cannot be started: `capture.start` is not built.",
        "recording, `capture.start`",
    ),
    not_built(
        protected(
            def(
                "authoring.capture.values",
                Kind::Bool,
                B(false),
                P,
                NextStart,
                "Whether a recording keeps values, not only shapes.",
            ),
            Safer::Off,
        ),
        "Recording cannot be started: `capture.start` is not built.",
        "recording, `capture.start`",
    ),
    not_built(
        def(
            "authoring.capture.limits.seconds",
            Kind::Int {
                min: 1,
                max: 86_400,
            },
            N(600),
            P,
            NextStart,
            "The longest one recording may run. A draft.",
        ),
        "Recording cannot be started: `capture.start` is not built.",
        "recording, `capture.start`",
    ),
    not_built(
        def(
            "authoring.capture.limits.bytes",
            BYTES,
            N(52_428_800),
            P,
            NextStart,
            "The most one recording may keep, in bytes. A draft.",
        ),
        "Recording cannot be started: `capture.start` is not built.",
        "recording, `capture.start`",
    ),
    not_built(
        def(
            "authoring.capture.limits.exchanges",
            Kind::Int {
                min: 1,
                max: 1_000_000,
            },
            N(2_000),
            P,
            NextStart,
            "The most exchanges one recording may keep. A draft.",
        ),
        "Recording cannot be started: `capture.start` is not built.",
        "recording, `capture.start`",
    ),
    // Evidence and telemetry.
    def(
        "evidence.capture",
        Kind::Word(&["minimal", "standard", "full"]),
        T("minimal"),
        U,
        NextStart,
        "How much evidence a read keeps.",
    ),
    protected(
        def(
            "evidence.capture_on_writes",
            Kind::Word(&["standard", "full"]),
            T("standard"),
            U,
            NextStart,
            "How much evidence a write keeps.",
        ),
        Safer::Rank(&["full", "standard"]),
    ),
    not_built(
        def(
            "evidence.trace_retention_days",
            Kind::Int {
                min: 1,
                max: 36_500,
            },
            N(30),
            U,
            NextStart,
            "How long traces are kept.",
        ),
        "Traces and captures are kept; nothing is deleted yet.",
        "retention, the first of the slices decision 0236 orders",
    ),
    not_built(
        protected(
            def(
                "evidence.audit_retention_days",
                Kind::Int {
                    min: 1,
                    max: 36_500,
                },
                N(365),
                I,
                NextStart,
                "How long the audit log is kept.",
            ),
            Safer::Higher,
        ),
        "The audit is kept; nothing is deleted yet.",
        "retention, the first of the slices decision 0236 orders",
    ),
    not_built(
        def(
            "evidence.redaction.extra",
            Kind::List,
            L(&[]),
            U,
            NextStart,
            "Further patterns and field names to mask, added to the built-in rules.",
        ),
        "Only registered secrets and the built-in rules are masked.",
        "a slice of its own, as decision 0236 orders them",
    ),
    not_built(
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
        "Nothing is sent anywhere.",
        "the server, milestone 9 of the roadmap",
    ),
    not_built(
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
        "No crash report is ever sent.",
        "the server, milestone 9 of the roadmap",
    ),
    // Front doors.
    not_built(
        def(
            "server.listen",
            Kind::Text,
            T(""),
            I,
            NextStart,
            "The address the server listens on. Empty opens no port.",
        ),
        "The engine listens only on the person's own channel.",
        "the server, milestone 9 of the roadmap",
    ),
    not_built(
        def(
            "server.tls.certificate",
            Kind::Text,
            T(""),
            I,
            NextStart,
            "A reference to the server's certificate.",
        ),
        "No port is opened, so no certificate is used.",
        "the server, milestone 9 of the roadmap",
    ),
    not_built(
        def(
            "server.tls.key",
            Kind::Text,
            T(""),
            I,
            NextStart,
            "A reference to the certificate's key.",
        ),
        "No port is opened, so no key is used.",
        "the server, milestone 9 of the roadmap",
    ),
    not_built(
        def(
            "server.base_url",
            Kind::Text,
            T(""),
            I,
            NextStart,
            "The address callers reach the server by.",
        ),
        "No port is opened.",
        "the server, milestone 9 of the roadmap",
    ),
    not_built(
        def(
            "server.trusted_proxies",
            Kind::List,
            L(&[]),
            I,
            NextStart,
            "Proxies whose forwarding headers are believed.",
        ),
        "No port is opened, so no forwarding header is read.",
        "the server, milestone 9 of the roadmap",
    ),
    not_built(
        def(
            "server.front_doors",
            Kind::Words(&["mcp_http", "rest", "console"]),
            L(&[]),
            I,
            NextStart,
            "The network front doors that are served.",
        ),
        "Only the local front doors are served, on the person's own channel.",
        "the server, milestone 9 of the roadmap",
    ),
    not_built(
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
        "No local port is opened.",
        "the server, milestone 9 of the roadmap",
    ),
    not_built(
        protected(
            def(
                "server.webhooks",
                Kind::List,
                L(&[]),
                I,
                NextStart,
                "Destinations run events are sent to, each checked against egress.",
            ),
            Safer::Fewer,
        ),
        "No run event is sent anywhere.",
        "the server, milestone 9 of the roadmap",
    ),
    def(
        "mcp.revisions",
        Kind::List,
        L(&["2026-07-28", "2025-11-25"]),
        I,
        NextStart,
        "The revisions of MCP that are served.",
    ),
    not_built(
        def(
            "auth.callers.issuer",
            Kind::Text,
            T(""),
            I,
            NextStart,
            "The identity provider that issues callers' tokens.",
        ),
        "The caller is the person's own account, as the local channel sees it.",
        "the server, milestone 9 of the roadmap",
    ),
    not_built(
        def(
            "auth.callers.audience",
            Kind::Text,
            T(""),
            I,
            NextStart,
            "The audience a caller's token must name.",
        ),
        "The caller is the person's own account, as the local channel sees it.",
        "the server, milestone 9 of the roadmap",
    ),
    not_built(
        def(
            "auth.callers.api_keys",
            Kind::Bool,
            B(false),
            I,
            NextStart,
            "Whether API keys are accepted.",
        ),
        "No API key is accepted.",
        "the server, milestone 9 of the roadmap",
    ),
    def(
        "tools.listed_limit",
        Kind::Int { min: 1, max: 200 },
        N(19),
        U,
        NextStart,
        "How many tools are listed up front.",
    ),
    def(
        "tools.full_listing",
        Kind::Bool,
        B(false),
        U,
        NextStart,
        "Whether every tool is listed, for clients with their own tool search.",
    ),
    not_built(
        def(
            "tools.results",
            Kind::Word(&["concise", "detailed"]),
            T("concise"),
            U,
            NextStart,
            "How much a result says by default.",
        ),
        "A result is in its one, concise form.",
        "a slice of its own, as decision 0236 orders them",
    ),
    def(
        "tools.result_cap_bytes",
        BYTES,
        N(262_144),
        U,
        NextStart,
        "The largest result returned, in bytes. A draft.",
    ),
    // The network.
    not_built(
        def(
            "network.egress",
            Kind::Word(&["declared_surfaces"]),
            T("declared_surfaces"),
            I,
            NextStart,
            "Where the engine may connect to.",
        ),
        "Connections go only to the surfaces a target declares, which is this key's one value; nothing reads the key.",
        "network awareness (decision 0222)",
    ),
    not_built(
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
        "Connections go only to the surfaces a target declares.",
        "network awareness (decision 0222)",
    ),
    not_built(
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
        "No outbound proxy is supported yet: traffic goes straight to each target, as the network allows.",
        "an outbound proxy (decisions 0197 and 0208)",
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
    not_built(
        def(
            "network.private_addresses",
            Kind::Word(&["refused", "declared"]),
            T("refused"),
            I,
            NextStart,
            "Whether a private address may be reached when a target declares it.",
        ),
        "A target may declare a private address, as the network allows.",
        "the server, milestone 9 of the roadmap",
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
    not_built(
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
        "Every first-party pack is on unless `providers.disabled` names it; no optional provider exists.",
        "plug-ins, roadmap package F3",
    ),
    def(
        "providers.mode.*",
        Kind::Text,
        T(""),
        I,
        NextStart,
        "The provider that drives a mode two ready providers offer.",
    ),
    not_built(
        protected(
            def(
                "providers.attach",
                Kind::List,
                L(&[]),
                U,
                NextStart,
                "Processes a person has named that a provider may attach to.",
            ),
            Safer::Fewer,
        ),
        "No provider attaches to a process a person names.",
        "a slice of its own, as decision 0236 orders them",
    ),
    not_built(
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
        "No plug-in runs: plug-ins are not built.",
        "plug-ins, roadmap package F3",
    ),
    // Updates, logging, and locale.
    not_built(
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
        "The engine never asks whether a newer release exists.",
        "a release, roadmap package R1",
    ),
    not_built(
        def(
            "updates.channel",
            Kind::Word(&["stable", "beta"]),
            T("stable"),
            I,
            NextStart,
            "Which releases an update takes.",
        ),
        "The engine never asks whether a newer release exists.",
        "a release, roadmap package R1",
    ),
    not_built(
        protected(
            def(
                "alerts.destinations",
                Kind::List,
                L(&[]),
                I,
                NextStart,
                "Where alerts are sent.",
            ),
            Safer::Fewer,
        ),
        "No alert is sent anywhere; alerts are shown on the machine.",
        "the server, milestone 9 of the roadmap",
    ),
    def(
        "logging.level",
        Kind::Word(&["error", "warn", "info", "debug"]),
        T("info"),
        U,
        NextStart,
        "How much is logged.",
    ),
    not_built(
        def(
            "logging.format",
            Kind::Word(&["text", "json"]),
            T("text"),
            U,
            NextStart,
            "The form of a log line.",
        ),
        "Log lines are text.",
        "a slice of its own, as decision 0236 orders them",
    ),
    not_built(
        def(
            "logging.to",
            Kind::Word(&["file", "stderr", "stdout"]),
            T("file"),
            U,
            NextStart,
            "Where log lines go: the logs directory, standard error, or standard output.",
        ),
        "Log lines go to the logs folder in the data directory.",
        "a slice of its own, as decision 0236 orders them",
    ),
    not_built(
        def(
            "locale.messages",
            Kind::Text,
            T("en"),
            U,
            NextStart,
            "The language of messages and the console.",
        ),
        "Every message is in English.",
        "a slice of its own, as decision 0236 orders them",
    ),
    not_built(
        def(
            "locale.display_timezone",
            Kind::Text,
            T(""),
            U,
            NextStart,
            "The time zone of times shown to people. Empty means the machine's.",
        ),
        "Times are shown as the machine gives them; none is converted.",
        "a slice of its own, as decision 0236 orders them",
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
        purpose: "A job in continuous integration, with no person present: the engine in the command's own process, writes only to declared test records, no scout, and no wait for a sign-in. It does not yet change the form or place of logs.",
        toml: r#"
[engine]
in_process = true
person = "absent"

[policy]
writes_outside_tests = false

[authoring.scout]
run = "off"

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
