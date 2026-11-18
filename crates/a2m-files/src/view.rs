//! The read-only views a pack's registered code sees while the compiler
//! runs. A view shows the pack what it may know and nothing else: its own
//! binding or documents, the target's surfaces and field types, its own
//! compiled sections, and the validated entries of kinds it declared it
//! reads.

use std::cell::RefCell;
use std::collections::BTreeMap;

use a2m_core::{
    BindingStatus, EffectClass, ExprIndex, FieldTypeIndex, FieldTypePlan, FileKindName, Hazard,
    IdentityStrategy, InputIndex, InputPlan, Mode, OperationName, PackSection, PlanExpr,
    SectionIndex, SurfaceIndex, SurfaceKind, SurfaceName, SurfacePlan, TargetName, ValueType, Verb,
};
use a2m_expr::{LibraryVersion, TypeEnv};

use crate::codes::{self, kernel};
use crate::diag::{Diagnostic, Stage};
use crate::doc::{Doc, Located, Place};
use crate::load::Sources;
use crate::register::{CompileMode, CompilerSet, ExprSite};

/// Every expression of the plan being built, each once.
#[derive(Debug, Default)]
pub(crate) struct ExprTable {
    pub exprs: Vec<PlanExpr>,
    by_source: BTreeMap<String, Vec<u32>>,
}

impl ExprTable {
    pub fn intern(&mut self, expr: PlanExpr) -> ExprIndex {
        let same = self.by_source.get(&expr.source).and_then(|positions| {
            positions
                .iter()
                .copied()
                .find(|position| self.exprs.get(*position as usize) == Some(&expr))
        });
        if let Some(position) = same {
            return ExprIndex::new(position);
        }
        let position = u32::try_from(self.exprs.len()).unwrap_or(u32::MAX);
        self.by_source
            .entry(expr.source.clone())
            .or_default()
            .push(position);
        self.exprs.push(expr);
        ExprIndex::new(position)
    }
}

/// One surface, with the document it came from.
#[derive(Debug, Clone)]
pub(crate) struct SurfaceInfo {
    pub plan: SurfacePlan,
    pub doc: Located<Doc>,
}

/// One input of an operation or pipeline, with its place.
#[derive(Debug, Clone)]
pub(crate) struct InputInfo {
    pub plan: InputPlan,
    pub at: Place,
}

/// One operation, resolved, before its bindings are lowered.
#[derive(Debug, Clone)]
pub(crate) struct OpInfo {
    pub name: OperationName,
    pub doc: Located<Doc>,
    pub effect: EffectClass,
    pub hazard: Hazard,
    pub inputs: Vec<InputInfo>,
    pub outputs: Vec<a2m_core::OutputPlan>,
    pub strategy: IdentityStrategy,
    pub key: Vec<String>,
    /// Where `place: fixed` is written, for an operation whose record is
    /// one fixed place (decision 0252).
    pub fixed: Option<Place>,
    pub commit_points: Option<u32>,
}

/// What the compiler knows about the target once names are resolved.
#[derive(Debug, Default)]
pub(crate) struct Model {
    pub target: Option<TargetName>,
    pub modules: Vec<String>,
    pub surfaces: Vec<SurfaceInfo>,
    pub field_types: Vec<FieldTypePlan>,
    pub incomplete_types: BTreeMap<String, String>,
    pub entries: BTreeMap<(FileKindName, SurfaceKind), Vec<Located<Doc>>>,
    pub owners: BTreeMap<(FileKindName, SurfaceKind), String>,
    pub sections: Vec<PackSection>,
    /// The names pack documents define, by file kind and surface kind.
    pub defined: BTreeMap<(FileKindName, SurfaceKind), Vec<String>>,
    /// `defaults.timeouts` of `target.yaml`, in milliseconds by name.
    pub default_timeouts: BTreeMap<String, u32>,
    pub exprs: RefCell<ExprTable>,
}

fn index(position: usize) -> u32 {
    u32::try_from(position).unwrap_or(u32::MAX)
}

impl Model {
    pub fn surface(&self, name: &str) -> Option<SurfaceView<'_>> {
        self.surfaces
            .iter()
            .position(|surface| surface.plan.name.as_str() == name)
            .map(|position| self.surface_at(position))
    }

    fn surface_at(&self, position: usize) -> SurfaceView<'_> {
        let info = &self.surfaces[position];
        SurfaceView {
            index: SurfaceIndex::new(index(position)),
            name: &info.plan.name,
            kind: &info.plan.kind,
            address: &info.plan.address,
            doc: &info.doc,
        }
    }

    pub fn field_type(&self, name: &str) -> Option<FieldTypeView<'_>> {
        self.field_types
            .iter()
            .position(|field_type| field_type.name == name)
            .map(|position| FieldTypeView {
                index: FieldTypeIndex::new(index(position)),
                plan: &self.field_types[position],
            })
    }

    /// The surface a pack's document lives on: the one it names, or the
    /// first declared surface of a kind its file kind is registered for.
    pub fn surface_of(
        &self,
        doc: &Located<Doc>,
        kind: &FileKindName,
        set: &CompilerSet,
    ) -> Option<SurfaceView<'_>> {
        if let Some(named) = doc.get("surface") {
            return named.as_text().and_then(|name| self.surface(name));
        }
        let kinds = set.surface_kinds_of(kind);
        (0..self.surfaces.len())
            .map(|position| self.surface_at(position))
            .find(|surface| kinds.contains(&surface.kind))
    }
}

/// The type of `env`: the environment's name and kind.
pub(crate) fn env_type() -> ValueType {
    ValueType::Record(vec![
        a2m_core::FieldOf::required("name", ValueType::Text),
        a2m_core::FieldOf::required("kind", ValueType::Text),
    ])
}

/// The type of `current` in a binding's steps: what the read of the record
/// gave, by the operation's inputs, each of which may be missing.
pub(crate) fn current_type(inputs: &[InputInfo]) -> ValueType {
    ValueType::Record(
        inputs
            .iter()
            .map(|input| {
                a2m_core::FieldOf::optional(input.plan.name.clone(), input.plan.ty.clone())
            })
            .collect(),
    )
}

/// The type of `record` in a binding's steps: the record's key in the
/// system, its parts as text.
pub(crate) fn record_type() -> ValueType {
    ValueType::Record(vec![a2m_core::FieldOf::optional(
        "key",
        ValueType::List(Box::new(ValueType::Text)),
    )])
}

/// The type of `input` for a list of inputs.
pub(crate) fn input_type(inputs: &[InputInfo]) -> ValueType {
    ValueType::Record(
        inputs
            .iter()
            .map(|input| {
                if input.plan.required || input.plan.default.is_some() {
                    a2m_core::FieldOf::required(input.plan.name.clone(), input.plan.ty.clone())
                } else {
                    a2m_core::FieldOf::optional(input.plan.name.clone(), input.plan.ty.clone())
                }
            })
            .collect(),
    )
}

/// An environment that declares `names`.
pub(crate) fn type_env(names: &[(&str, ValueType)]) -> TypeEnv {
    let mut env = TypeEnv::new(LibraryVersion::CURRENT);
    for (name, ty) in names {
        // A name that cannot be declared is simply not there; an expression
        // that reads it is then refused where it is written.
        let _ = env.declare(name, ty.clone());
    }
    env
}

/// An environment whose names have no type until the expression runs. An
/// expression that reads one must still end in a known type to go into a
/// plan.
pub(crate) fn unknown_env(names: &[&str]) -> TypeEnv {
    let mut env = TypeEnv::new(LibraryVersion::CURRENT);
    for name in names {
        let _ = env.declare_unknown(name);
    }
    env
}

/// Checks one expression and gives its plan form, or a diagnostic at its
/// place in the file.
pub(crate) fn check_expr(
    text: &str,
    at: &Place,
    env: &TypeEnv,
    expected: Option<&ValueType>,
) -> Result<PlanExpr, Diagnostic> {
    let to_diagnostic = |error: a2m_expr::ExprError| {
        let code = if error.code == a2m_expr::EXPRESSION_TYPE_ERROR {
            codes::EXPRESSION_TYPE_ERROR
        } else {
            codes::EXPRESSION_INVALID
        };
        let shown = crate::schema::shown(text);
        Diagnostic::error(
            kernel(code),
            Stage::TypeCheck,
            at,
            format!(
                "The expression `{shown}` does not check: {} (characters {} to {} of the expression).",
                error.message, error.at.start, error.at.end
            ),
            error.fix.unwrap_or_else(|| {
                "Correct the expression so that it reads only declared names and its types fit."
                    .to_owned()
            }),
        )
    };
    a2m_expr::compile(text, env, expected)
        .and_then(|compiled| compiled.to_plan_expr())
        .map_err(to_diagnostic)
}

/// The text of an expression written as a value: text as it stands, a whole
/// number or a truth value as itself.
#[must_use]
pub fn expr_text(doc: &Located<Doc>) -> Option<String> {
    match &doc.value {
        Doc::Text(text) => Some(text.clone()),
        Doc::Int(number) => Some(number.to_string()),
        Doc::Bool(flag) => Some(flag.to_string()),
        _ => None,
    }
}

/// One surface of the target.
#[derive(Debug, Clone, Copy)]
pub struct SurfaceView<'a> {
    /// Its position in the plan.
    pub index: SurfaceIndex,
    /// Its name.
    pub name: &'a SurfaceName,
    /// Its kind.
    pub kind: &'a SurfaceKind,
    /// Its address.
    pub address: &'a str,
    /// Its entry in `target.yaml`, with the fields its pack added.
    pub doc: &'a Located<Doc>,
}

/// One field type.
#[derive(Debug, Clone, Copy)]
pub struct FieldTypeView<'a> {
    /// Its position in the plan.
    pub index: FieldTypeIndex,
    /// What the plan holds about it: base, value type, units, rounding.
    pub plan: &'a FieldTypePlan,
}

/// One input of an operation.
#[derive(Debug, Clone, Copy)]
pub struct InputView<'a> {
    /// Its position among the operation's inputs.
    pub index: InputIndex,
    /// What the plan holds about it: name, type, field type.
    pub plan: &'a InputPlan,
    /// Where it is declared.
    pub at: &'a Place,
}

/// The target, as every pack may see it.
#[derive(Debug, Clone, Copy)]
pub struct TargetView<'a> {
    pub(crate) model: &'a Model,
}

impl<'a> TargetView<'a> {
    /// The target's name, when `target.yaml` gave a well-formed one.
    #[must_use]
    pub fn name(&self) -> Option<&'a TargetName> {
        self.model.target.as_ref()
    }

    /// The surfaces, in declared order.
    #[must_use]
    pub fn surfaces(&self) -> Vec<SurfaceView<'a>> {
        (0..self.model.surfaces.len())
            .map(|position| self.model.surface_at(position))
            .collect()
    }

    /// The surface with this name.
    #[must_use]
    pub fn surface(&self, name: &str) -> Option<SurfaceView<'a>> {
        self.model.surface(name)
    }

    /// The field type with this name.
    #[must_use]
    pub fn field_type(&self, name: &str) -> Option<FieldTypeView<'a>> {
        self.model.field_type(name)
    }

    /// A timeout the target states under `defaults.timeouts`, in
    /// milliseconds. A lowering gives it to a step that states none of its
    /// own, so the plan holds every step's timeout and the engine applies no
    /// default.
    #[must_use]
    pub fn default_timeout_ms(&self, name: &str) -> Option<u32> {
        self.model.default_timeouts.get(name).copied()
    }
}

/// One operation, as its bindings' lowerings and the lint rules see it.
#[derive(Debug, Clone, Copy)]
pub struct OperationView<'a> {
    pub(crate) info: &'a OpInfo,
}

impl<'a> OperationView<'a> {
    /// Its name.
    #[must_use]
    pub fn name(&self) -> &'a OperationName {
        &self.info.name
    }

    /// Its declared effect class.
    #[must_use]
    pub fn effect(&self) -> EffectClass {
        self.info.effect
    }

    /// Its declared hazard.
    #[must_use]
    pub fn hazard(&self) -> Hazard {
        self.info.hazard
    }

    /// Its identity strategy.
    #[must_use]
    pub fn strategy(&self) -> IdentityStrategy {
        self.info.strategy
    }

    /// The inputs that make its record key, in declared order.
    #[must_use]
    pub fn key(&self) -> &'a [String] {
        &self.info.key
    }

    /// Its inputs, in declared order.
    #[must_use]
    pub fn inputs(&self) -> Vec<InputView<'a>> {
        self.info
            .inputs
            .iter()
            .enumerate()
            .map(|(position, input)| InputView {
                index: InputIndex::new(index(position)),
                plan: &input.plan,
                at: &input.at,
            })
            .collect()
    }

    /// The input with this name.
    #[must_use]
    pub fn input(&self, name: &str) -> Option<InputView<'a>> {
        self.inputs()
            .into_iter()
            .find(|input| input.plan.name == name)
    }

    /// The fields of its result, in declared order.
    #[must_use]
    pub fn outputs(&self) -> &'a [a2m_core::OutputPlan] {
        &self.info.outputs
    }

    /// Its document.
    #[must_use]
    pub fn doc(&self) -> &'a Located<Doc> {
        &self.info.doc
    }
}

/// One binding, with the names in it resolved.
#[derive(Debug, Clone, Copy)]
pub struct ResolvedBinding<'a> {
    /// The operation it belongs to.
    pub operation: OperationView<'a>,
    /// Its position among the operation's bindings.
    pub index: usize,
    /// Its mode.
    pub mode: &'a Mode,
    /// Its verb, stated or the mode's default.
    pub verb: &'a Verb,
    /// How far it has come.
    pub status: BindingStatus,
    /// The binding as written, with the fields its mode added.
    pub doc: &'a Located<Doc>,
}

impl<'a> ResolvedBinding<'a> {
    /// The value under `key` in the binding.
    #[must_use]
    pub fn field(&self, key: &str) -> Option<&'a Located<Doc>> {
        self.doc.get(key)
    }
}

/// What a file kind sees while it compiles its documents.
pub struct KindContext<'a> {
    pub(crate) model: &'a Model,
    pub(crate) set: &'a CompilerSet,
    pub(crate) kind: &'a FileKindName,
    pub(crate) surface_kind: &'a SurfaceKind,
}

impl<'a> KindContext<'a> {
    /// The target.
    #[must_use]
    pub fn target(&self) -> TargetView<'a> {
        TargetView { model: self.model }
    }

    /// The surface kind whose documents are being compiled.
    #[must_use]
    pub fn surface_kind(&self) -> &'a SurfaceKind {
        self.surface_kind
    }

    /// The surface a document lives on: the one it names under `surface`,
    /// or the first declared surface of a kind this file kind serves.
    #[must_use]
    pub fn surface_of(&self, doc: &Located<Doc>) -> Option<SurfaceView<'a>> {
        self.model.surface_of(doc, self.kind, self.set)
    }

    /// Checks an expression of one of the pack's documents and puts it in
    /// the plan.
    ///
    /// # Errors
    ///
    /// A diagnostic at the expression's place when it does not check.
    pub fn expr(&self, site: &ExprSite) -> Result<ExprIndex, Diagnostic> {
        let plan_expr = check_site(site)?;
        Ok(self.model.exprs.borrow_mut().intern(plan_expr))
    }
}

/// Checks one expression site of a pack's document.
pub(crate) fn check_site(site: &ExprSite) -> Result<PlanExpr, Diagnostic> {
    let names: Vec<(&str, ValueType)> = site
        .names
        .iter()
        .map(|declared| (declared.name.as_str(), declared.ty.clone()))
        .collect();
    check_expr(
        &site.text,
        &site.at,
        &type_env(&names),
        site.expected.as_ref(),
    )
}

/// What a lowering sees while it lowers one binding.
pub struct LowerContext<'a> {
    pub(crate) model: &'a Model,
    pub(crate) pack: &'a str,
    pub(crate) reads: &'a [(FileKindName, SurfaceKind)],
    pub(crate) operation: &'a OpInfo,
    pub(crate) env: TypeEnv,
}

impl<'a> LowerContext<'a> {
    /// The target.
    #[must_use]
    pub fn target(&self) -> TargetView<'a> {
        TargetView { model: self.model }
    }

    /// The operation the binding belongs to.
    #[must_use]
    pub fn operation(&self) -> OperationView<'a> {
        OperationView {
            info: self.operation,
        }
    }

    /// Checks an expression written in the binding and puts it in the plan.
    /// It may read `input`, the operation's inputs; `env`; `current`, what
    /// the engine's last read of the record gave, by the operation's inputs,
    /// null when the record does not exist; and `record`, whose `key` is the
    /// record's key in the system, its parts as text, null when it has none
    /// yet. `expected` is the type the place needs, when it needs one.
    ///
    /// # Errors
    ///
    /// A diagnostic at the expression's place when it does not check.
    pub fn expr(
        &self,
        doc: &Located<Doc>,
        expected: Option<&ValueType>,
    ) -> Result<ExprIndex, Diagnostic> {
        let Some(text) = expr_text(doc) else {
            return Err(Diagnostic::error(
                kernel(codes::WRONG_TYPE),
                Stage::Lower,
                &doc.at,
                format!(
                    "This value is a {}, and an expression is needed.",
                    doc.value.kind()
                ),
                "Write an expression, such as `input.order_id`.",
            ));
        };
        self.expr_at(&text, &doc.at, expected)
    }

    /// As [`LowerContext::expr`], for an expression the lowering wrote
    /// itself, such as `input.order_id` for a placeholder. `at` is the place
    /// in the file that caused it.
    ///
    /// # Errors
    ///
    /// A diagnostic at `at` when the expression does not check.
    pub fn expr_at(
        &self,
        text: &str,
        at: &Place,
        expected: Option<&ValueType>,
    ) -> Result<ExprIndex, Diagnostic> {
        let plan_expr = check_expr(text, at, &self.env, expected)?;
        Ok(self.model.exprs.borrow_mut().intern(plan_expr))
    }

    /// The type of an expression this context returned.
    #[must_use]
    pub fn expr_type(&self, index: ExprIndex) -> Option<ValueType> {
        self.model
            .exprs
            .borrow()
            .exprs
            .get(index.get())
            .map(|expr| expr.result.clone())
    }

    /// One of the pack's own compiled sections, with its position in the
    /// plan for a step's `refers_to`. Another pack's section is never given.
    #[must_use]
    pub fn section(
        &self,
        kind: &FileKindName,
        surface_kind: &SurfaceKind,
    ) -> Option<(SectionIndex, &'a PackSection)> {
        self.model
            .sections
            .iter()
            .position(|section| {
                section.pack == self.pack
                    && &section.kind == kind
                    && &section.surface_kind == surface_kind
            })
            .map(|position| {
                (
                    SectionIndex::new(index(position)),
                    &self.model.sections[position],
                )
            })
    }

    /// The validated documents of a file kind for a surface kind: one the
    /// pack owns, or one the lowering declared it reads. Any other is not
    /// given.
    #[must_use]
    pub fn entries(
        &self,
        kind: &FileKindName,
        surface_kind: &SurfaceKind,
    ) -> Option<&'a [Located<Doc>]> {
        let key = (kind.clone(), surface_kind.clone());
        let owned = self
            .model
            .owners
            .get(&key)
            .is_some_and(|owner| owner == self.pack);
        if owned || self.reads.contains(&key) {
            Some(self.model.entries.get(&key).map_or(&[], Vec::as_slice))
        } else {
            None
        }
    }

    /// The position in the plan of the section that holds a kind this
    /// lowering reads or owns, for a step's `refers_to`.
    #[must_use]
    pub fn section_index(
        &self,
        kind: &FileKindName,
        surface_kind: &SurfaceKind,
    ) -> Option<SectionIndex> {
        let key = (kind.clone(), surface_kind.clone());
        let owned = self
            .model
            .owners
            .get(&key)
            .is_some_and(|owner| owner == self.pack);
        if !(owned || self.reads.contains(&key)) {
            return None;
        }
        self.model
            .sections
            .iter()
            .position(|section| &section.kind == kind && &section.surface_kind == surface_kind)
            .map(|position| SectionIndex::new(index(position)))
    }
}

/// What a lint rule sees: the target with its names resolved.
pub struct ResolvedView<'a> {
    pub(crate) model: &'a Model,
    pub(crate) sources: &'a Sources,
    pub(crate) mode: CompileMode,
    pub(crate) operations: &'a [OpInfo],
}

impl<'a> ResolvedView<'a> {
    /// The target.
    #[must_use]
    pub fn target(&self) -> TargetView<'a> {
        TargetView { model: self.model }
    }

    /// How the compiler is being run.
    #[must_use]
    pub fn mode(&self) -> CompileMode {
        self.mode
    }

    /// Every file, as loaded.
    #[must_use]
    pub fn sources(&self) -> &'a Sources {
        self.sources
    }

    /// The operations that resolved, in order of file and place.
    #[must_use]
    pub fn operations(&self) -> Vec<OperationView<'a>> {
        self.operations
            .iter()
            .map(|info| OperationView { info })
            .collect()
    }

    /// The validated documents of a file kind for a surface kind.
    #[must_use]
    pub fn entries(&self, kind: &FileKindName, surface_kind: &SurfaceKind) -> &'a [Located<Doc>] {
        self.model
            .entries
            .get(&(kind.clone(), surface_kind.clone()))
            .map_or(&[], Vec::as_slice)
    }
}
