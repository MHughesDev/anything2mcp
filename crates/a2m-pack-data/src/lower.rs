//! How a `sql` binding becomes steps.
//!
//! A read is `begin` read-only, `execute`, `fetch`. A write is `begin`,
//! `execute`, `expect_rows`, then `fetch` when the store assigns the key,
//! and `commit`, which alone is the commit point. A value reaches a
//! statement only as a step argument, which the driver binds to a parameter;
//! nothing here can place one in a statement's text.

use a2m_core::{
    EffectClass, FailsAs, FileKindName, IdentityStrategy, LintCode, Mode, PackPayload, PackRef,
    SectionIndex, Step, StepArg, StepKind, SurfaceKind, Verb, WriteEvidence,
};
use a2m_files::{
    Diagnostic, Diagnostics, Doc, JsonSchema, Located, LowerContext, Lowered, Lowering, Place,
    Possible, ResolvedBinding, Stage, StepCommits,
};

use crate::model::{Access, Does, Returns, Section, Statement, StepDetail};
use crate::names::{
    BEGIN, COMMIT, CREATE, DATABASE, DELETE, EXECUTE, EXPECT_ROWS, FETCH, LIST, READ, SEARCH,
    STATEMENT, UPDATE, codes, lint,
};

/// The lowering of every verb of the mode `sql`.
pub(crate) struct SqlLowering {
    mode: Mode,
    verbs: Vec<Verb>,
    steps: Vec<StepKind>,
    schema: JsonSchema,
}

impl SqlLowering {
    pub(crate) fn new(mode: Mode) -> Self {
        Self {
            mode,
            verbs: vec![READ, LIST, SEARCH, CREATE, UPDATE, DELETE],
            steps: vec![BEGIN, EXECUTE, FETCH, EXPECT_ROWS, COMMIT],
            schema: JsonSchema::object()
                .required("statement", JsonSchema::name())
                .optional("params", JsonSchema::map_of(JsonSchema::expression()))
                .optional("read_back", JsonSchema::name()),
        }
    }
}

fn problem(code: &LintCode, stage: Stage, at: &Place, message: String, fix: &str) -> Diagnostic {
    Diagnostic::error(code.clone(), stage, at, message, fix)
}

/// A step that fails as the code of `row`, in the class its table gives it.
fn fails(row: &a2m_core::CodeEntry) -> FailsAs {
    FailsAs::from(row)
}

/// The verb a statement is bound with.
fn verb_fits(verb: &Verb, statement: &Statement) -> bool {
    match (statement.does, statement.returns) {
        (Does::Read, Returns::Many) => *verb == LIST || *verb == SEARCH,
        (Does::Read, _) => *verb == READ,
        (Does::Insert, _) => *verb == CREATE,
        (Does::Update, _) => *verb == UPDATE,
        (Does::Delete, _) => *verb == DELETE,
    }
}

fn verbs_for(statement: &Statement) -> &'static str {
    match (statement.does, statement.returns) {
        (Does::Read, Returns::Many) => "`list` or `search`",
        (Does::Read, _) => "`read`",
        (Does::Insert, _) => "`create`",
        (Does::Update, _) => "`update`",
        (Does::Delete, _) => "`delete`",
    }
}

/// What one lowering works with.
struct Lower<'a, 'c> {
    cx: &'a LowerContext<'c>,
    section: SectionIndex,
    statements: &'a Section,
    params: Option<&'a Located<Doc>>,
}

impl Lower<'_, '_> {
    fn step(
        &self,
        kind: StepKind,
        name: &str,
        statement: &Statement,
        detail: &StepDetail,
        fails_as: FailsAs,
    ) -> Option<Step> {
        Some(Step {
            kind,
            refers_to: Some(PackRef {
                section: self.section,
                entry: name.to_owned(),
            }),
            args: Vec::new(),
            timeout_ms: statement.timeout_ms,
            fails_as,
            commits: false,
            lookup: false,
            idempotent: false,
            when: None,
            payload: PackPayload::encode(detail).ok()?,
        })
    }

    /// One argument for each parameter of the statement: the expression the
    /// binding gives under `params`, or the operation's input of that name.
    /// A key parameter that neither gives, in the read-back of a write whose
    /// key the store assigns (`assigned`), has no argument: the driver takes
    /// it from what the write's own steps recorded before its commit point.
    fn args(
        &self,
        name: &str,
        statement: &Statement,
        at: &Place,
        assigned: bool,
        problems: &mut Diagnostics,
    ) -> Vec<StepArg> {
        let mut args = Vec::new();
        for param in &statement.params {
            let expected = param
                .field_type
                .as_deref()
                .and_then(|field_type| self.cx.target().field_type(field_type))
                .map(|field_type| field_type.plan.ty.clone());
            let given = self.params.and_then(|params| params.get(&param.name));
            let value = match given {
                Some(given) => self.cx.expr(given, expected.as_ref()),
                None if self.cx.operation().input(&param.name).is_some() => {
                    self.cx
                        .expr_at(&format!("input.{}", param.name), at, expected.as_ref())
                }
                None if assigned && param.key => continue,
                None => Err(problem(
                    &lint::PARAM_UNBOUND,
                    Stage::Resolve,
                    at,
                    format!(
                        "The parameter `{}` of the statement `{name}` matches no input and has no entry under `params`.",
                        param.name
                    ),
                    "Add an input of that name, or an entry under `params`.",
                )),
            };
            match value {
                Ok(value) => args.push(StepArg {
                    name: param.name.clone(),
                    value,
                }),
                Err(found) => problems.push(found),
            }
        }
        args
    }

    /// `begin` read-only, `execute`, `fetch`.
    fn read(
        &self,
        name: &str,
        statement: &Statement,
        at: &Place,
        assigned: bool,
        problems: &mut Diagnostics,
    ) -> Option<Vec<Step>> {
        let drift = || fails(&codes::row::SCHEMA_CHANGED);
        let begin = self.step(
            BEGIN,
            name,
            statement,
            &StepDetail {
                access: Some(Access::ReadOnly),
                isolation: Some(statement.isolation),
                rows: None,
            },
            fails(&codes::row::STORE_LOCKED),
        )?;
        let mut execute = self.step(EXECUTE, name, statement, &StepDetail::default(), drift())?;
        execute.args = self.args(name, statement, at, assigned, problems);
        let mut fetch = self.step(FETCH, name, statement, &StepDetail::default(), drift())?;
        fetch.lookup = statement.returns == Returns::OneOrNone;
        Some(vec![begin, execute, fetch])
    }

    /// `begin`, `execute`, `expect_rows`, `fetch` an assigned key, `commit`.
    fn write(
        &self,
        name: &str,
        statement: &Statement,
        at: &Place,
        problems: &mut Diagnostics,
    ) -> Option<Vec<Step>> {
        let begin = self.step(
            BEGIN,
            name,
            statement,
            &StepDetail {
                access: Some(Access::ReadWrite),
                isolation: Some(statement.isolation),
                rows: None,
            },
            fails(&codes::row::STORE_LOCKED),
        )?;
        let mut execute = self.step(
            EXECUTE,
            name,
            statement,
            &StepDetail::default(),
            fails(&codes::row::SCHEMA_CHANGED),
        )?;
        execute.args = self.args(name, statement, at, false, problems);
        // No row changed: the record is gone, or the guard did not hold.
        let none_changed = match (statement.does, statement.guarded) {
            (Does::Insert, _) => fails(&codes::row::UNEXPECTED_STATUS),
            (_, true) => fails(&codes::row::VERSION_CONFLICT),
            (_, false) => fails(&codes::row::CHANGED_SINCE_PREVIEW),
        };
        let expect = self.step(
            EXPECT_ROWS,
            name,
            statement,
            &StepDetail {
                access: None,
                isolation: None,
                rows: Some(1),
            },
            none_changed,
        )?;
        let mut steps = vec![begin, execute, expect];
        if statement.returns == Returns::AssignedKey {
            steps.push(self.step(
                FETCH,
                name,
                statement,
                &StepDetail::default(),
                fails(&codes::row::SCHEMA_CHANGED),
            )?);
        }
        let mut commit = self.step(
            COMMIT,
            name,
            statement,
            &StepDetail::default(),
            fails(&a2m_provider::codes::row::NO_CONFIRMATION),
        )?;
        commit.commits = true;
        steps.push(commit);
        Some(steps)
    }
}

impl Lowering for SqlLowering {
    fn mode(&self) -> Mode {
        self.mode.clone()
    }

    fn verbs(&self) -> &[Verb] {
        &self.verbs
    }

    fn default_verb(&self) -> Option<Verb> {
        Some(READ)
    }

    fn reads(&self) -> &[(FileKindName, SurfaceKind)] {
        &[]
    }

    fn step_kinds(&self) -> &[StepKind] {
        &self.steps
    }

    /// `commit` is the one kind that saves, and it always does: a writing
    /// transaction changes nothing in the store until it is committed, and
    /// no read ends in a `commit`. Every other kind never saves.
    fn commits(&self, kind: &StepKind) -> StepCommits {
        if *kind == COMMIT {
            StepCommits::Always
        } else {
            StepCommits::Never
        }
    }

    /// By the verb, which `lower` holds to what the statement does: a
    /// `create` or an `update` writes, and a `delete` cannot be undone.
    fn least_effect(&self, binding: &ResolvedBinding<'_>) -> EffectClass {
        let verb = binding.verb;
        if *verb == DELETE {
            EffectClass::Irreversible
        } else if *verb == CREATE || *verb == UPDATE {
            EffectClass::Write
        } else {
            EffectClass::Read
        }
    }

    /// A write that names no read-back is the append of a row no one names
    /// (`irreversible`, identity `none`): the store's count of one row, checked
    /// before the commit, is its only evidence. Every other write is read back.
    fn evidence(&self, binding: &ResolvedBinding<'_>, lowered: &Lowered) -> WriteEvidence {
        if lowered.read_back.is_empty()
            && lowered.act.iter().any(|step| step.commits)
            && binding.operation.effect() == EffectClass::Irreversible
            && binding.operation.strategy() == IdentityStrategy::None
        {
            WriteEvidence::Acknowledged
        } else {
            WriteEvidence::ReadBack
        }
    }

    fn binding_schema(&self) -> &JsonSchema {
        &self.schema
    }

    fn identity(&self, _binding: &ResolvedBinding<'_>, strategy: IdentityStrategy) -> Possible {
        match strategy {
            IdentityStrategy::Natural | IdentityStrategy::Composite | IdentityStrategy::None => {
                Possible::Yes
            }
            IdentityStrategy::Stamped | IdentityStrategy::Find => Possible::No(
                "the data pack does not yet find a record by a stamp or by a declared search."
                    .to_owned(),
            ),
        }
    }

    #[allow(clippy::too_many_lines)] // one check after another, each a few lines; splitting hides the order
    fn lower(
        &self,
        binding: &ResolvedBinding<'_>,
        cx: &LowerContext<'_>,
    ) -> Result<Lowered, Diagnostics> {
        let fail = |found: Diagnostic| Err(Diagnostics::from(found));
        let at = &binding.doc.at;
        let Some(named) = binding.field("statement") else {
            // The schema requires the key; the kernel reported its absence.
            return fail(problem(
                &lint::UNKNOWN_STATEMENT,
                Stage::Resolve,
                at,
                "The binding names no statement.".to_owned(),
                "Add `statement:` with a statement that `statements/` declares.",
            ));
        };
        let name = named.as_text().unwrap_or_default();
        // The statement is in the section of its surface's kind: a store in
        // a file, or one on a server.
        let section = crate::names::surface_kinds()
            .iter()
            .filter_map(|kind| cx.section(&STATEMENT, kind))
            .filter_map(|(index, section)| Some((index, section.payload.decode::<Section>().ok()?)))
            .find(|(_, held)| held.statements.contains_key(name))
            .or_else(|| {
                cx.section(&STATEMENT, &DATABASE)
                    .and_then(|(index, section)| {
                        Some((index, section.payload.decode::<Section>().ok()?))
                    })
            });
        let Some((section, statements)) = section else {
            return fail(problem(
                &lint::UNKNOWN_STATEMENT,
                Stage::Resolve,
                &named.at,
                format!("No file under `statements/` declares a statement named `{name}`."),
                "Declare the statement in a file under `statements/`, on a `database` surface.",
            ));
        };
        let unknown = |name: &str, at: &Place| {
            let known: Vec<&str> = statements.statements.keys().map(String::as_str).collect();
            problem(
                &lint::UNKNOWN_STATEMENT,
                Stage::Resolve,
                at,
                format!("No file under `statements/` declares a statement named `{name}`."),
                &format!("Name one of: {}.", known.join(", ")),
            )
        };
        let Some(statement) = statements.statements.get(name) else {
            return fail(unknown(name, &named.at));
        };
        if !verb_fits(binding.verb, statement) {
            return fail(problem(
                &lint::VERB_MISMATCH,
                Stage::Lower,
                binding.field("verb").map_or(at, |verb| &verb.at),
                format!(
                    "The binding's verb is `{}`, and the statement `{name}` is bound with {}.",
                    binding.verb,
                    verbs_for(statement)
                ),
                &format!("Write `verb:` with {}.", verbs_for(statement)),
            ));
        }
        let Some(surface) = cx.target().surface(&statement.surface) else {
            return fail(unknown(name, &named.at));
        };
        let lower = Lower {
            cx,
            section,
            statements: &statements,
            params: binding.field("params"),
        };
        let mut problems = Diagnostics::new();
        if let Some(params) = lower.params {
            for entry in params.entries() {
                let read_back_takes = binding
                    .field("read_back")
                    .and_then(Located::as_text)
                    .and_then(|read_back| statements.statements.get(read_back))
                    .is_some_and(|read_back| {
                        read_back
                            .params
                            .iter()
                            .any(|param| param.name == entry.key.value)
                    });
                let taken = statement
                    .params
                    .iter()
                    .any(|param| param.name == entry.key.value);
                if !taken && !read_back_takes {
                    problems.push(problem(
                        &lint::PARAM_UNBOUND,
                        Stage::Resolve,
                        &entry.key.at,
                        format!(
                            "`params` names `{}`, which is not a parameter of the statement `{name}`.",
                            entry.key.value
                        ),
                        "Remove the entry, or name a parameter the statement declares.",
                    ));
                }
            }
        }
        let read_back_named = binding.field("read_back");
        let lowered = if statement.access == Access::ReadOnly {
            if let Some(read_back) = read_back_named {
                problems.push(problem(
                    &lint::READ_BACK_INVALID,
                    Stage::Lower,
                    &read_back.at,
                    "A binding that reads has no read-back.".to_owned(),
                    "Remove `read_back:`.",
                ));
            }
            lower
                .read(name, statement, at, false, &mut problems)
                .map(|act| (act, Vec::new()))
        } else {
            // A write is allowed only where the target says the store is the
            // system of record. Unknown is not alone.
            let alone = surface
                .doc
                .get("stands")
                .is_some_and(|stands| stands.as_text() == Some("alone"));
            if !alone {
                problems.push(problem(
                    &lint::WRITE_NEEDS_STANDS_ALONE,
                    Stage::Analyze,
                    at,
                    format!(
                        "This binding writes to the surface `{}`, which does not declare `stands: alone`. A write beneath an application is not built yet, and an undeclared surface takes no write.",
                        surface.name
                    ),
                    "Declare `stands: alone` on the surface in `target.yaml` when the store is the system of record; otherwise write through the application.",
                ));
            }
            // A write is read back. The one that cannot be is a row appended
            // with no key that anyone names: the operation is `irreversible`
            // with identity `none`, and the store's own count of exactly one
            // row, checked before the commit, is the evidence.
            let appends = binding.operation.effect() == EffectClass::Irreversible
                && binding.operation.strategy() == IdentityStrategy::None;
            if read_back_named.is_none() && !appends {
                problems.push(problem(
                    &lint::READ_BACK_INVALID,
                    Stage::Lower,
                    at,
                    format!(
                        "The binding writes the record `{}` and names no read-back.",
                        statement.record
                    ),
                    "Add `read_back:` naming the read the pack writes by key for this record (`effect: read`, `form: table`).",
                ));
            }
            let read_back = match read_back_named {
                None => Some(Vec::new()),
                Some(read_back) => {
                    let read_back_name = read_back.as_text().unwrap_or_default();
                    match lower.statements.statements.get(read_back_name) {
                        None => {
                            problems.push(unknown(read_back_name, &read_back.at));
                            Some(Vec::new())
                        }
                        // A write is read back by the read the pack itself
                        // wrote: the same record, by its full key, and
                        // nothing a hand-written text could put in its place.
                        Some(found)
                            if !found.by_key
                                || found.returns != Returns::OneOrNone
                                || found.surface != statement.surface
                                || found.record != statement.record =>
                        {
                            problems.push(problem(
                                &lint::READ_BACK_INVALID,
                                Stage::Lower,
                                &read_back.at,
                                format!("The read-back `{read_back_name}` is not the read of the record `{}` by its key that the pack writes, on the same surface.", statement.record),
                                "Declare the read-back as a statement with `effect: read` and `form: table` for the same record, and name it here. A hand-written read is never a read-back.",
                            ));
                            Some(Vec::new())
                        }
                        Some(found) => lower.read(
                            read_back_name,
                            found,
                            &read_back.at,
                            statement.returns == Returns::AssignedKey,
                            &mut problems,
                        ),
                    }
                }
            };
            lower
                .write(name, statement, at, &mut problems)
                .zip(read_back)
        };
        let Some((act, read_back)) = lowered.filter(|_| problems.is_empty()) else {
            if problems.is_empty() {
                problems.push(problem(
                    &lint::STATEMENT_INVALID,
                    Stage::Lower,
                    at,
                    "The binding's steps cannot be encoded.".to_owned(),
                    "Report this to the pack's author.",
                ));
            }
            return Err(problems);
        };
        Ok(Lowered {
            surface: surface.name.clone(),
            act,
            confirm: Vec::new(),
            read_back,
            lookup: Vec::new(),
            settle_ms: 0,
            prefer_response: false,
        })
    }
}
