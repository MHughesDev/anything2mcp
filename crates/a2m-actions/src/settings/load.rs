//! The loader: eight layers, lowest first, and what each may set.
//!
//! 1 built-in defaults, 2 the deployment profile, 3 a preset, 4 the
//! installation's file, 5 the user's file, 6 the project's file, 7
//! environment variables, 8 a command's flags. A higher layer wins, key by
//! key. Precedence does not let a layer loosen anything: layers 5 to 8 may
//! tighten a protected setting and never loosen it, a preset likewise, and
//! an installation-only setting is read from the installation file and the
//! flags of `serve` alone. An unknown key is an error, never ignored.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use a2m_core::{Digest, Failure, FieldHasher, Mode, Value, Values};
use a2m_provider::{Manifest, ProviderName, ProviderSettings, SettingScope, SettingType};
use serde_json::{Value as Json, json};
use toml::de::{DeTable, DeValue};

use super::keeper::{InstallationKeeper, read_installation_file, system_installation_folder};
use super::schema::{self, Built, Effective, Kind, PRESETS, SCHEMA, Safer, Scope};
use crate::codes;
use crate::paths::Paths;

/// The name of the installation's and the user's settings file.
pub const SETTINGS_FILE_NAME: &str = "settings.toml";
/// The name of the project's file.
pub const PROJECT_FILE_NAME: &str = "anything2mcp.toml";
/// The folder of a site's own presets, in the installation's configuration
/// directory.
pub const PRESETS_FOLDER_NAME: &str = "presets";
/// What every settings variable starts with.
pub const ENV_PREFIX: &str = "A2M_";

/// A layer. A higher one overrides a lower one, key by key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Layer {
    /// The binary's own defaults.
    Default = 1,
    /// The deployment shape, under the settings keys `profile.*` (decision 0402).
    DeploymentShape = 2,
    /// A preset.
    Preset = 3,
    /// The installation's settings file.
    Installation = 4,
    /// The user's settings file.
    User = 5,
    /// The project's file.
    Project = 6,
    /// Environment variables.
    Environment = 7,
    /// A command's flags.
    Flag = 8,
}

impl Layer {
    /// The layer's name, as `config.show` prints it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::DeploymentShape => "profile",
            Self::Preset => "preset",
            Self::Installation => "installation",
            Self::User => "user",
            Self::Project => "project",
            Self::Environment => "environment",
            Self::Flag => "flag",
        }
    }

    /// Whether the layer may only tighten a protected setting.
    const fn only_tightens(self) -> bool {
        matches!(
            self,
            Self::Preset | Self::User | Self::Project | Self::Environment | Self::Flag
        )
    }
}

/// A value a lower layer set, which a higher one overrode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Overridden {
    /// The value.
    pub value: Value,
    /// The layer that set it.
    pub layer: Layer,
    /// Where: a file and line, a variable, a flag, a preset, or the binary.
    pub origin: String,
}

/// One effective setting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The value in force.
    pub value: Value,
    /// The layer that set it.
    pub layer: Layer,
    /// Where: a file and line, a variable, a flag, a preset, or the binary.
    pub origin: String,
    /// Every lower layer that set it and was overridden, lowest first.
    pub overridden: Vec<Overridden>,
    /// Whether the installation file locks it.
    pub locked: bool,
    /// Whether it is a protected setting.
    pub protected: bool,
    /// Where it may be set.
    pub scope: Scope,
    /// When a change takes effect.
    pub effective: Effective,
    /// What it does.
    pub doc: String,
    /// Whether the part it governs is built. A key that is not built keeps
    /// its default: a value written for it is refused.
    pub built: Built,
}

/// One preset, as `config.show` lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresetInfo {
    /// Its name.
    pub name: String,
    /// What it is for; the file it came from, for a site's own.
    pub purpose: String,
    /// The keys it sets, with their values.
    pub sets: Vec<(String, Value)>,
    /// Whether it is the one in force.
    pub chosen: bool,
}

/// One settings file: where it is, and its text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFile {
    /// Its path, for messages.
    pub path: PathBuf,
    /// Its text.
    pub text: String,
}

impl SourceFile {
    /// A file at `path` holding `text`.
    #[must_use]
    pub fn new(path: impl Into<PathBuf>, text: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            text: text.into(),
        }
    }
}

/// Everything the loader reads. Nothing here is read from disk by the
/// loader itself: [`Sources::read`] does that, and a test builds one in
/// memory.
#[derive(Debug, Clone, Default)]
pub struct Sources {
    /// The installation's settings file, when there is one.
    pub installation: Option<SourceFile>,
    /// Who may change the installation's settings file. Only a file no
    /// account but an administrator can change may loosen a protected
    /// setting or lock a key; any other is held to the rule of the
    /// person's own layers. [`Sources::read`] checks the file on disk; the
    /// default is not checked, which is held to the person's rule. Outside
    /// this crate only a test sets it, through
    /// [`Sources::with_installation_keeper_for_tests`]: the field is to
    /// become private once the product crate sets it that way too
    /// (decision 0273).
    pub installation_keeper: InstallationKeeper,
    /// A site's own presets; each file's stem is the preset's name.
    pub presets: Vec<SourceFile>,
    /// The user's settings file.
    pub user: Option<SourceFile>,
    /// The project's file, `anything2mcp.toml`.
    pub project: Option<SourceFile>,
    /// The process environment. Only `A2M_<GROUP>__<KEY>` variables are
    /// settings.
    pub environment: BTreeMap<String, String>,
    /// A command's flags, as key and written value.
    pub flags: Vec<(String, String)>,
    /// Whether the flags are those of `serve`, which alone may set an
    /// installation-only setting.
    pub serve: bool,
    /// The manifest of every installed pack, whose `[[settings]]` are the
    /// schema of its group.
    pub packs: Vec<Arc<Manifest>>,
}

impl Sources {
    /// No file, no variable, no flag: the empty configuration.
    #[must_use]
    pub fn none() -> Self {
        Self::default()
    }

    /// Reads the files the paths name: the installation's and the user's
    /// settings files, the site's presets, and the project's file. A file
    /// that is not there is not a problem.
    ///
    /// # Errors
    ///
    /// `input`/`settings_invalid` for a file that exists and cannot be read
    /// as text.
    pub fn read(
        paths: &Paths,
        project: Option<&Path>,
        environment: BTreeMap<String, String>,
    ) -> Result<Self, Failure> {
        let installation_path = paths.installation_config.join(SETTINGS_FILE_NAME);
        // Checked and read as one file: the check ends at the file's entry,
        // the file is opened there without following a link, and the text
        // is read from the handle whose owner and mode were checked
        // (decision 0273).
        let (installation, installation_keeper) = match read_installation_file(&installation_path) {
            Ok((text, keeper)) => (Some(SourceFile::new(&installation_path, text)), keeper),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                (None, InstallationKeeper::default())
            }
            Err(_) => return Err(unreadable(&installation_path)),
        };
        let user = read_file(&paths.config.join(SETTINGS_FILE_NAME))?;
        let project = match project {
            Some(folder) => read_file(&folder.join(PROJECT_FILE_NAME))?,
            None => None,
        };
        let mut presets = Vec::new();
        let folder = paths.installation_config.join(PRESETS_FOLDER_NAME);
        if let Ok(listing) = std::fs::read_dir(&folder) {
            let mut files: Vec<PathBuf> = listing
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| path.extension().is_some_and(|ext| ext == "toml"))
                .collect();
            files.sort();
            for path in files {
                if let Some(file) = read_file(&path)? {
                    presets.push(file);
                }
            }
        }
        Ok(Self {
            installation,
            installation_keeper,
            presets,
            user,
            project,
            environment,
            flags: Vec::new(),
            serve: false,
            packs: Vec::new(),
        })
    }

    /// The same sources, with one more pack's manifest.
    #[must_use]
    pub fn with_pack(mut self, manifest: Arc<Manifest>) -> Self {
        self.packs.push(manifest);
        self
    }

    /// The same sources, with one more pack's manifest, from its text.
    ///
    /// # Errors
    ///
    /// What [`Manifest::parse`] answers.
    pub fn with_pack_manifest(self, text: &str) -> Result<Self, Failure> {
        Ok(self.with_pack(Arc::new(Manifest::parse(text)?)))
    }

    /// The same sources, with the installation's settings file taken as
    /// kept by `keeper`, in place of the check of the file on disk. For a
    /// test alone: no product source but the product's own test switch
    /// names it, and a test of this crate holds that (decisions 0241 and
    /// 0273).
    #[doc(hidden)]
    #[must_use]
    pub fn with_installation_keeper_for_tests(mut self, keeper: InstallationKeeper) -> Self {
        self.installation_keeper = keeper;
        self
    }

    /// The same sources, with one more flag.
    #[must_use]
    pub fn with_flag(mut self, key: &str, value: &str) -> Self {
        self.flags.push((key.to_owned(), value.to_owned()));
        self
    }
}

fn read_file(path: &Path) -> Result<Option<SourceFile>, Failure> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(SourceFile::new(path, text))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(unreadable(path)),
    }
}

fn unreadable(path: &Path) -> Failure {
    codes::input(
        codes::SETTINGS_INVALID,
        format!("The settings file `{}` cannot be read.", path.display()),
        "Make the file readable text, or remove it.",
    )
}

/// One value as a layer wrote it.
struct Raw {
    key: String,
    value: Written,
    origin: String,
}

/// A value before its key's type is known.
enum Written {
    /// From a file: already typed.
    Typed(Value),
    /// From a variable or a flag: text, read by the key's type.
    Text(String),
    /// A value of a type no setting takes, such as a number with a
    /// fraction.
    Unusable,
}

/// What the schema, or a pack's manifest, says about one key.
struct Rule {
    scope: Scope,
    protected: Option<Safer>,
    effective: Effective,
    doc: String,
    built: Built,
    kind: RuleKind,
}

enum RuleKind {
    Kernel(Kind),
    Pack {
        manifest: Arc<Manifest>,
        key: String,
        ty: SettingType,
    },
}

impl Rule {
    fn read(&self, text: &str) -> Option<Value> {
        let list = |text: &str| {
            Value::List(
                text.split(',')
                    .map(str::trim)
                    .filter(|item| !item.is_empty())
                    .map(|item| Value::Text(item.to_owned()))
                    .collect(),
            )
        };
        let (is_bool, is_int, is_list) = match &self.kind {
            RuleKind::Kernel(kind) => (
                matches!(kind, Kind::Bool),
                matches!(kind, Kind::Int { .. }),
                matches!(kind, Kind::List | Kind::Words(_)),
            ),
            RuleKind::Pack { ty, .. } => (
                *ty == SettingType::Bool,
                *ty == SettingType::Int,
                *ty == SettingType::List,
            ),
        };
        if is_bool {
            match text {
                "true" => Some(Value::Bool(true)),
                "false" => Some(Value::Bool(false)),
                _ => None,
            }
        } else if is_int {
            text.parse::<i64>().ok().map(Value::Int)
        } else if is_list {
            Some(list(text))
        } else {
            Some(Value::Text(text.to_owned()))
        }
    }

    /// Why `value` does not fit, or `None` when it does.
    fn misfit(&self, value: &Value) -> Option<String> {
        match &self.kind {
            RuleKind::Kernel(kind) => (!kind.fits(value)).then(|| kind.describe()),
            RuleKind::Pack { manifest, key, .. } => manifest
                .validate_settings(&Values::new().with(key.clone(), value.clone()))
                .err()
                .map(|failure| failure.message().to_owned()),
        }
    }
}

/// The effective settings, and where each value came from.
#[derive(Debug, Clone)]
pub struct Settings {
    entries: BTreeMap<String, Entry>,
    locks: Vec<String>,
    presets: Vec<PresetInfo>,
    refused: Vec<Failure>,
    packs: Vec<Arc<Manifest>>,
    digest: Digest,
    project_file: Option<PathBuf>,
}

impl Settings {
    /// Loads the settings as start-up does: any problem stops the load.
    ///
    /// # Errors
    ///
    /// The first problem, saying how many more there are:
    /// `input`/`settings_invalid` for an unknown key (with the nearest
    /// known keys), a value of the wrong type, or a file that is not TOML,
    /// each with its file and line; `policy`/`setting_protected` for a
    /// layer that loosens a protected setting or sets a key it may not;
    /// `policy`/`setting_locked` for a change to a locked key.
    pub fn load(sources: &Sources) -> Result<Self, Failure> {
        let (settings, problems) = Self::load_all(sources);
        match problems.split_first() {
            None => Ok(settings),
            Some((first, [])) => Err(first.clone()),
            Some((first, rest)) => Err(Failure::new(
                first.class(),
                first.code().clone(),
                format!(
                    "{} ({} more {} with the settings.)",
                    first.message(),
                    rest.len(),
                    if rest.len() == 1 {
                        "problem"
                    } else {
                        "problems"
                    }
                ),
                first.next_step().to_owned(),
            )),
        }
    }

    /// Loads the settings and reports every problem. A value that was
    /// refused is left out, so the result is what the remaining layers
    /// make; `doctor.run` and `config.show` use this.
    #[must_use]
    pub fn load_all(sources: &Sources) -> (Self, Vec<Failure>) {
        let mut loader = Loader::new(sources);
        loader.run(sources);
        loader.finish(sources.project.as_ref().map(|file| file.path.clone()))
    }

    /// The settings of the empty configuration: every default, the Solo
    /// profile, the `standard` preset.
    #[must_use]
    pub fn defaults() -> Self {
        Self::load_all(&Sources::none()).0
    }

    /// The value of a key.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.entries.get(key).map(|entry| &entry.value)
    }

    /// A text or word setting.
    #[must_use]
    pub fn text(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(Value::as_text)
    }

    /// A whole-number setting.
    #[must_use]
    pub fn int(&self, key: &str) -> Option<i64> {
        self.get(key).and_then(Value::as_int)
    }

    /// A true-or-false setting.
    #[must_use]
    pub fn flag(&self, key: &str) -> Option<bool> {
        self.get(key).and_then(Value::as_bool)
    }

    /// A list setting.
    #[must_use]
    pub fn list(&self, key: &str) -> Vec<String> {
        self.get(key)
            .and_then(Value::as_list)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_text)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// One effective setting, with where it came from.
    #[must_use]
    pub fn entry(&self, key: &str) -> Option<&Entry> {
        self.entries.get(key)
    }

    /// Every effective setting, in order of key.
    pub fn entries(&self) -> impl Iterator<Item = (&str, &Entry)> {
        self.entries
            .iter()
            .map(|(key, entry)| (key.as_str(), entry))
    }

    /// The keys the installation file locks, as it wrote them.
    #[must_use]
    pub fn locks(&self) -> &[String] {
        &self.locks
    }

    /// Every preset: the built-in ones, then the site's own.
    #[must_use]
    pub fn presets(&self) -> &[PresetInfo] {
        &self.presets
    }

    /// Every value that was refused, and why.
    #[must_use]
    pub fn refused(&self) -> &[Failure] {
        &self.refused
    }

    /// The project file whose values are layer 6, as the sources named it:
    /// for an engine, the project file of the folder it was started in.
    #[must_use]
    pub fn project_file(&self) -> Option<&Path> {
        self.project_file.as_deref()
    }

    /// What the project file wrote, key by key, as the loader took it:
    /// every value of layer 6, whether or not a higher layer overrode it.
    #[must_use]
    pub fn project_values(&self) -> BTreeMap<String, Value> {
        self.entries
            .iter()
            .filter_map(|(key, entry)| {
                let value = if entry.layer == Layer::Project {
                    Some(&entry.value)
                } else {
                    entry
                        .overridden
                        .iter()
                        .find(|lower| lower.layer == Layer::Project)
                        .map(|lower| &lower.value)
                }?;
                Some((key.clone(), value.clone()))
            })
            .collect()
    }

    /// The digest of the effective settings: every key and value, in order.
    /// Recorded at each start, so that a change between two starts is seen.
    #[must_use]
    pub const fn digest(&self) -> Digest {
        self.digest
    }

    /// What the provider registry is built from: the packs switched off,
    /// the provider chosen for a mode, and each pack's own group as it was
    /// written. The registry validates each group against its manifest once
    /// more and fills in the defaults.
    #[must_use]
    pub fn provider_settings(&self) -> ProviderSettings {
        let mut out = ProviderSettings::default();
        for name in self.list("providers.disabled") {
            if let Ok(name) = ProviderName::new(&name) {
                out.disabled.insert(name);
            }
        }
        for (key, entry) in &self.entries {
            if let Some(mode) = key.strip_prefix("providers.mode.") {
                if let (Ok(mode), Some(Ok(provider))) = (
                    Mode::new(mode),
                    entry.value.as_text().map(ProviderName::new),
                ) {
                    out.mode_providers.insert(mode, provider);
                }
            } else if let Some(rest) = key.strip_prefix("packs.")
                && let Some((pack, setting)) = rest.split_once('.')
                && entry.layer != Layer::Default
                && let Ok(pack) = ProviderName::new(pack)
            {
                out.packs
                    .entry(pack)
                    .or_default()
                    .insert(setting.to_owned(), entry.value.clone());
            }
        }
        out
    }

    /// The settings as `config.show` prints them: every key with its value,
    /// the layer that set it and where, whether it is locked or protected,
    /// every lower layer that was overridden, the presets, and every value
    /// that was refused. With a key, that key alone, with its documentation.
    ///
    /// # Errors
    ///
    /// `input`/`not_found` for a key the schema does not declare, with the
    /// nearest known keys.
    pub fn show(&self, key: Option<&str>) -> Result<Json, Failure> {
        let shown = |key: &str, entry: &Entry, doc: bool| {
            let mut out = serde_json::Map::new();
            out.insert("key".to_owned(), json!(key));
            out.insert("value".to_owned(), value_json(&entry.value));
            out.insert("layer".to_owned(), json!(entry.layer.as_str()));
            out.insert("origin".to_owned(), json!(entry.origin));
            out.insert("protected".to_owned(), json!(entry.protected));
            out.insert("locked".to_owned(), json!(entry.locked));
            out.insert("built".to_owned(), json!(entry.built.is_built()));
            if let Built::No {
                instead,
                arrives_with,
            } = entry.built
            {
                out.insert("instead".to_owned(), json!(instead));
                out.insert("arrives_with".to_owned(), json!(arrives_with));
            }
            if !entry.overridden.is_empty() {
                out.insert(
                    "overridden".to_owned(),
                    Json::Array(
                        entry
                            .overridden
                            .iter()
                            .map(|lower| {
                                json!({
                                    "value": value_json(&lower.value),
                                    "layer": lower.layer.as_str(),
                                    "origin": lower.origin,
                                })
                            })
                            .collect(),
                    ),
                );
            }
            if doc {
                out.insert(
                    "doc".to_owned(),
                    json!(format!(
                        "{} Set at the scope `{}` and above; a change takes effect {}.",
                        entry.doc,
                        entry.scope.as_str(),
                        entry.effective.as_str()
                    )),
                );
            }
            Json::Object(out)
        };
        let settings: Vec<Json> = match key {
            Some(key) => {
                let entry = self.entries.get(key).ok_or_else(|| {
                    let near = codes::nearest(key, self.entries.keys().map(String::as_str));
                    codes::input(
                        codes::NOT_FOUND,
                        format!("There is no setting named `{}`.", codes::shown(key)),
                        if near.is_empty() {
                            "Call `config.show` with no key to list every setting.".to_owned()
                        } else {
                            format!("Use one of: {}.", near.join(", "))
                        },
                    )
                })?;
                vec![shown(key, entry, true)]
            }
            None => self
                .entries
                .iter()
                .map(|(key, entry)| shown(key, entry, false))
                .collect(),
        };
        Ok(json!({
            "settings": settings,
            "presets": self.presets.iter().map(|preset| {
                if preset.chosen {
                    format!("{} (in force): {}", preset.name, preset.purpose)
                } else {
                    format!("{}: {}", preset.name, preset.purpose)
                }
            }).collect::<Vec<_>>(),
            "locks": self.locks,
            // The one project file the engine applies, to every request
            // (decision 0241): the folder it started in.
            "project_file": self.project_file.as_ref().map(|path| path.display().to_string()),
            "refused": self.refused.iter().map(|failure| json!({
                "class": failure.class().to_string(),
                "code": failure.code().to_string(),
                "message": failure.message(),
                "fix": failure.next_step(),
            })).collect::<Vec<_>>(),
            "digest": self.digest.to_string(),
        }))
    }
}

fn value_json(value: &Value) -> Json {
    serde_json::to_value(value).unwrap_or(Json::Null)
}

struct Loader {
    entries: BTreeMap<String, Entry>,
    locks: Vec<String>,
    presets: Vec<PresetInfo>,
    problems: Vec<Failure>,
    packs: Vec<Arc<Manifest>>,
    serve: bool,
    /// Why the installation's file may not loosen, when it may not.
    installation_held: Option<String>,
}

impl Loader {
    fn new(sources: &Sources) -> Self {
        let mut entries = BTreeMap::new();
        for setting in SCHEMA.iter().filter(|setting| !setting.is_pattern()) {
            entries.insert(
                setting.key.to_owned(),
                Entry {
                    value: setting.default.value(),
                    layer: Layer::Default,
                    origin: "the built-in default".to_owned(),
                    overridden: Vec::new(),
                    locked: false,
                    protected: setting.protected.is_some(),
                    scope: setting.scope,
                    effective: setting.effective,
                    doc: setting.doc.to_owned(),
                    built: setting.built,
                },
            );
        }
        for manifest in &sources.packs {
            for decl in &manifest.settings {
                entries.insert(
                    format!("packs.{}.{}", manifest.provider.name, decl.key),
                    Entry {
                        value: decl.default.clone(),
                        layer: Layer::Default,
                        origin: format!("the default of the pack `{}`", manifest.provider.name),
                        overridden: Vec::new(),
                        locked: false,
                        protected: decl.protected,
                        scope: pack_scope(decl.scope),
                        effective: Effective::NextStart,
                        doc: decl.doc.clone(),
                        built: Built::Yes,
                    },
                );
            }
        }
        Self {
            entries,
            locks: Vec::new(),
            presets: Vec::new(),
            problems: Vec::new(),
            packs: sources.packs.clone(),
            serve: sources.serve,
            installation_held: sources
                .installation
                .as_ref()
                .and_then(|_| sources.installation_keeper.why_not()),
        }
    }

    fn run(&mut self, sources: &Sources) {
        // Every layer is read first, so that the profile and the preset,
        // which any file layer may name, are known before they are applied.
        let installation = sources
            .installation
            .as_ref()
            .map(|file| self.read_file(file, true))
            .unwrap_or_default();
        let user = sources
            .user
            .as_ref()
            .map(|file| self.read_file(file, false))
            .unwrap_or_default();
        let project = sources
            .project
            .as_ref()
            .map(|file| self.read_file(file, false))
            .unwrap_or_default();
        let environment: Vec<Raw> = sources
            .environment
            .iter()
            .filter_map(|(name, value)| {
                let rest = name.strip_prefix(ENV_PREFIX)?;
                rest.contains("__").then(|| Raw {
                    key: rest.to_ascii_lowercase().replace("__", "."),
                    value: Written::Text(value.clone()),
                    origin: format!("the variable `{name}`"),
                })
            })
            .collect();
        let flags: Vec<Raw> = sources
            .flags
            .iter()
            .map(|(key, value)| Raw {
                key: key.clone(),
                value: Written::Text(value.clone()),
                origin: format!("the flag `--{key}`"),
            })
            .collect();

        // Layer 2: the profile. Only the installation file and the flags of
        // `serve` may name it.
        let named = |layer: &[Raw], key: &str| {
            layer
                .iter()
                .rev()
                .find(|raw| raw.key == key)
                .and_then(|raw| match &raw.value {
                    Written::Typed(Value::Text(text)) | Written::Text(text) => Some(text.clone()),
                    _ => None,
                })
        };
        let profile = sources
            .serve
            .then(|| named(&flags, "profile.name"))
            .flatten()
            .or_else(|| named(&installation, "profile.name"))
            .unwrap_or_else(|| "solo".to_owned());
        for (key, value) in schema::profile_values(&profile) {
            self.apply(
                Raw {
                    key: key.to_owned(),
                    value: Written::Typed(value),
                    origin: format!("the profile `{profile}`"),
                },
                Layer::DeploymentShape,
            );
        }

        // Layer 3: one preset, named by the highest layer that names one.
        let chosen = named(&flags, "profile.preset")
            .or_else(|| named(&environment, "profile.preset"))
            .or_else(|| named(&user, "profile.preset"))
            .or_else(|| named(&installation, "profile.preset"))
            .unwrap_or_else(|| "standard".to_owned());
        self.apply_preset(sources, &chosen);

        for raw in installation {
            self.apply(raw, Layer::Installation);
        }
        for (layer, raws) in [
            (Layer::User, user),
            (Layer::Project, project),
            (Layer::Environment, environment),
            (Layer::Flag, flags),
        ] {
            for raw in raws {
                self.apply(raw, layer);
            }
        }
    }

    fn apply_preset(&mut self, sources: &Sources, chosen: &str) {
        let mut found = false;
        let mut all: Vec<(String, String, SourceFile)> = PRESETS
            .iter()
            .map(|preset| {
                (
                    preset.name.to_owned(),
                    preset.purpose.to_owned(),
                    SourceFile::new(format!("the preset `{}`", preset.name), preset.toml),
                )
            })
            .collect();
        for file in &sources.presets {
            let name = file
                .path
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_default();
            all.push((
                name,
                format!("A preset of this site, from `{}`.", file.path.display()),
                file.clone(),
            ));
        }
        for (name, purpose, file) in all {
            let is_chosen = name == chosen;
            // A preset that is not the chosen one is still read, so that
            // `config.show` can say what it sets; its problems are reported
            // only when it is chosen.
            let before = self.problems.len();
            let raws = self.read_file(&file, false);
            if !is_chosen {
                self.problems.truncate(before);
            }
            let sets: Vec<(String, Value)> = raws
                .iter()
                .filter_map(|raw| match &raw.value {
                    Written::Typed(value) => Some((raw.key.clone(), value.clone())),
                    _ => None,
                })
                .collect();
            self.presets.push(PresetInfo {
                name: name.clone(),
                purpose,
                sets,
                chosen: is_chosen,
            });
            if is_chosen {
                found = true;
                for mut raw in raws {
                    if raw.key.starts_with("profile.") {
                        self.problems.push(invalid(
                            &raw.origin,
                            format!("a preset may not set `{}`", raw.key),
                            "A preset cannot name a profile or another preset. Remove the key from the preset.",
                        ));
                        continue;
                    }
                    raw.origin = format!("the preset `{name}`");
                    self.apply(raw, Layer::Preset);
                }
            }
        }
        if !found {
            let known: Vec<String> = self
                .presets
                .iter()
                .map(|preset| preset.name.clone())
                .collect();
            self.problems.push(invalid(
                "`profile.preset`",
                format!("there is no preset named `{}`", codes::shown(chosen)),
                format!("Use one of: {}.", known.join(", ")),
            ));
        }
    }

    /// Reads one TOML file into dotted keys, each with its file and line.
    fn read_file(&mut self, file: &SourceFile, installation: bool) -> Vec<Raw> {
        let at = |offset: usize| {
            let line = file.text[..offset.min(file.text.len())]
                .bytes()
                .filter(|byte| *byte == b'\n')
                .count()
                + 1;
            format!("`{}`, line {line}", file.path.display())
        };
        let table = match DeTable::parse(&file.text) {
            Ok(table) => table,
            Err(error) => {
                let origin = error.span().map_or_else(
                    || format!("`{}`", file.path.display()),
                    |span| at(span.start),
                );
                self.problems.push(invalid(
                    &origin,
                    "the file is not valid TOML".to_owned(),
                    "Correct the file; settings are TOML tables of keys and values.",
                ));
                return Vec::new();
            }
        };
        let mut raws = Vec::new();
        let mut stack: Vec<(String, &DeTable<'_>)> = vec![(String::new(), table.get_ref())];
        while let Some((prefix, table)) = stack.pop() {
            for (name, value) in table {
                let key = if prefix.is_empty() {
                    name.get_ref().to_string()
                } else {
                    format!("{prefix}.{}", name.get_ref())
                };
                let origin = at(name.span().start);
                match value.get_ref() {
                    DeValue::Table(inner) => stack.push((key, inner)),
                    _ if key == "locks.keys" => {
                        if installation && let Some(why) = self.installation_held.clone() {
                            self.problems.push(Failure::new(
                                a2m_core::FailureClass::Policy,
                                codes::SETTING_PROTECTED,
                                format!(
                                    "{origin}: the installation's settings file may lock keys only when no account but an administrator can change it, and {why}."
                                ),
                                format!(
                                    "Remove `[locks]` from this file. An administrator locks keys in the installation's settings file in {}, which no other account can change; under `{}` the installation's folder is the person's own, and no lock is taken from it.",
                                    system_installation_folder(),
                                    crate::paths::HOME_VARIABLE
                                ),
                            ));
                        } else if !installation {
                            self.problems.push(Failure::new(
                                a2m_core::FailureClass::Policy,
                                codes::SETTING_PROTECTED,
                                format!(
                                    "{origin}: only the installation's settings file may lock keys."
                                ),
                                "Remove `[locks]` from this file.",
                            ));
                        } else if let Written::Typed(Value::List(items)) = written(value.get_ref())
                        {
                            self.locks
                                .extend(items.iter().filter_map(Value::as_text).map(str::to_owned));
                        } else {
                            self.problems.push(invalid(
                                &origin,
                                "`locks.keys` must be a list of keys".to_owned(),
                                "Write `keys = [\"policy.*\"]`.",
                            ));
                        }
                    }
                    other => raws.push(Raw {
                        key,
                        value: written(other),
                        origin,
                    }),
                }
            }
        }
        // The stack walks tables last-first; put the values back in the
        // order they were written, which is the order of their lines.
        raws.sort_by_key(|raw| line_of(&raw.origin));
        raws
    }

    fn rule(&self, key: &str) -> Option<Rule> {
        if let Some(setting) = schema::find(key) {
            return Some(Rule {
                scope: setting.scope,
                protected: setting.protected,
                effective: setting.effective,
                doc: setting.doc.to_owned(),
                built: setting.built,
                kind: RuleKind::Kernel(setting.kind),
            });
        }
        let (pack, setting) = key.strip_prefix("packs.")?.split_once('.')?;
        let manifest = self
            .packs
            .iter()
            .find(|manifest| manifest.provider.name.as_str() == pack)?;
        let decl = manifest.settings.iter().find(|decl| decl.key == setting)?;
        Some(Rule {
            scope: pack_scope(decl.scope),
            protected: decl.protected.then(|| {
                decl.safer
                    .as_deref()
                    .map_or(Safer::Unknown, Safer::from_word)
            }),
            effective: Effective::NextStart,
            doc: decl.doc.clone(),
            built: Built::Yes,
            kind: RuleKind::Pack {
                manifest: Arc::clone(manifest),
                key: setting.to_owned(),
                ty: decl.ty,
            },
        })
    }

    fn locked(&self, key: &str) -> Option<&str> {
        self.locks
            .iter()
            .find(|lock| {
                lock.as_str() == key
                    || lock
                        .strip_suffix(".*")
                        .is_some_and(|group| key.starts_with(&format!("{group}.")))
            })
            .map(String::as_str)
    }

    fn apply(&mut self, raw: Raw, layer: Layer) {
        let Some(rule) = self.rule(&raw.key) else {
            let known: Vec<String> = self.entries.keys().cloned().collect();
            let near = codes::nearest(&raw.key, known.iter().map(String::as_str));
            self.problems.push(invalid(
                &raw.origin,
                format!("`{}` is not a setting", codes::shown(&raw.key)),
                if near.is_empty() {
                    "Remove the key, or call `config.show` to list every setting.".to_owned()
                } else {
                    format!(
                        "Did you mean one of: {}? An unknown key is never ignored.",
                        near.join(", ")
                    )
                },
            ));
            return;
        };
        // A key whose part is not built is refused from every layer, a
        // value equal to its default too: a written value states an intent
        // nothing can honour (decision 0236).
        if let Built::No {
            instead,
            arrives_with,
        } = rule.built
        {
            self.problems.push(invalid(
                &raw.origin,
                format!(
                    "`{}` is a setting whose part is not built yet, so a value would have no effect. {instead}",
                    raw.key
                ),
                format!(
                    "Remove `{}` from this layer. It arrives with {arrives_with}.",
                    raw.key
                ),
            ));
            return;
        }
        let value = match raw.value {
            Written::Typed(value) => Some(value),
            Written::Text(text) => rule.read(&text),
            Written::Unusable => None,
        };
        let value = match value {
            Some(value) if rule.misfit(&value).is_none() => value,
            other => {
                let must = other
                    .as_ref()
                    .and_then(|value| rule.misfit(value))
                    .unwrap_or_else(|| match &rule.kind {
                        RuleKind::Kernel(kind) => kind.describe(),
                        RuleKind::Pack { .. } => "a value of the type the pack declares".to_owned(),
                    });
                self.problems.push(invalid(
                    &raw.origin,
                    format!("the value of `{}` must be {must}", raw.key),
                    format!("Correct the value of `{}`.", raw.key),
                ));
                return;
            }
        };

        // What the layer may set.
        let may_set = match (rule.scope, layer) {
            (_, Layer::Default | Layer::DeploymentShape | Layer::Preset | Layer::Installation) => {
                true
            }
            (Scope::Installation, Layer::Flag) => self.serve,
            (Scope::Installation, _) => false,
            (Scope::User, Layer::Project) => false,
            _ => true,
        };
        if !may_set {
            self.problems.push(Failure::new(
                a2m_core::FailureClass::Policy,
                codes::SETTING_PROTECTED,
                format!(
                    "{}: `{}` is set at the scope `{}`, and the {} layer may not set it.",
                    raw.origin,
                    raw.key,
                    rule.scope.as_str(),
                    layer.as_str()
                ),
                match rule.scope {
                    Scope::Installation => "Set it in the installation's settings file.",
                    _ => "Set it in the user's settings file.",
                },
            ));
            return;
        }
        let current = self.entries.get(&raw.key).map(|entry| entry.value.clone());
        if layer > Layer::Installation
            && let Some(lock) = self.locked(&raw.key)
            && current.as_ref() != Some(&value)
        {
            self.problems.push(Failure::new(
                a2m_core::FailureClass::Policy,
                codes::SETTING_LOCKED,
                format!(
                    "{}: `{}` is locked by `{lock}` in the installation's settings file.",
                    raw.origin, raw.key
                ),
                "Remove the key from this layer, or ask whoever installed the engine to change it.",
            ));
            return;
        }
        let held = (layer == Layer::Installation)
            .then(|| self.installation_held.clone())
            .flatten();
        if let Some(why) = &held
            && let Some(safer) = rule.protected
            && let Some(current) = &current
            && safer.loosens(current, &value)
        {
            self.problems.push(Failure::new(
                a2m_core::FailureClass::Policy,
                codes::SETTING_PROTECTED,
                format!(
                    "{}: `{}` is a protected setting, and the installation's settings file may loosen it only when no account but an administrator can change the file, and {why}.",
                    raw.origin, raw.key
                ),
                format!(
                    "Remove the key from this file, or set it to a value that tightens it. To loosen it, an administrator writes it in the installation's settings file in {}, which no other account can change; under `{}` the installation's folder is the person's own, and no loosening is taken from it.",
                    system_installation_folder(),
                    crate::paths::HOME_VARIABLE
                ),
            ));
            return;
        }
        if layer.only_tightens()
            && let Some(safer) = rule.protected
            && let Some(current) = &current
            && safer.loosens(current, &value)
        {
            self.problems.push(Failure::new(
                a2m_core::FailureClass::Policy,
                codes::SETTING_PROTECTED,
                format!(
                    "{}: `{}` is a protected setting, and the {} layer may tighten it but not loosen it.",
                    raw.origin,
                    raw.key,
                    layer.as_str()
                ),
                format!(
                    "Remove the key from this layer, or set it to a value that tightens it. A loosening is taken only from the installation's settings file in {}, which an administrator writes and no other account can change; `anything2mcp config set`, which would let a person loosen it with proof of presence, is not built yet.",
                    system_installation_folder()
                ),
            ));
            return;
        }

        let locked = self.locked(&raw.key).is_some();
        let mut overridden = Vec::new();
        if let Some(lower) = self.entries.remove(&raw.key) {
            overridden = lower.overridden;
            overridden.push(Overridden {
                value: lower.value,
                layer: lower.layer,
                origin: lower.origin,
            });
        }
        self.entries.insert(
            raw.key,
            Entry {
                value,
                layer,
                origin: raw.origin,
                overridden,
                locked,
                protected: rule.protected.is_some(),
                scope: rule.scope,
                effective: rule.effective,
                doc: rule.doc,
                built: rule.built,
            },
        );
    }

    fn finish(mut self, project_file: Option<PathBuf>) -> (Settings, Vec<Failure>) {
        let locked: Vec<String> = self
            .entries
            .keys()
            .filter(|key| self.locked(key).is_some())
            .cloned()
            .collect();
        for key in locked {
            if let Some(entry) = self.entries.get_mut(&key) {
                entry.locked = true;
            }
        }
        let mut hasher = FieldHasher::new();
        for (key, entry) in &self.entries {
            hasher.field(key, &entry.value.canonical_bytes());
        }
        let settings = Settings {
            entries: self.entries,
            locks: self.locks,
            presets: self.presets,
            refused: self.problems.clone(),
            packs: self.packs,
            digest: hasher.finish(),
            project_file,
        };
        (settings, self.problems)
    }
}

impl Settings {
    /// The manifests of the packs whose groups were loaded.
    #[must_use]
    pub fn packs(&self) -> &[Arc<Manifest>] {
        &self.packs
    }
}

const fn pack_scope(scope: SettingScope) -> Scope {
    match scope {
        SettingScope::Installation => Scope::Installation,
        SettingScope::User => Scope::User,
        SettingScope::Project => Scope::Project,
    }
}

fn written(value: &DeValue<'_>) -> Written {
    match value {
        DeValue::String(text) => Written::Typed(Value::Text(text.to_string())),
        DeValue::Boolean(flag) => Written::Typed(Value::Bool(*flag)),
        DeValue::Integer(number) => {
            let digits: String = number.as_str().chars().filter(|c| *c != '_').collect();
            i64::from_str_radix(&digits, number.radix()).map_or(Written::Unusable, |number| {
                Written::Typed(Value::Int(number))
            })
        }
        DeValue::Array(items) => {
            let mut texts = Vec::new();
            for item in items.iter() {
                match item.get_ref() {
                    DeValue::String(text) => texts.push(Value::Text(text.to_string())),
                    DeValue::Integer(number) => match number.as_str().parse::<i64>() {
                        Ok(number) => texts.push(Value::Int(number)),
                        Err(_) => return Written::Unusable,
                    },
                    _ => return Written::Unusable,
                }
            }
            Written::Typed(Value::List(texts))
        }
        DeValue::Float(_) => Written::Unusable,
        DeValue::Datetime(_) => Written::Unusable,
        DeValue::Table(_) => Written::Unusable,
    }
}

fn line_of(origin: &str) -> usize {
    origin
        .rsplit_once("line ")
        .and_then(|(_, line)| line.parse().ok())
        .unwrap_or(0)
}

fn invalid(origin: &str, what: String, fix: impl Into<String>) -> Failure {
    codes::input(codes::SETTINGS_INVALID, format!("{origin}: {what}."), fix)
}
