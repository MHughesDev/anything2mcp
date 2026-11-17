//! What a pack registers with the compiler, and the set that holds it.
//!
//! A pack brings four kinds of thing: file kinds with their schemas,
//! lowerings from a binding to steps, lint rules, and drafting rules. Beside
//! them it names the surface kinds and controls it serves, as data. The
//! kernel calls these and never knows what any of them is about.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use a2m_core::{
    ControlName, Failure, FailureClass, FileKindName, Finding, FindingKind, IdentityStrategy,
    LintCode, Mode, PackPayload, Step, StepKind, SurfaceKind, SurfaceName, ValueType, Verb,
};

use crate::diag::{Diagnostic, Diagnostics, Level, REGISTRATION_CONFLICT};
use crate::doc::{Doc, Located, Place};
use crate::draft::{DraftContext, DraftDoc};
use crate::kernel::{KERNEL_FILE_KINDS, KERNEL_FOLDERS, KERNEL_OWNER};
use crate::schema::JsonSchema;
use crate::view::{KindContext, LowerContext, ResolvedBinding, ResolvedView};

/// How the compiler is run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CompileMode {
    /// For `lint`, `try`, and `test`: test rules are warnings.
    Draft,
    /// For `publish`: test rules are errors.
    Publish,
}

/// A name a document defines, such as a screen or an endpoint, which other
/// files may refer to.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct DefinedName {
    /// The name, unique within its file kind and surface kind.
    pub name: String,
}

/// A name an expression may read, with its type when the type is known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclaredName {
    /// The name.
    pub name: String,
    /// Its type. An expression that reads a name of no known type cannot go
    /// into a plan.
    pub ty: ValueType,
}

/// One expression inside a pack's own file: its text, its place, the names
/// it may read, and the type its place needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExprSite {
    /// The expression, as written.
    pub text: String,
    /// Where it is written.
    pub at: Place,
    /// The names it may read.
    pub names: Vec<DeclaredName>,
    /// The type its place needs, when it needs one.
    pub expected: Option<ValueType>,
}

/// What a file kind compiled its documents to, for one surface kind. The
/// kernel wraps it in the plan's pack section with the owner's name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledSection {
    /// The version of the payload's own shape, which the pack sets.
    pub format: u16,
    /// The compiled knowledge, opaque to the kernel.
    pub payload: PackPayload,
}

/// A file kind a pack owns, such as `screens/`, for the surface kinds it
/// serves. One owner per file kind and surface kind; a second registration
/// for the same pair is refused (decision 0062).
pub trait FileKind: Send + Sync {
    /// The file kind's name, such as `screen`.
    fn name(&self) -> FileKindName;
    /// The surface kinds it is registered for. With `name`, the key.
    fn surface_kinds(&self) -> &[SurfaceKind];
    /// The folder its files live in, as one name with no slash, such as
    /// `screens`. Every registration of one file kind names the same folder.
    fn folder(&self) -> &str;
    /// The schema of one document. Published with the kernel's. A document
    /// may state `surface`; the kernel reads that key to find the document's
    /// surface, so a schema that allows it declares it as
    /// [`JsonSchema::surface`].
    fn schema(&self) -> &JsonSchema;
    /// The names a document defines, for stage 3. The kernel refuses a name
    /// defined twice within the file kind and surface kind.
    fn names(&self, doc: &Located<Doc>) -> Vec<Located<DefinedName>>;
    /// The expressions inside a document, for stage 4.
    fn expressions(&self, doc: &Located<Doc>) -> Vec<ExprSite>;
    /// Compiles the validated documents that live on surfaces of one surface
    /// kind into one section of the plan.
    ///
    /// # Errors
    ///
    /// Diagnostics, each with a place and a fix, under the pack's own codes.
    fn compile(
        &self,
        docs: &[Located<Doc>],
        cx: &KindContext<'_>,
    ) -> Result<CompiledSection, Diagnostics>;
}

/// Whether an identity strategy can be carried out by a binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Possible {
    /// It can.
    Yes,
    /// It cannot, and why, in one sentence.
    No(String),
}

/// What a lowering makes of one binding. The step lists go into the plan
/// unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lowered {
    /// The surface the binding acts on.
    pub surface: SurfaceName,
    /// The steps that do the work.
    pub act: Vec<Step>,
    /// The steps that see the system accept it.
    pub confirm: Vec<Step>,
    /// The steps that read the record back, and look it up on their own.
    pub read_back: Vec<Step>,
    /// The steps that find the record by its stamp or a declared search.
    pub lookup: Vec<Step>,
    /// How long a lookup may lag behind a write, in milliseconds.
    pub settle_ms: u32,
    /// Whether the save's own response may stand for the read-back.
    pub prefer_response: bool,
}

/// How one mode lowers one or more verbs into steps.
pub trait Lowering: Send + Sync {
    /// The mode.
    fn mode(&self) -> Mode;
    /// The verbs this lowering serves. With `mode`, the keys.
    fn verbs(&self) -> &[Verb];
    /// The verb of a binding of this mode that states none. At most one
    /// lowering of a mode gives one.
    fn default_verb(&self) -> Option<Verb> {
        None
    }
    /// The file kinds it reads and does not own; declared, never assumed.
    fn reads(&self) -> &[(FileKindName, SurfaceKind)];
    /// Every kind its steps may carry; the mode's driver must know each.
    fn step_kinds(&self) -> &[StepKind];
    /// The fields this mode adds to a binding: an object schema. The kernel
    /// owns `mode`, `verb`, `status`, and `precondition`; everything else in
    /// a binding, `read_back` included, is checked against this.
    fn binding_schema(&self) -> &JsonSchema;
    /// Whether `strategy` is possible for this binding.
    fn identity(&self, binding: &ResolvedBinding<'_>, strategy: IdentityStrategy) -> Possible;
    /// Lowers the binding into steps.
    ///
    /// # Errors
    ///
    /// Diagnostics, each with a place and a fix, under the pack's own codes.
    fn lower(
        &self,
        binding: &ResolvedBinding<'_>,
        cx: &LowerContext<'_>,
    ) -> Result<Lowered, Diagnostics>;
}

/// A lint rule with a stable code. Sees resolved files; cannot change them.
pub trait LintRule: Send + Sync {
    /// The code: the registering pack's name, a dot, and the rule.
    fn code(&self) -> LintCode;
    /// How serious a finding of this rule is in each mode.
    fn level(&self, mode: CompileMode) -> Level;
    /// What the rule requires, in one sentence.
    fn summary(&self) -> &str;
    /// Checks the target. The kernel sets each diagnostic's code and level
    /// from the two methods above.
    fn check(&self, view: &ResolvedView<'_>) -> Vec<Diagnostic>;
}

/// From one kind of finding to one kind of file. Mechanical: the same
/// finding gives the same bytes.
pub trait DraftingRule: Send + Sync {
    /// The kind of finding it reads.
    fn finding_kind(&self) -> FindingKind;
    /// The kind of file it writes.
    fn file_kind(&self) -> FileKindName;
    /// Drafts the documents one finding gives. A value the finding does not
    /// state is left as [`DraftContext::open_question`], never guessed.
    ///
    /// # Errors
    ///
    /// A failure when the finding is not one the rule can read.
    fn draft(&self, finding: &Finding, cx: &DraftContext<'_>) -> Result<Vec<DraftDoc>, Failure>;
}

/// A surface kind a pack serves, as data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceKindSpec {
    /// The surface kind, such as `web`.
    pub kind: SurfaceKind,
    /// The key of a surface that holds its address, such as `origin`.
    pub address_key: String,
    /// The fields this kind adds to a surface: an object schema that lists
    /// the address key. The kernel owns `kind`, `same_session_as`, `rate`,
    /// and `stands`. The other fields travel in the surface's payload.
    pub fields: JsonSchema,
}

/// One registered thing, and the pack that registered it.
#[derive(Clone)]
pub(crate) struct Registered<T> {
    pub pack: String,
    pub item: T,
}

/// Everything every enabled pack registered. Built by `a2m-provider`;
/// passed by `a2m-actions`.
#[derive(Clone, Default)]
pub struct CompilerSet {
    pub(crate) surface_kinds: BTreeMap<SurfaceKind, Registered<SurfaceKindSpec>>,
    pub(crate) file_kinds: BTreeMap<(FileKindName, SurfaceKind), Registered<Arc<dyn FileKind>>>,
    pub(crate) folders: BTreeMap<String, FileKindName>,
    pub(crate) lowerings: BTreeMap<(Mode, Verb), Registered<Arc<dyn Lowering>>>,
    pub(crate) default_verbs: BTreeMap<Mode, Verb>,
    pub(crate) lint_rules: BTreeMap<LintCode, Registered<Arc<dyn LintRule>>>,
    pub(crate) drafting_rules: BTreeMap<(FindingKind, FileKindName), Registered<Arc<dyn DraftingRule>>>,
    pub(crate) controls: BTreeMap<ControlName, String>,
}

impl fmt::Debug for CompilerSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CompilerSet")
            .field("surface_kinds", &self.surface_kinds.keys().collect::<Vec<_>>())
            .field("file_kinds", &self.file_kinds.keys().collect::<Vec<_>>())
            .field("lowerings", &self.lowerings.keys().collect::<Vec<_>>())
            .field("lint_rules", &self.lint_rules.keys().collect::<Vec<_>>())
            .field("drafting_rules", &self.drafting_rules.keys().collect::<Vec<_>>())
            .field("controls", &self.controls.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl CompilerSet {
    /// A builder that holds nothing yet.
    #[must_use]
    pub fn builder() -> CompilerSetBuilder {
        CompilerSetBuilder::default()
    }

    /// A set with no pack: only the kernel's own file kinds compile.
    #[must_use]
    pub fn kernel_only() -> Self {
        Self::default()
    }

    /// The surface kinds registered, in order of name.
    pub fn surface_kinds(&self) -> impl Iterator<Item = &SurfaceKind> {
        self.surface_kinds.keys()
    }

    /// The modes that have a lowering, in order of name.
    #[must_use]
    pub fn modes(&self) -> Vec<&Mode> {
        let mut modes: Vec<&Mode> = self.lowerings.keys().map(|(mode, _)| mode).collect();
        modes.dedup();
        modes
    }

    /// The verbs registered for a mode, in order of name.
    #[must_use]
    pub fn verbs(&self, mode: &Mode) -> Vec<&Verb> {
        self.lowerings
            .keys()
            .filter(|(registered, _)| registered == mode)
            .map(|(_, verb)| verb)
            .collect()
    }

    /// The controls registered, in order of name.
    pub fn controls(&self) -> impl Iterator<Item = &ControlName> {
        self.controls.keys()
    }

    /// The file kinds packs registered, with the surface kind of each.
    pub fn file_kinds(&self) -> impl Iterator<Item = &(FileKindName, SurfaceKind)> {
        self.file_kinds.keys()
    }

    /// The drafting rules registered: the finding kind and file kind of each.
    pub fn drafting_rules(&self) -> impl Iterator<Item = &(FindingKind, FileKindName)> {
        self.drafting_rules.keys()
    }

    /// Every lint code the compiler can report with this set: the kernel's
    /// and each registered rule's, with what the rule requires.
    #[must_use]
    pub fn lint_codes(&self) -> Vec<(String, String)> {
        let mut codes: Vec<(String, String)> = crate::codes::KERNEL_CODES
            .iter()
            .map(|(code, summary)| ((*code).to_owned(), (*summary).to_owned()))
            .collect();
        codes.extend(self.lint_rules.iter().map(|(code, rule)| {
            (code.as_str().to_owned(), rule.item.summary().to_owned())
        }));
        codes
    }

    /// Every schema, the kernel's and the packs', as JSON Schema, by the
    /// name it is published under.
    #[must_use]
    pub fn schemas(&self) -> BTreeMap<String, serde_json::Value> {
        let mut schemas = crate::kernel::schemas();
        for ((kind, surface_kind), registered) in &self.file_kinds {
            schemas.insert(
                format!("{}.{kind}.{surface_kind}", registered.pack),
                registered.item.schema().to_json(),
            );
        }
        for ((mode, verb), registered) in &self.lowerings {
            schemas.insert(
                format!("{}.binding.{mode}.{verb}", registered.pack),
                registered.item.binding_schema().to_json(),
            );
        }
        for (kind, registered) in &self.surface_kinds {
            schemas.insert(
                format!("{}.surface.{kind}", registered.pack),
                registered.item.fields.to_json(),
            );
        }
        schemas
    }

    /// The owner of a file kind for a surface kind, when one registered it.
    pub(crate) fn file_kind(
        &self,
        kind: &FileKindName,
        surface_kind: &SurfaceKind,
    ) -> Option<&Registered<Arc<dyn FileKind>>> {
        self.file_kinds.get(&(kind.clone(), surface_kind.clone()))
    }

    /// The surface kinds a file kind is registered for.
    pub(crate) fn surface_kinds_of(&self, kind: &FileKindName) -> Vec<&SurfaceKind> {
        self.file_kinds
            .keys()
            .filter(|(registered, _)| registered == kind)
            .map(|(_, surface_kind)| surface_kind)
            .collect()
    }
}

/// Gathers what packs register. Each method names the pack, so the plan and
/// every message can say who owns what.
#[derive(Default)]
pub struct CompilerSetBuilder {
    set: CompilerSet,
}

fn conflict(message: String) -> Failure {
    Failure::new(
        FailureClass::Input,
        REGISTRATION_CONFLICT,
        message,
        "Disable one of the two providers, or correct the provider that registers a name that is not its own.",
    )
}

fn check_pack(pack: &str) -> Result<(), Failure> {
    let named = a2m_core::FileKindName::new(pack).is_ok();
    if named && pack != KERNEL_OWNER {
        Ok(())
    } else {
        Err(conflict(format!(
            "A provider registers under its own name, a lowercase name that is not `{KERNEL_OWNER}`; `{}` is not one.",
            crate::schema::shown(pack)
        )))
    }
}

impl CompilerSetBuilder {
    /// Registers a surface kind.
    ///
    /// # Errors
    ///
    /// `registration_conflict` when the kind is already registered, or its
    /// fields are not an object schema that lists the address key and none
    /// of the kernel's keys.
    pub fn surface_kind(&mut self, pack: &str, spec: SurfaceKindSpec) -> Result<&mut Self, Failure> {
        check_pack(pack)?;
        if let Some(first) = self.set.surface_kinds.get(&spec.kind) {
            return Err(conflict(format!(
                "The surface kind `{}` is registered by `{}` and again by `{pack}`.",
                spec.kind, first.pack
            )));
        }
        let keys = spec.fields.keys();
        let kernel_key = keys
            .iter()
            .find(|key| crate::kernel::SURFACE_KEYS.contains(key));
        if !keys.contains(&spec.address_key.as_str()) || kernel_key.is_some() {
            return Err(conflict(format!(
                "The fields of the surface kind `{}` must list its address key `{}` and none of the kernel's keys.",
                spec.kind,
                crate::schema::shown(&spec.address_key)
            )));
        }
        self.set.surface_kinds.insert(
            spec.kind.clone(),
            Registered {
                pack: pack.to_owned(),
                item: spec,
            },
        );
        Ok(self)
    }

    /// Registers a control the pack can operate.
    ///
    /// # Errors
    ///
    /// `registration_conflict` when another pack registered it.
    pub fn control(&mut self, pack: &str, control: ControlName) -> Result<&mut Self, Failure> {
        check_pack(pack)?;
        match self.set.controls.get(&control) {
            Some(first) if first != pack => Err(conflict(format!(
                "The control `{control}` is registered by `{first}` and again by `{pack}`."
            ))),
            _ => {
                self.set.controls.insert(control, pack.to_owned());
                Ok(self)
            }
        }
    }

    /// Registers a file kind for each of its surface kinds.
    ///
    /// # Errors
    ///
    /// `registration_conflict` when a pair is already registered, the kind
    /// or its folder is the kernel's, or the folder differs from the one the
    /// kind was first registered with.
    pub fn file_kind(&mut self, pack: &str, kind: Arc<dyn FileKind>) -> Result<&mut Self, Failure> {
        check_pack(pack)?;
        let name = kind.name();
        let folder = kind.folder().to_owned();
        if KERNEL_FILE_KINDS.contains(&name.as_str())
            || KERNEL_FOLDERS.contains(&folder.as_str())
            || a2m_core::FileKindName::new(&folder).is_err()
        {
            return Err(conflict(format!(
                "The file kind `{name}` in the folder `{}` is the kernel's, or the folder is not one lowercase name.",
                crate::schema::shown(&folder)
            )));
        }
        if kind.surface_kinds().is_empty() {
            return Err(conflict(format!(
                "The file kind `{name}` is registered for no surface kind."
            )));
        }
        let taken = self.set.folders.get(&folder);
        let other_folder = self
            .set
            .folders
            .iter()
            .find(|(_, registered)| **registered == name)
            .map(|(folder, _)| folder);
        if taken.is_some_and(|taken| *taken != name)
            || other_folder.is_some_and(|other| *other != folder)
        {
            return Err(conflict(format!(
                "The folder `{folder}` and the file kind `{name}` are registered with another kind or folder."
            )));
        }
        for surface_kind in kind.surface_kinds() {
            if let Some(first) = self.set.file_kind(&name, surface_kind) {
                return Err(conflict(format!(
                    "The file kind `{name}` for `{surface_kind}` surfaces is registered by `{}` and again by `{pack}`.",
                    first.pack
                )));
            }
        }
        for surface_kind in kind.surface_kinds() {
            self.set.file_kinds.insert(
                (name.clone(), surface_kind.clone()),
                Registered {
                    pack: pack.to_owned(),
                    item: Arc::clone(&kind),
                },
            );
        }
        self.set.folders.insert(folder, name);
        Ok(self)
    }

    /// Registers a lowering for its mode and each of its verbs.
    ///
    /// # Errors
    ///
    /// `registration_conflict` when a mode and verb is already registered,
    /// the lowering serves no verb, or the mode already has a default verb.
    pub fn lowering(&mut self, pack: &str, lowering: Arc<dyn Lowering>) -> Result<&mut Self, Failure> {
        check_pack(pack)?;
        let mode = lowering.mode();
        if lowering.verbs().is_empty() {
            return Err(conflict(format!(
                "The lowering for the mode `{mode}` serves no verb."
            )));
        }
        for verb in lowering.verbs() {
            if let Some(first) = self.set.lowerings.get(&(mode.clone(), verb.clone())) {
                return Err(conflict(format!(
                    "The verb `{verb}` of the mode `{mode}` is registered by `{}` and again by `{pack}`.",
                    first.pack
                )));
            }
        }
        if let Some(verb) = lowering.default_verb() {
            if self.set.default_verbs.contains_key(&mode) || !lowering.verbs().contains(&verb) {
                return Err(conflict(format!(
                    "The mode `{mode}` has one default verb, and it is one its lowering serves."
                )));
            }
            self.set.default_verbs.insert(mode.clone(), verb);
        }
        for verb in lowering.verbs() {
            self.set.lowerings.insert(
                (mode.clone(), verb.clone()),
                Registered {
                    pack: pack.to_owned(),
                    item: Arc::clone(&lowering),
                },
            );
        }
        Ok(self)
    }

    /// Registers a lint rule.
    ///
    /// # Errors
    ///
    /// `registration_conflict` when the code is already registered or its
    /// owner is not the pack.
    pub fn lint_rule(&mut self, pack: &str, rule: Arc<dyn LintRule>) -> Result<&mut Self, Failure> {
        check_pack(pack)?;
        let code = rule.code();
        if code.owner() != pack {
            return Err(conflict(format!(
                "The lint code `{code}` does not start with the name of `{pack}`, which registers it."
            )));
        }
        if self.set.lint_rules.contains_key(&code) {
            return Err(conflict(format!("The lint code `{code}` is registered twice.")));
        }
        self.set.lint_rules.insert(
            code,
            Registered {
                pack: pack.to_owned(),
                item: rule,
            },
        );
        Ok(self)
    }

    /// Registers a drafting rule.
    ///
    /// # Errors
    ///
    /// `registration_conflict` when a rule from the same kind of finding to
    /// the same kind of file is already registered.
    pub fn drafting_rule(
        &mut self,
        pack: &str,
        rule: Arc<dyn DraftingRule>,
    ) -> Result<&mut Self, Failure> {
        check_pack(pack)?;
        let key = (rule.finding_kind(), rule.file_kind());
        if let Some(first) = self.set.drafting_rules.get(&key) {
            return Err(conflict(format!(
                "A drafting rule from `{}` findings to `{}` files is registered by `{}` and again by `{pack}`.",
                key.0.as_str(),
                key.1,
                first.pack
            )));
        }
        self.set.drafting_rules.insert(
            key,
            Registered {
                pack: pack.to_owned(),
                item: rule,
            },
        );
        Ok(self)
    }

    /// The finished set. A lowering's declared read of a kind nobody owns is
    /// not refused here: the pack that owns it may be disabled, and only a
    /// binding that needs the kind fails to compile.
    #[must_use]
    pub fn build(self) -> CompilerSet {
        self.set
    }
}
