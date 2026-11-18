//! The provider manifest: what a pack or a plug-in declares before it may
//! register anything.
//!
//! One TOML file per provider, `anything2mcp-provider.toml`. A first-party
//! pack embeds its file with `include_str!`, so packs and plug-ins pass one
//! parser and the same checks. The shape is checked by the types below as
//! the file is read, and a problem names the line; the rules that span
//! fields are checked afterwards and name the field.

use std::collections::{BTreeMap, BTreeSet};

use a2m_core::{
    Archetype, ControlName, Digest, EffectClass, EvidenceLevel, Failure, FailureClass, FailureCode,
    FileKindName, FindingKind, LintCode, Mode, StepKind, SurfaceKind, Value, Values, Verb, Version,
};
use serde::Deserialize;

use crate::codes::{self, shown};
use crate::name::{ActionName, DiscovererName, ProviderName, RootName};

/// The file name of a manifest.
pub const MANIFEST_FILE_NAME: &str = "anything2mcp-provider.toml";
/// The manifest format this build reads.
pub const MANIFEST_FORMAT: u32 = 1;
/// The provider contract this build offers. The previous one is accepted
/// too; there is none before 1.
pub const CONTRACT: u32 = 1;

/// Whether a provider is compiled in or runs as its own process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    /// A first-party pack, compiled into the product.
    Pack,
    /// A plug-in: its own process, under the supervisor.
    Plugin,
}

/// What one write of a mode holds alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Exclusive {
    /// Nothing: writes may run side by side.
    None,
    /// The surface.
    #[default]
    Surface,
    /// One file.
    File,
    /// One screen.
    Screen,
    /// One link to a device or a stream.
    Link,
}

impl Exclusive {
    /// The word as the manifest writes it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Surface => "surface",
            Self::File => "file",
            Self::Screen => "screen",
            Self::Link => "link",
        }
    }
}

/// What a provider may reach over a network.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkEffect {
    /// Nothing.
    #[default]
    None,
    /// Only the surfaces a target declares.
    Surfaces,
}

/// Which list a kind is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KindOf {
    /// A kind of target file.
    FileKinds,
    /// A kind of finding.
    FindingKinds,
}

impl KindOf {
    /// The word as the manifest writes it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::FileKinds => "file_kinds",
            Self::FindingKinds => "finding_kinds",
        }
    }
}

/// Where a setting may be set (spec section 226).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SettingScope {
    /// The installation only.
    Installation,
    /// A user and above.
    #[default]
    User,
    /// A project and above.
    Project,
}

/// The type of a setting's value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SettingType {
    /// True or false.
    Bool,
    /// A whole number, within `min` and `max` when they are given.
    Int,
    /// Text.
    Text,
    /// One of the words in `choices`.
    Choice,
    /// A list of texts.
    List,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawManifest {
    manifest_format: u32,
    contract: u32,
    provider: RawProvider,
    run: Option<RawRun>,
    #[serde(default)]
    modes: Vec<ModeDecl>,
    #[serde(default)]
    surface_kinds: Vec<SurfaceKindDecl>,
    #[serde(default)]
    reads: Vec<ReadDecl>,
    #[serde(default)]
    points: RawPoints,
    #[serde(default)]
    discoverers: Vec<RawDiscoverer>,
    #[serde(default)]
    file_kinds: Vec<FileKindDecl>,
    #[serde(default)]
    finding_kinds: Vec<FindingKindDecl>,
    #[serde(default)]
    controls: Vec<ControlDecl>,
    #[serde(default)]
    lint_rules: Vec<LintRuleDecl>,
    #[serde(default)]
    drafting_rules: Vec<DraftingRuleDecl>,
    #[serde(default)]
    actions: Vec<RawAction>,
    #[serde(default)]
    failure_codes: Vec<FailureCodeDecl>,
    #[serde(default)]
    effects: RawEffects,
    #[serde(default)]
    requirements: Vec<RawRequirement>,
    #[serde(default)]
    platforms: RawPlatforms,
    evidence: Option<EvidenceDecl>,
    #[serde(default)]
    settings: Vec<RawSetting>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProvider {
    name: String,
    version: Version,
    #[serde(default)]
    publisher: String,
    kind: ProviderKind,
    digest: Option<String>,
    signature: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRun {
    program: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    environment: Vec<String>,
    protocol: Vec<u32>,
    #[serde(default = "default_frame")]
    max_frame_bytes: u32,
    #[serde(default = "one")]
    concurrency: u32,
    #[serde(default = "default_queue")]
    queue: u32,
}

const fn default_frame() -> u32 {
    1_048_576
}
const fn one() -> u32 {
    1
}
const fn default_queue() -> u32 {
    16
}
const fn default_rank() -> u32 {
    100
}

/// How a plug-in is started. Nothing from a caller is ever added.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunSection {
    /// The program, relative to the manifest; never searched on a path.
    pub program: String,
    /// Its fixed arguments.
    pub args: Vec<String>,
    /// The names of the variables passed through to it.
    pub environment: Vec<String>,
    /// The plug-in protocol versions it speaks, newest first.
    pub protocol: Vec<u32>,
    /// The longest frame it sends.
    pub max_frame_bytes: u32,
    /// How many requests it serves at once.
    pub concurrency: u32,
    /// How many requests may wait beyond that.
    pub queue: u32,
}

/// One mode a provider brings, with its defaults.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModeDecl {
    /// The mode's name.
    pub name: Mode,
    /// Its one archetype.
    pub archetype: Archetype,
    /// The verbs a binding of the mode may state.
    #[serde(default)]
    pub verbs: Vec<Verb>,
    /// Every kind of step the mode's driver carries out.
    #[serde(default)]
    pub steps: Vec<StepKind>,
    /// The kinds of step that always save. The handle refuses one that is
    /// sent as a step that commits nothing, so that the mark a plan or a
    /// discoverer puts on a step is not all that stands between a read and
    /// a write. Empty, the default, checks nothing.
    #[serde(default)]
    pub commit_steps: Vec<StepKind>,
    /// The kinds of step whose "absent" the mode's driver states is
    /// authoritative: when such a step answers that a record is not there,
    /// after the binding's settle time, the record is not there in the
    /// system, whatever replica, page, or window a reader might otherwise
    /// see. Only a lookup made of these kinds alone may prove that a write
    /// of a record with no key did not land (decision 0185); a read-back
    /// never does. Empty, the default, states none: "absent" from such a
    /// mode proves nothing on its own.
    #[serde(default)]
    pub absence_steps: Vec<StepKind>,
    /// Its place in binding order when an operation states none; lower is
    /// tried first.
    #[serde(default = "default_rank")]
    pub rank: u32,
    /// How many intents are stored ahead of acting.
    #[serde(default = "one")]
    pub intent_window: u32,
    /// Parallel reads in one session.
    #[serde(default = "one")]
    pub session_reads: u32,
    /// Parallel writes in one session.
    #[serde(default = "one")]
    pub session_writes: u32,
    /// What one write holds alone.
    #[serde(default)]
    pub exclusive: Exclusive,
}

/// The defaults of a mode, as the registry gives them to the engine and,
/// as data, to the scheduler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModeDefaults {
    /// Its place in binding order; lower is tried first.
    pub rank: u32,
    /// Intents stored ahead of acting.
    pub intent_window: u32,
    /// Parallel reads in one session.
    pub session_reads: u32,
    /// Parallel writes in one session.
    pub session_writes: u32,
    /// What one write holds alone.
    pub exclusive: Exclusive,
}

impl ModeDecl {
    /// The mode's defaults.
    #[must_use]
    pub fn defaults(&self) -> ModeDefaults {
        ModeDefaults {
            rank: self.rank,
            intent_window: self.intent_window,
            session_reads: self.session_reads,
            session_writes: self.session_writes,
            exclusive: self.exclusive,
        }
    }
}

/// A kind of surface a provider serves.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceKindDecl {
    /// The surface kind's name.
    pub name: SurfaceKind,
    /// Its archetype.
    pub archetype: Archetype,
    /// Whether the keeper holds a session on a surface of this kind.
    #[serde(default)]
    pub session: bool,
}

/// A kind another provider owns, which this one reads as data.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadDecl {
    /// The kind's name: a file kind or a finding kind.
    pub kind: String,
    /// Which of the two.
    pub of: KindOf,
    /// The surface kind whose owner is meant.
    pub surface_kind: SurfaceKind,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPoints {
    #[serde(default)]
    driver: Vec<Mode>,
    #[serde(default)]
    discoverer: Vec<String>,
    #[serde(default)]
    auth_provider: Vec<String>,
    #[serde(default)]
    source_reader: Vec<String>,
    #[serde(default)]
    widget_driver: Vec<String>,
}

/// The extension points a provider fills. Nothing else can be registered.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Points {
    /// The modes it drives.
    pub driver: BTreeSet<Mode>,
    /// Its discoverers, each with its describer.
    pub discoverer: BTreeSet<DiscovererName>,
    /// The auth providers it brings, by name.
    pub auth_provider: BTreeSet<String>,
    /// The source readers it brings, by name.
    pub source_reader: BTreeSet<String>,
    /// The widget drivers it brings. No trait exists for these yet.
    pub widget_driver: BTreeSet<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDiscoverer {
    name: String,
    archetype: Archetype,
    #[serde(default)]
    surface_kinds: Vec<SurfaceKind>,
    kinds: Vec<FindingKind>,
    #[serde(default)]
    sources: Vec<String>,
    max_level: EvidenceLevel,
    bounds: Bounds,
    #[serde(default)]
    files: Vec<String>,
}

/// How far a discoverer may go.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bounds {
    /// The longest it may run, in seconds.
    pub seconds: u32,
    /// The most bytes it may read.
    pub bytes: u64,
    /// The deepest it may follow from where it starts.
    pub depth: u32,
}

/// One discoverer a provider declares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscovererDecl {
    /// Its name within the provider.
    pub name: DiscovererName,
    /// The archetype of what it observes.
    pub archetype: Archetype,
    /// The surface kinds it applies to. Empty means any surface of its
    /// archetype.
    pub surface_kinds: Vec<SurfaceKind>,
    /// The finding kinds it may return.
    pub kinds: Vec<FindingKind>,
    /// What it observes, as the finding's `source` words.
    pub sources: Vec<String>,
    /// The highest evidence level it may claim: `inferred` or `discovered`.
    pub max_level: EvidenceLevel,
    /// Its bounds.
    pub bounds: Bounds,
    /// The names under which it reads files a person hands a discovery, in
    /// the order the manifest lists them: a file of the project's `sources`
    /// folder reaches it as the path of that name in
    /// [`crate::DiscoveryScope::paths`] (decision 0263). Each is a name of
    /// lowercase letters, digits, and underscores. Empty, the default: it
    /// reads no file it is handed.
    pub files: Vec<String>,
}

/// The longest name a discoverer may read a file under.
const LONGEST_FILE_NAME: usize = 32;

/// Whether `name` may name a file a discoverer reads: lowercase letters,
/// digits, and underscores, starting with a letter.
fn is_file_name(name: &str) -> bool {
    let mut chars = name.chars();
    name.len() <= LONGEST_FILE_NAME
        && chars.next().is_some_and(|first| first.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// A file kind a provider owns.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileKindDecl {
    /// The file kind's name.
    pub name: FileKindName,
    /// The folder its files live in, as one name.
    pub directory: String,
    /// The surface kinds it is owned for.
    pub surface_kinds: Vec<SurfaceKind>,
    /// A plug-in's schema file, beside the manifest. A pack registers its
    /// schema as an object.
    pub schema: Option<String>,
}

/// A finding kind a provider owns.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FindingKindDecl {
    /// The finding kind's name.
    pub name: FindingKind,
    /// The surface kinds it is owned for.
    pub surface_kinds: Vec<SurfaceKind>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct ControlDecl {
    name: ControlName,
}

/// A lint rule a provider declares.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LintRuleDecl {
    /// The code: the provider's name, a dot, and the rule.
    pub code: LintCode,
    /// `error` or `warning`, for a person reading the manifest.
    pub level: String,
    /// What the rule requires.
    pub summary: String,
}

/// A drafting rule a provider declares.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DraftingRuleDecl {
    /// The kind of finding it reads.
    pub finding_kind: FindingKind,
    /// The kind of file it writes.
    pub file_kind: FileKindName,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawAction {
    name: String,
    effect: EffectClass,
}

/// An action a provider adds, under its own name space.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionDecl {
    /// The action's name.
    pub name: ActionName,
    /// Whether it reads, writes, or cannot be undone.
    pub effect: EffectClass,
}

/// A failure code a provider's drivers may return, with its one class.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FailureCodeDecl {
    /// The class.
    pub class: FailureClass,
    /// The code.
    pub code: FailureCode,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEffects {
    #[serde(default)]
    processes: Vec<RawProgram>,
    #[serde(default)]
    plan_programs: bool,
    #[serde(default)]
    read_roots: Vec<String>,
    #[serde(default)]
    write_roots: Vec<String>,
    #[serde(default)]
    network: NetworkEffect,
    #[serde(default)]
    sessions: Vec<SurfaceKind>,
    #[serde(default)]
    target_code: bool,
    #[serde(default)]
    attach: bool,
    #[serde(default)]
    screen: bool,
    #[serde(default)]
    devices: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProgram {
    path: String,
    digest: Option<String>,
}

/// A program a provider starts itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgramDecl {
    /// A path relative to the provider, or a fixed absolute path.
    pub path: String,
    /// The digest it must have, when the manifest pins one.
    pub digest: Option<Digest>,
}

/// What a provider may do beyond computing. Every start, root, and session
/// is checked against this.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Effects {
    /// The programs it starts itself.
    pub processes: Vec<ProgramDecl>,
    /// Whether it may also start programs a published plan names.
    pub plan_programs: bool,
    /// The roots it reads.
    pub read_roots: Vec<RootName>,
    /// The roots it writes.
    pub write_roots: Vec<RootName>,
    /// What it reaches over a network.
    pub network: NetworkEffect,
    /// The surface kinds it holds a session for.
    pub sessions: Vec<SurfaceKind>,
    /// Whether it runs code inside the system.
    pub target_code: bool,
    /// Whether it attaches to a running program.
    pub attach: bool,
    /// Whether it reads or drives a screen.
    pub screen: bool,
    /// Whether it reaches hardware.
    pub devices: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRequirement {
    name: String,
    detect: RawDetect,
    #[serde(default)]
    known_digests: Vec<String>,
    versions: Option<String>,
    #[serde(default)]
    missing: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDetect {
    paths: Vec<String>,
}

/// An outside program or runtime a provider needs on the machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Requirement {
    /// Its name, for messages.
    pub name: String,
    /// The fixed places it is looked for, in order.
    pub paths: Vec<String>,
    /// The digests known for it. Empty means any is taken as unverified.
    pub known_digests: Vec<Digest>,
    /// The versions supported, as text for a person. The availability check
    /// starts nothing, so it cannot ask the program its version.
    pub versions: Option<String>,
    /// What to tell a person when it is missing.
    pub missing: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPlatforms {
    #[serde(default)]
    supported: Vec<String>,
}

/// What a provider's findings may claim.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceDecl {
    /// The highest level.
    pub max_level: EvidenceLevel,
    /// The sources it vouches for.
    #[serde(default)]
    pub vouches_for: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSetting {
    key: String,
    #[serde(rename = "type")]
    ty: SettingType,
    default: toml::Value,
    #[serde(default)]
    choices: Vec<String>,
    min: Option<i64>,
    max: Option<i64>,
    #[serde(default)]
    scope: SettingScope,
    #[serde(default)]
    protected: bool,
    safer: Option<String>,
    doc: String,
}

/// One setting of a pack's own group, `packs.<pack>.<key>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingDecl {
    /// The key within the group.
    pub key: String,
    /// The type of its value.
    pub ty: SettingType,
    /// The value when nothing sets it.
    pub default: Value,
    /// The words allowed, for a choice.
    pub choices: Vec<String>,
    /// The least a whole number may be.
    pub min: Option<i64>,
    /// The most a whole number may be.
    pub max: Option<i64>,
    /// Where it may be set.
    pub scope: SettingScope,
    /// Whether it is a protected setting.
    pub protected: bool,
    /// Which direction is safer, in a word, for a protected setting.
    pub safer: Option<String>,
    /// What it does, in a sentence.
    pub doc: String,
}

/// Who a provider is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderSection {
    /// Its name.
    pub name: ProviderName,
    /// Its version.
    pub version: Version,
    /// Who published it.
    pub publisher: String,
    /// A pack or a plug-in.
    pub kind: ProviderKind,
    /// The digest of the program that runs, when the manifest states one.
    pub digest: Option<Digest>,
    /// A plug-in's detached signature file.
    pub signature: Option<String>,
}

/// A manifest that passed every check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    /// The digest of the manifest's text.
    pub digest: Digest,
    /// Who the provider is.
    pub provider: ProviderSection,
    /// How a plug-in is started.
    pub run: Option<RunSection>,
    /// The modes it brings.
    pub modes: Vec<ModeDecl>,
    /// The surface kinds it serves.
    pub surface_kinds: Vec<SurfaceKindDecl>,
    /// The kinds of other providers it reads.
    pub reads: Vec<ReadDecl>,
    /// The extension points it fills.
    pub points: Points,
    /// Its discoverers.
    pub discoverers: Vec<DiscovererDecl>,
    /// The file kinds it owns.
    pub file_kinds: Vec<FileKindDecl>,
    /// The finding kinds it owns.
    pub finding_kinds: Vec<FindingKindDecl>,
    /// The controls it can operate.
    pub controls: Vec<ControlName>,
    /// Its lint rules.
    pub lint_rules: Vec<LintRuleDecl>,
    /// Its drafting rules.
    pub drafting_rules: Vec<DraftingRuleDecl>,
    /// The actions it adds.
    pub actions: Vec<ActionDecl>,
    /// The failure codes its drivers may return.
    pub failure_codes: Vec<FailureCodeDecl>,
    /// What it may do.
    pub effects: Effects,
    /// What it needs on the machine.
    pub requirements: Vec<Requirement>,
    /// The hosts it supports, such as `linux-x86_64`. Empty means all.
    pub platforms: Vec<String>,
    /// What its findings may claim.
    pub evidence: Option<EvidenceDecl>,
    /// Its own settings group.
    pub settings: Vec<SettingDecl>,
}

fn invalid(field: &str, why: &str) -> Failure {
    codes::failure(
        &codes::row::MANIFEST_INVALID,
        format!("The manifest field `{field}` is wrong: {why}."),
        "Correct the provider's manifest.",
    )
}

fn undeclared(what: String) -> Failure {
    codes::failure(
        &codes::row::EFFECT_NOT_DECLARED,
        what,
        "Declare it in the provider's manifest, where a person can review it, or remove what asks for it.",
    )
}

/// A digest as a manifest writes it: `sha256:` and 64 hexadecimal
/// characters.
fn digest(field: &str, text: &str) -> Result<Digest, Failure> {
    text.strip_prefix("sha256:")
        .and_then(|hex| hex.parse::<Digest>().ok())
        .ok_or_else(|| {
            invalid(
                field,
                "a digest is `sha256:` and 64 lowercase hexadecimal characters",
            )
        })
}

/// Whether the text is an absolute path on any operating system.
fn is_absolute_anywhere(path: &str) -> bool {
    let bytes = path.as_bytes();
    path.starts_with('/')
        || path.starts_with('\\')
        || (bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && (bytes[2] == b'/' || bytes[2] == b'\\'))
}

/// A program is a relative path inside the provider or a fixed absolute
/// path. Never a bare name to be searched for, and never a way out.
fn program_path(field: &str, path: &str) -> Result<(), Failure> {
    let refused = |why: &str| {
        codes::failure(
            &codes::row::PATH_NOT_ALLOWED,
            format!(
                "The manifest field `{field}` names the program `{}`: {why}.",
                shown(path)
            ),
            "Name the program by a path inside the provider, or by one fixed absolute path.",
        )
    };
    if path.is_empty() || path.chars().any(char::is_control) {
        return Err(refused("it is empty or holds a control character"));
    }
    if path.split(['/', '\\']).any(|part| part == "..") {
        return Err(refused("it steps through `..`"));
    }
    if is_absolute_anywhere(path) {
        return Ok(());
    }
    if path.contains(':') || path.contains('\\') || path.starts_with('~') {
        return Err(refused(
            "a relative program path uses forward slashes and no drive",
        ));
    }
    Ok(())
}

/// The line, from one, that holds the byte at `offset`.
fn line_of(text: &str, offset: usize) -> usize {
    text.bytes()
        .take(offset)
        .filter(|byte| *byte == b'\n')
        .count()
        + 1
}

fn unique<'a, T: Ord + 'a>(items: impl Iterator<Item = &'a T>) -> bool {
    let mut seen = BTreeSet::new();
    items.into_iter().all(|item| seen.insert(item))
}

fn to_value(value: &toml::Value) -> Option<Value> {
    match value {
        toml::Value::Boolean(flag) => Some(Value::Bool(*flag)),
        toml::Value::Integer(number) => Some(Value::Int(*number)),
        toml::Value::String(text) => Some(Value::Text(text.clone())),
        toml::Value::Array(items) => items
            .iter()
            .map(|item| item.as_str().map(|text| Value::Text(text.to_owned())))
            .collect::<Option<Vec<Value>>>()
            .map(Value::List),
        _ => None,
    }
}

impl SettingDecl {
    /// Whether `value` is one this setting takes, and if not, why.
    fn fits(&self, value: &Value) -> Result<(), String> {
        match (self.ty, value) {
            (SettingType::Bool, Value::Bool(_)) | (SettingType::Text, Value::Text(_)) => Ok(()),
            (SettingType::Int, Value::Int(number)) => {
                if self.min.is_some_and(|min| *number < min)
                    || self.max.is_some_and(|max| *number > max)
                {
                    Err("the number is outside the range the setting allows".to_owned())
                } else {
                    Ok(())
                }
            }
            (SettingType::Choice, Value::Text(word)) => {
                if self.choices.contains(word) {
                    Ok(())
                } else {
                    Err(format!("it is one of: {}", self.choices.join(", ")))
                }
            }
            (SettingType::List, Value::List(items)) => {
                if items.iter().all(|item| item.as_text().is_some()) {
                    Ok(())
                } else {
                    Err("every item of the list is text".to_owned())
                }
            }
            _ => Err(format!(
                "it is {}",
                match self.ty {
                    SettingType::Bool => "true or false",
                    SettingType::Int => "a whole number",
                    SettingType::Text => "text",
                    SettingType::Choice => "one word",
                    SettingType::List => "a list of texts",
                }
            )),
        }
    }
}

impl Manifest {
    /// Reads a manifest and makes every check that one manifest allows on
    /// its own. The checks that need the other providers are the registry's.
    ///
    /// # Errors
    ///
    /// `input`/`manifest_invalid` with the line for a file that does not
    /// parse, has a key the format does not know, or holds a badly formed
    /// name; `input`/`contract_unsupported`; `input`/`mode_conflict` for a
    /// mode or surface kind named twice; `input`/`path_not_allowed` for a
    /// program path; `policy`/`effect_not_declared` when a point, a
    /// discoverer, or a session asks for what the manifest does not declare.
    #[allow(clippy::too_many_lines)] // one check after another, each a few lines; splitting hides the order
    pub fn parse(text: &str) -> Result<Self, Failure> {
        let raw: RawManifest = toml::from_str(text).map_err(|error| {
            codes::failure(
                &codes::row::MANIFEST_INVALID,
                format!("The manifest cannot be read: {}", shown(error.message())),
                match error.span() {
                    Some(span) => format!(
                        "Correct line {} of the provider's manifest.",
                        line_of(text, span.start)
                    ),
                    None => "Correct the provider's manifest.".to_owned(),
                },
            )
        })?;

        if raw.manifest_format != MANIFEST_FORMAT {
            return Err(invalid("manifest_format", "this build reads format 1"));
        }
        if raw.contract != CONTRACT {
            return Err(codes::failure(
                &codes::row::CONTRACT_UNSUPPORTED,
                format!(
                    "The manifest was written for provider contract {}, and this build offers contract {CONTRACT}.",
                    raw.contract
                ),
                "Use a version of the provider written for this contract.",
            ));
        }

        let name = ProviderName::new(&raw.provider.name)?;
        if name.as_str() == "kernel" {
            return Err(invalid(
                "provider.name",
                "`kernel` is the kernel's own name",
            ));
        }
        let provider = ProviderSection {
            digest: raw
                .provider
                .digest
                .as_deref()
                .map(|text| digest("provider.digest", text))
                .transpose()?,
            name,
            version: raw.provider.version,
            publisher: raw.provider.publisher,
            kind: raw.provider.kind,
            signature: raw.provider.signature,
        };

        let run = match (provider.kind, raw.run) {
            (ProviderKind::Pack, Some(_)) => {
                return Err(invalid("run", "a pack is compiled in and is not started"));
            }
            (ProviderKind::Plugin, None) => {
                return Err(invalid("run", "a plug-in states how it is started"));
            }
            (_, None) => None,
            (_, Some(run)) => {
                program_path("run.program", &run.program)?;
                if is_absolute_anywhere(&run.program) {
                    return Err(invalid(
                        "run.program",
                        "a plug-in's program is inside the plug-in",
                    ));
                }
                if run.protocol.is_empty() || run.concurrency == 0 {
                    return Err(invalid(
                        "run",
                        "it names a protocol version and serves at least one request",
                    ));
                }
                Some(RunSection {
                    program: run.program,
                    args: run.args,
                    environment: run.environment,
                    protocol: run.protocol,
                    max_frame_bytes: run.max_frame_bytes,
                    concurrency: run.concurrency,
                    queue: run.queue,
                })
            }
        };

        // Modes and surface kinds: one entry, one archetype.
        if !unique(raw.modes.iter().map(|mode| &mode.name)) {
            return Err(codes::failure(
                &codes::row::MODE_CONFLICT,
                format!("The provider `{}` registers one mode twice.", provider.name),
                "State each mode once, with its one archetype.",
            ));
        }
        if !unique(raw.surface_kinds.iter().map(|kind| &kind.name)) {
            return Err(codes::failure(
                &codes::row::MODE_CONFLICT,
                format!(
                    "The provider `{}` registers one surface kind twice.",
                    provider.name
                ),
                "State each surface kind once, with its one archetype.",
            ));
        }
        for mode in &raw.modes {
            if !unique(mode.steps.iter()) || !unique(mode.verbs.iter()) {
                return Err(invalid(
                    "modes",
                    "a mode lists each step kind and each verb once",
                ));
            }
            if !unique(mode.commit_steps.iter())
                || mode
                    .commit_steps
                    .iter()
                    .any(|kind| !mode.steps.contains(kind))
            {
                return Err(invalid(
                    "modes",
                    "a step kind that always commits is listed once, and is one of the mode's step kinds",
                ));
            }
            if !unique(mode.absence_steps.iter())
                || mode
                    .absence_steps
                    .iter()
                    .any(|kind| !mode.steps.contains(kind) || mode.commit_steps.contains(kind))
            {
                return Err(invalid(
                    "modes",
                    "a step kind whose absence is authoritative is listed once, is one of the mode's step kinds, and never commits",
                ));
            }
            if mode.session_reads == 0 || mode.session_writes == 0 {
                return Err(invalid(
                    "modes",
                    "a session allows at least one read and one write at a time",
                ));
            }
        }
        if !unique(raw.file_kinds.iter().map(|kind| &kind.name))
            || !unique(raw.finding_kinds.iter().map(|kind| &kind.name))
            || !unique(raw.lint_rules.iter().map(|rule| &rule.code))
            || !unique(raw.controls.iter().map(|control| &control.name))
            || !unique(raw.failure_codes.iter().map(|code| &code.code))
        {
            return Err(invalid(
                "file_kinds, finding_kinds, lint_rules, controls, failure_codes",
                "a name is listed twice",
            ));
        }
        for kind in &raw.file_kinds {
            if kind.surface_kinds.is_empty()
                || FileKindName::new(kind.directory.trim_end_matches('/')).is_err()
            {
                return Err(invalid(
                    "file_kinds",
                    "a file kind names at least one surface kind, and its directory is one lowercase name",
                ));
            }
        }
        if raw
            .finding_kinds
            .iter()
            .any(|kind| kind.surface_kinds.is_empty())
        {
            return Err(invalid(
                "finding_kinds",
                "a finding kind names at least one surface kind",
            ));
        }
        for rule in &raw.lint_rules {
            if rule.code.owner() != provider.name.as_str() {
                return Err(invalid(
                    "lint_rules",
                    "a lint code starts with the provider's own name",
                ));
            }
        }
        for code in &raw.failure_codes {
            let other_class = codes::ALL
                .iter()
                .any(|(class, text)| *text == code.code.as_str() && *class != code.class);
            if other_class {
                return Err(invalid(
                    "failure_codes",
                    &format!(
                        "`{}` is a code of the provider contract and keeps its own class, not `{}`",
                        code.code, code.class
                    ),
                ));
            }
            if codes::is_kernel_only(&code.code) {
                return Err(invalid(
                    "failure_codes",
                    "a code that states the kernel's own bookkeeping is the kernel's alone, and no provider declares it",
                ));
            }
        }

        // Points: only what is declared can be registered.
        let declared_modes: BTreeSet<&Mode> = raw.modes.iter().map(|mode| &mode.name).collect();
        for mode in &raw.points.driver {
            if !declared_modes.contains(mode) {
                return Err(undeclared(format!(
                    "The provider `{}` asks to drive the mode `{mode}`, which its manifest does not declare under `modes`.",
                    provider.name
                )));
            }
        }
        let mut discoverers = Vec::new();
        for found in raw.discoverers {
            if !matches!(
                found.max_level,
                EvidenceLevel::Inferred | EvidenceLevel::Discovered
            ) {
                return Err(invalid(
                    "discoverers.max_level",
                    "a discoverer claims `inferred` or `discovered`",
                ));
            }
            for kind in &found.kinds {
                let owned = raw.finding_kinds.iter().any(|own| own.name == *kind);
                let read = raw
                    .reads
                    .iter()
                    .any(|read| read.of == KindOf::FindingKinds && read.kind == kind.as_str());
                if !owned && !read {
                    return Err(undeclared(format!(
                        "The discoverer `{}` of `{}` returns `{}` findings, a kind the manifest neither owns nor reads.",
                        shown(&found.name),
                        provider.name,
                        kind.as_str()
                    )));
                }
            }
            if !found.files.iter().all(|file| is_file_name(file)) {
                return Err(invalid(
                    "discoverers.files",
                    "a file a discoverer reads is named in lowercase letters, digits, and underscores, starting with a letter, in at most 32 bytes",
                ));
            }
            if !unique(found.files.iter()) {
                return Err(invalid(
                    "discoverers.files",
                    "a discoverer lists one file name twice",
                ));
            }
            discoverers.push(DiscovererDecl {
                name: DiscovererName::new(&found.name)?,
                archetype: found.archetype,
                surface_kinds: found.surface_kinds,
                kinds: found.kinds,
                sources: found.sources,
                max_level: found.max_level,
                bounds: found.bounds,
                files: found.files,
            });
        }
        if !unique(discoverers.iter().map(|d| &d.name)) {
            return Err(invalid("discoverers", "a discoverer is listed twice"));
        }
        let mut point_discoverers = BTreeSet::new();
        for name in &raw.points.discoverer {
            let name = DiscovererName::new(name)?;
            if !discoverers.iter().any(|d| d.name == name) {
                return Err(undeclared(format!(
                    "The provider `{}` asks to register the discoverer `{name}`, which its manifest does not describe under `discoverers`.",
                    provider.name
                )));
            }
            point_discoverers.insert(name);
        }
        let points = Points {
            driver: raw.points.driver.into_iter().collect(),
            discoverer: point_discoverers,
            auth_provider: raw.points.auth_provider.into_iter().collect(),
            source_reader: raw.points.source_reader.into_iter().collect(),
            widget_driver: raw.points.widget_driver.into_iter().collect(),
        };

        // Effects.
        let roots = |field: &str, names: &[String]| -> Result<Vec<RootName>, Failure> {
            names
                .iter()
                .map(|name| {
                    RootName::new(name)
                        .map_err(|_| invalid(field, "a root is a lowercase name, never a path"))
                })
                .collect()
        };
        let mut processes = Vec::new();
        for program in &raw.effects.processes {
            program_path("effects.processes", &program.path)?;
            // A plug-in has a folder of its own, and a relative path is
            // inside it. A pack is compiled in and has none: a relative
            // path would be looked for in the engine's own working folder.
            if provider.kind == ProviderKind::Pack && !is_absolute_anywhere(&program.path) {
                return Err(codes::failure(
                    &codes::row::PATH_NOT_ALLOWED,
                    format!(
                        "The manifest field `effects.processes` names the program `{}` by a relative path, and a pack has no folder of its own.",
                        shown(&program.path)
                    ),
                    "Name the program by one fixed absolute path.",
                ));
            }
            processes.push(ProgramDecl {
                path: program.path.clone(),
                digest: program
                    .digest
                    .as_deref()
                    .map(|text| digest("effects.processes.digest", text))
                    .transpose()?,
            });
        }
        let effects = Effects {
            processes,
            plan_programs: raw.effects.plan_programs,
            read_roots: roots("effects.read_roots", &raw.effects.read_roots)?,
            write_roots: roots("effects.write_roots", &raw.effects.write_roots)?,
            network: raw.effects.network,
            sessions: raw.effects.sessions,
            target_code: raw.effects.target_code,
            attach: raw.effects.attach,
            screen: raw.effects.screen,
            devices: raw.effects.devices,
        };
        for kind in &raw.surface_kinds {
            if kind.session && !effects.sessions.contains(&kind.name) {
                return Err(undeclared(format!(
                    "The provider `{}` holds a session on `{}` surfaces, and its manifest does not list that kind under `effects.sessions`.",
                    provider.name, kind.name
                )));
            }
        }
        if !points.auth_provider.is_empty() && effects.sessions.is_empty() {
            return Err(undeclared(format!(
                "The provider `{}` brings an auth provider, and its manifest declares no session under `effects.sessions`.",
                provider.name
            )));
        }

        let mut actions = Vec::new();
        for action in raw.actions {
            let name = ActionName::new(&action.name)?;
            if name.provider() != provider.name.as_str() {
                return Err(invalid(
                    "actions",
                    "an action is named under the provider's own name",
                ));
            }
            actions.push(ActionDecl {
                name,
                effect: action.effect,
            });
        }
        if !unique(actions.iter().map(|action| &action.name)) {
            return Err(invalid("actions", "an action is listed twice"));
        }

        let mut requirements = Vec::new();
        for needed in raw.requirements {
            if needed.detect.paths.is_empty()
                || needed
                    .detect
                    .paths
                    .iter()
                    .any(|path| !is_absolute_anywhere(path))
            {
                return Err(codes::failure(
                    &codes::row::PATH_NOT_ALLOWED,
                    format!(
                        "The requirement `{}` is looked for at a place that is not a fixed absolute path.",
                        shown(&needed.name)
                    ),
                    "List the absolute paths where the program is installed. Nothing is searched for.",
                ));
            }
            requirements.push(Requirement {
                known_digests: needed
                    .known_digests
                    .iter()
                    .map(|text| digest("requirements.known_digests", text))
                    .collect::<Result<_, _>>()?,
                name: needed.name,
                paths: needed.detect.paths,
                versions: needed.versions,
                missing: needed.missing,
            });
        }

        let mut settings = Vec::new();
        for setting in raw.settings {
            let ok_key = FileKindName::new(&setting.key).is_ok();
            let default = to_value(&setting.default);
            let (true, Some(default)) = (ok_key, default) else {
                return Err(invalid(
                    "settings",
                    "a key is a lowercase name and a default is a flag, a number, text, or a list of texts",
                ));
            };
            let decl = SettingDecl {
                key: setting.key,
                ty: setting.ty,
                default,
                choices: setting.choices,
                min: setting.min,
                max: setting.max,
                scope: setting.scope,
                protected: setting.protected,
                safer: setting.safer,
                doc: setting.doc,
            };
            if decl.doc.trim().is_empty() {
                return Err(invalid("settings", "every setting is documented"));
            }
            if decl.protected && decl.safer.is_none() {
                return Err(invalid(
                    "settings",
                    "a protected setting states its safer direction",
                ));
            }
            if let Err(why) = decl.fits(&decl.default) {
                return Err(invalid(
                    "settings",
                    &format!("the default of `{}` does not fit: {why}", decl.key),
                ));
            }
            settings.push(decl);
        }
        if !unique(settings.iter().map(|setting| &setting.key)) {
            return Err(invalid("settings", "a key is listed twice"));
        }

        Ok(Self {
            digest: Digest::of(text.as_bytes()),
            provider,
            run,
            modes: raw.modes,
            surface_kinds: raw.surface_kinds,
            reads: raw.reads,
            points,
            discoverers,
            file_kinds: raw.file_kinds,
            finding_kinds: raw.finding_kinds,
            controls: raw
                .controls
                .into_iter()
                .map(|control| control.name)
                .collect(),
            lint_rules: raw.lint_rules,
            drafting_rules: raw.drafting_rules,
            actions,
            failure_codes: raw.failure_codes,
            effects,
            requirements,
            platforms: raw.platforms.supported,
            evidence: raw.evidence,
            settings,
        })
    }

    /// The declaration of a mode.
    #[must_use]
    pub fn mode(&self, mode: &Mode) -> Option<&ModeDecl> {
        self.modes.iter().find(|decl| decl.name == *mode)
    }

    /// Whether the provider owns the kind for the surface kind.
    #[must_use]
    pub fn owns(&self, of: KindOf, kind: &str, surface_kind: &SurfaceKind) -> bool {
        match of {
            KindOf::FileKinds => self
                .file_kinds
                .iter()
                .any(|own| own.name.as_str() == kind && own.surface_kinds.contains(surface_kind)),
            KindOf::FindingKinds => self
                .finding_kinds
                .iter()
                .any(|own| own.name.as_str() == kind && own.surface_kinds.contains(surface_kind)),
        }
    }

    /// Checks a pack's settings group against the schema this manifest
    /// declares, and fills in every default.
    ///
    /// # Errors
    ///
    /// `input`/`pack_settings_invalid` for a key the manifest does not
    /// declare or a value that does not fit.
    pub fn validate_settings(&self, given: &Values) -> Result<Values, Failure> {
        let wrong = |key: &str, why: &str| {
            codes::failure(
                &codes::row::PACK_SETTINGS_INVALID,
                format!(
                    "The setting `packs.{}.{}` is wrong: {why}.",
                    self.provider.name,
                    shown(key)
                ),
                "Correct the setting, or remove it to take the default.",
            )
        };
        let declared: BTreeMap<&str, &SettingDecl> = self
            .settings
            .iter()
            .map(|setting| (setting.key.as_str(), setting))
            .collect();
        for (key, value) in given.iter() {
            let decl = declared
                .get(key)
                .ok_or_else(|| wrong(key, "the pack declares no such setting"))?;
            decl.fits(value).map_err(|why| wrong(key, &why))?;
        }
        let mut validated = Values::new();
        for setting in &self.settings {
            let value = given.get(&setting.key).unwrap_or(&setting.default).clone();
            validated.insert(setting.key.clone(), value);
        }
        Ok(validated)
    }
}
