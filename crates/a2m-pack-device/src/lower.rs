//! How the `device` mode lowers its verbs into steps.
//!
//! A step is one of four kinds: `open` identifies the device, `write_frame`
//! sends one frame, `read_frame` reads its reply, and `expect` reads the
//! reply in the form the map declares. What each step does is in its
//! payload, which only this pack reads.

use std::sync::Arc;

use a2m_core::{
    EffectClass, FailsAs, FailureClass, FailureCode, FileKindName, Hazard, IdentityStrategy, Mode,
    PackPayload, PackRef, SectionIndex, Step, StepArg, StepKind, SurfaceKind, SurfaceName,
    ValueType, Verb,
};
use a2m_files::{
    Diagnostic, Diagnostics, Doc, JsonSchema, Located, LowerContext, Lowered, Lowering, Place,
    Possible, ResolvedBinding, Stage,
};
use serde::{Deserialize, Serialize};

use crate::codes;
use crate::map::{MapPlan, MapSection, ReadEffect, RegisterPlan, ValueKind};
use crate::names::{Names, lint_code};

/// How long a reply may take when neither the map nor the target says.
const DEFAULT_REPLY_MS: u32 = 1000;
/// The name of the target's default timeout a map's `reply_within` stands
/// in for.
const REPLY_TIMEOUT: &str = "device_reply";

/// What one step does. The step's kind says which of the four it is; this
/// says the rest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum StepPayload {
    /// `open`: ask the device what it is, and compare it with the map the
    /// step refers to. Done once on a link.
    Identify,
    /// `open`, for discovery: ask the device what it is, and report it.
    Report,
    /// `write_frame`: send the request that reads a register.
    AskRead {
        /// The register, by its name in the map.
        register: String,
    },
    /// `write_frame`: send the request that writes one argument to a
    /// register, after the range checks.
    Write {
        /// The register, by its name in the map.
        register: String,
        /// The step argument that holds the value.
        arg: String,
    },
    /// `read_frame`: read the reply to the frame last sent.
    Reply,
    /// `expect`: the reply holds the register's value; give it out under
    /// `output`.
    Value {
        /// The register, by its name in the map.
        register: String,
        /// The name the value is given out under.
        output: String,
    },
    /// `expect`: the reply holds the register's value before a write; keep
    /// it for reconcile.
    Before {
        /// The register, by its name in the map.
        register: String,
    },
    /// `expect`: the reply acknowledges the write.
    Acknowledged,
}

/// The name a value read before a write is kept under, for reconcile.
pub(crate) fn before_name(register: &str) -> String {
    format!("before_{register}")
}

/// The lowering of the `device` mode.
pub(crate) struct DeviceLowering {
    names: Arc<Names>,
    verbs: Vec<Verb>,
    step_kinds: Vec<StepKind>,
    schema: JsonSchema,
}

impl DeviceLowering {
    pub fn new(names: Arc<Names>) -> Self {
        Self {
            verbs: vec![
                names.read.clone(),
                names.update.clone(),
                names.submit.clone(),
            ],
            step_kinds: vec![
                names.open.clone(),
                names.write_frame.clone(),
                names.read_frame.clone(),
                names.expect.clone(),
            ],
            names,
            schema: JsonSchema::object()
                .required("device_map", JsonSchema::name())
                .optional("read", JsonSchema::map_of(JsonSchema::name()))
                .optional("registers", JsonSchema::map_of(JsonSchema::expression()))
                .optional("read_back", JsonSchema::map_of(JsonSchema::name())),
        }
    }
}

/// What a lowering needs of one binding, found once.
struct Scene<'a> {
    names: &'a Names,
    section: SectionIndex,
    map_name: String,
    map: MapPlan,
    reply_ms: u32,
    problems: Diagnostics,
}

impl Scene<'_> {
    fn problem(&mut self, rule: &str, at: &Place, message: String, fix: &str) {
        if let Ok(code) = lint_code(rule) {
            self.problems
                .push(Diagnostic::error(code, Stage::Lower, at, message, fix));
        }
    }

    fn step(
        &mut self,
        kind: &StepKind,
        payload: &StepPayload,
        fails_as: (FailureClass, FailureCode),
        args: Vec<StepArg>,
        at: &Place,
    ) -> Option<Step> {
        // The one step that changes the device: the frame that writes.
        let commits = matches!(payload, StepPayload::Write { .. });
        let payload = match PackPayload::encode(payload) {
            Ok(payload) => payload,
            Err(failure) => {
                self.problem(
                    "step_not_encodable",
                    at,
                    failure.message().to_owned(),
                    "Report this to the pack's author.",
                );
                return None;
            }
        };
        Some(Step {
            kind: kind.clone(),
            refers_to: Some(PackRef {
                section: self.section,
                entry: self.map_name.clone(),
            }),
            args,
            // An identification is a frame and its reply, like any other.
            timeout_ms: self.reply_ms,
            fails_as: FailsAs {
                class: fails_as.0,
                code: fails_as.1,
            },
            commits,
            lookup: false,
            // No device write repeats: a frame with no reply cannot be
            // shown to have done nothing.
            idempotent: false,
            when: None,
            payload,
        })
    }

    fn open(&mut self, at: &Place) -> Option<Step> {
        let kind = self.names.open.clone();
        self.step(
            &kind,
            &StepPayload::Identify,
            (
                FailureClass::UnknownVariant,
                codes::DEVICE_IDENTITY_MISMATCH,
            ),
            Vec::new(),
            at,
        )
    }

    fn reply(&mut self, at: &Place) -> Option<Step> {
        let kind = self.names.read_frame.clone();
        self.step(
            &kind,
            &StepPayload::Reply,
            (FailureClass::Transient, codes::TIMEOUT),
            Vec::new(),
            at,
        )
    }

    fn expect(&mut self, payload: &StepPayload, at: &Place) -> Option<Step> {
        let kind = self.names.expect.clone();
        self.step(
            &kind,
            payload,
            (FailureClass::UnknownVariant, codes::UNEXPECTED_REPLY),
            Vec::new(),
            at,
        )
    }

    /// The three steps that read one register: ask, read the reply, expect
    /// its form.
    fn read(&mut self, register: &str, expect: &StepPayload, at: &Place) -> Option<Vec<Step>> {
        let kind = self.names.write_frame.clone();
        let ask = self.step(
            &kind,
            &StepPayload::AskRead {
                register: register.to_owned(),
            },
            (FailureClass::Transient, codes::CONNECTION_LOST),
            Vec::new(),
            at,
        )?;
        Some(vec![ask, self.reply(at)?, self.expect(expect, at)?])
    }

    /// A register that may be read without changing it, by its name.
    fn readable(&mut self, name: &Located<Doc>) -> Option<String> {
        let register = name.as_text()?;
        match self.map.registers.get(register) {
            None => {
                self.unknown_register(register, &name.at);
                None
            }
            Some(found) if !found.access.readable() => {
                self.problem(
                    "register_not_readable",
                    &name.at,
                    format!("The register `{register}` is a command and has nothing to read."),
                    "Name a register whose `access` is `read` or `read_write`.",
                );
                None
            }
            Some(found) if found.read_effect != ReadEffect::None => {
                self.problem(
                    "read_has_effect",
                    &name.at,
                    format!("Reading the register `{register}` changes it, and an operation that reads such a register is not built."),
                    "Name a register whose `read_effect` is `none`.",
                );
                None
            }
            Some(_) => Some(register.to_owned()),
        }
    }

    fn unknown_register(&mut self, register: &str, at: &Place) {
        let map = self.map_name.clone();
        self.problem(
            "unknown_register",
            at,
            format!("The map `{map}` has no register named `{register}`."),
            "Name a register the map defines; an operation names a register, never an address.",
        );
    }

    /// The steps that read each `output: register` pair of `pairs`, and the
    /// registers read.
    fn read_all(
        &mut self,
        pairs: &Located<Doc>,
        outputs: &[&str],
    ) -> (Vec<Step>, Vec<String>) {
        let mut steps = Vec::new();
        let mut registers = Vec::new();
        for entry in pairs.entries() {
            let output = entry.key.value.as_str();
            if !outputs.contains(&output) {
                self.problem(
                    "unknown_output",
                    &entry.key.at,
                    format!("`{output}` is not an output of the operation."),
                    "Use the name of one of the operation's outputs.",
                );
                continue;
            }
            let Some(register) = self.readable(&entry.value) else {
                continue;
            };
            let expect = StepPayload::Value {
                register: register.clone(),
                output: output.to_owned(),
            };
            if let Some(read) = self.read(&register, &expect, &entry.value.at) {
                steps.extend(read);
                registers.push(register);
            }
        }
        (steps, registers)
    }
}

/// The type an expression must give to be written to a register.
const fn value_type(kind: ValueKind) -> Option<ValueType> {
    match kind {
        ValueKind::Bool => Some(ValueType::Bool),
        ValueKind::Int => Some(ValueType::Int),
        ValueKind::Decimal => Some(ValueType::Decimal),
        ValueKind::Text => Some(ValueType::Text),
        // A choice is text; whether it is one of the codes is the field
        // type's check, and the driver refuses any other.
        ValueKind::Code => None,
    }
}

impl DeviceLowering {
    fn scene<'a>(
        &'a self,
        binding: &ResolvedBinding<'_>,
        cx: &LowerContext<'_>,
    ) -> Result<Scene<'a>, Diagnostics> {
        let refuse = |rule: &str, at: &Place, message: String, fix: &str| {
            let mut problems = Diagnostics::new();
            if let Ok(code) = lint_code(rule) {
                problems.push(Diagnostic::error(code, Stage::Lower, at, message, fix));
            }
            problems
        };
        let kind = &self.names.register_kind;
        let surface_kind = &self.names.surface_kind;
        let named = binding.field("device_map");
        let at = named.map_or(&binding.doc.at, |named| &named.at);
        let map_name = named.and_then(Located::as_text).unwrap_or_default();
        let found = cx.section(kind, surface_kind).and_then(|(index, section)| {
            let maps = section.payload.decode::<MapSection>().ok()?;
            Some((index, maps.get(map_name)?.clone()))
        });
        let Some((section, map)) = found else {
            return Err(refuse(
                "unknown_map",
                at,
                format!("No file under `registers/` defines a map named `{map_name}`."),
                "Name a map that `registers/` defines under `device_map:`.",
            ));
        };
        let reply_ms = map
            .pace
            .reply_within_ms
            .or_else(|| cx.target().default_timeout_ms(REPLY_TIMEOUT))
            .unwrap_or(DEFAULT_REPLY_MS);
        Ok(Scene {
            names: &self.names,
            section,
            map_name: map_name.to_owned(),
            map,
            reply_ms,
            problems: Diagnostics::new(),
        })
    }

    /// The one register a binding writes, with the expression for its value.
    fn written<'d>(
        scene: &mut Scene<'_>,
        binding: &ResolvedBinding<'d>,
    ) -> Option<(String, RegisterPlan, &'d Located<Doc>)> {
        let Some(registers) = binding.field("registers") else {
            scene.problem(
                "registers_missing",
                &binding.doc.at,
                "The binding writes and does not say which register.".to_owned(),
                "Add `registers:` with the register and the expression for its value.",
            );
            return None;
        };
        let [entry] = registers.entries() else {
            scene.problem(
                "one_register_per_write",
                &registers.at,
                "The binding names no register or several; one frame that writes several registers is not built.".to_owned(),
                "Name one register; write several with a pipeline of operations.",
            );
            return None;
        };
        let name = entry.key.value.clone();
        let Some(register) = scene.map.registers.get(&name).cloned() else {
            scene.unknown_register(&name, &entry.key.at);
            return None;
        };
        if !register.access.writable() {
            scene.problem(
                "register_not_writable",
                &entry.key.at,
                format!("The register `{name}` is declared `access: read`."),
                "Name a register the map declares writable.",
            );
            return None;
        }
        Some((name, register, &entry.value))
    }

    /// Checks the operation's declared effect and hazard against the
    /// register's, and gives the hazard the register declares.
    fn hazard(
        scene: &mut Scene<'_>,
        binding: &ResolvedBinding<'_>,
        name: &str,
        register: &RegisterPlan,
    ) -> Option<Hazard> {
        let at = &binding.doc.at;
        let Some(hazard) = register.hazard else {
            scene.problem(
                "writable_without_hazard",
                at,
                format!("The register `{name}` can be written and declares no hazard."),
                "Add `hazard:` to the register in its map.",
            );
            return None;
        };
        if hazard == Hazard::Physical {
            scene.problem(
                "physical_not_built",
                at,
                format!("The register `{name}` has the hazard `physical`; writes that move, heat, or open something are not built: they need `confirm_by` and the safe-state operation."),
                "Leave this operation out until those are built.",
            );
            return None;
        }
        let operation = binding.operation;
        if hazard != Hazard::None
            && (operation.effect() != EffectClass::Irreversible || operation.hazard() < hazard)
        {
            scene.problem(
                "hazard_not_declared",
                at,
                format!("The register `{name}` has the hazard `{hazard}`, and the operation declares the effect `{}` and the hazard `{}`.", operation.effect(), operation.hazard()),
                "Declare `effect: irreversible` and the register's hazard on the operation, so that a person approves every run.",
            );
            return None;
        }
        Some(hazard)
    }

    fn write_step(
        scene: &mut Scene<'_>,
        cx: &LowerContext<'_>,
        name: &str,
        register: &RegisterPlan,
        value: &Located<Doc>,
    ) -> Option<Step> {
        let expected = value_type(register.kind);
        let expr = match cx.expr(value, expected.as_ref()) {
            Ok(expr) => expr,
            Err(problem) => {
                scene.problems.push(problem);
                return None;
            }
        };
        let kind = scene.names.write_frame.clone();
        scene.step(
            &kind,
            &StepPayload::Write {
                register: name.to_owned(),
                arg: name.to_owned(),
            },
            (FailureClass::PossibleCommit, codes::NO_CONFIRMATION),
            vec![StepArg {
                name: name.to_owned(),
                value: expr,
            }],
            &value.at,
        )
    }

    fn absent(scene: &mut Scene<'_>, binding: &ResolvedBinding<'_>, keys: &[&str]) {
        for key in keys {
            if let Some(found) = binding.field(key) {
                let verb = binding.verb.as_str().to_owned();
                scene.problem(
                    "field_not_of_verb",
                    &found.at,
                    format!("A `{verb}` binding does not take `{key}`."),
                    "Remove it, or use the verb it belongs to.",
                );
            }
        }
    }
}

impl Lowering for DeviceLowering {
    fn mode(&self) -> Mode {
        self.names.mode.clone()
    }

    fn verbs(&self) -> &[Verb] {
        &self.verbs
    }

    fn reads(&self) -> &[(FileKindName, SurfaceKind)] {
        &[]
    }

    fn step_kinds(&self) -> &[StepKind] {
        &self.step_kinds
    }

    fn binding_schema(&self) -> &JsonSchema {
        &self.schema
    }

    fn identity(&self, _binding: &ResolvedBinding<'_>, strategy: IdentityStrategy) -> Possible {
        match strategy {
            IdentityStrategy::Natural | IdentityStrategy::None => Possible::Yes,
            IdentityStrategy::Composite | IdentityStrategy::Stamped | IdentityStrategy::Find => {
                Possible::No(
                    "a register is found by its name in the map, and a command has no record."
                        .to_owned(),
                )
            }
        }
    }

    fn lower(
        &self,
        binding: &ResolvedBinding<'_>,
        cx: &LowerContext<'_>,
    ) -> Result<Lowered, Diagnostics> {
        let mut scene = self.scene(binding, cx)?;
        let at = &binding.doc.at;
        let outputs: Vec<&str> = binding
            .operation
            .outputs()
            .iter()
            .map(|output| output.name.as_str())
            .collect();
        let (mut act, mut confirm, mut read_back) = (Vec::new(), Vec::new(), Vec::new());
        act.extend(scene.open(at));
        if *binding.verb == self.names.read {
            Self::absent(&mut scene, binding, &["registers", "read_back"]);
            match binding.field("read") {
                Some(read) if !read.entries().is_empty() => {
                    act.extend(scene.read_all(read, &outputs).0);
                }
                _ => scene.problem(
                    "read_missing",
                    at,
                    "The binding reads and names no register.".to_owned(),
                    "Add `read:` with each output and the register it is read from.",
                ),
            }
        } else if let Some((name, register, value)) = Self::written(&mut scene, binding) {
            Self::absent(&mut scene, binding, &["read"]);
            let hazard = Self::hazard(&mut scene, binding, &name, &register);
            let updating = *binding.verb == self.names.update;
            if updating {
                if !register.access.readable() || register.read_effect != ReadEffect::None {
                    scene.problem(
                        "update_without_read_back",
                        at,
                        format!("The register `{name}` cannot be read back without effect, so a write to it is a `submit`."),
                        "Use `verb: submit`, and declare the operation `irreversible`.",
                    );
                }
                // The value before the write, for reconcile.
                let before = StepPayload::Before {
                    register: name.clone(),
                };
                act.extend(scene.read(&name, &before, at).unwrap_or_default());
                match binding.field("read_back") {
                    Some(pairs) => {
                        read_back.extend(scene.open(&pairs.at));
                        let (steps, registers) = scene.read_all(pairs, &outputs);
                        read_back.extend(steps);
                        if !registers.contains(&name) {
                            scene.problem(
                                "read_back_elsewhere",
                                &pairs.at,
                                format!("The binding writes the register `{name}` and does not read it back."),
                                "Read the written register again under `read_back:`.",
                            );
                        }
                    }
                    None => scene.problem(
                        "update_without_read_back",
                        at,
                        format!("The binding writes the register `{name}` and does not read it back."),
                        "Add `read_back:` with an output and the written register.",
                    ),
                }
            } else {
                Self::absent(&mut scene, binding, &["read_back"]);
                if hazard == Some(Hazard::None) {
                    scene.problem(
                        "command_hazard_none",
                        at,
                        format!("A `submit` has nothing to read back, and the register `{name}` declares the hazard `none`."),
                        "Declare what the command could harm in the map, or use `verb: update` with a read-back.",
                    );
                }
            }
            act.extend(Self::write_step(&mut scene, cx, &name, &register, value));
            confirm.extend(scene.reply(at));
            confirm.extend(scene.expect(&StepPayload::Acknowledged, at));
        }
        let surface = SurfaceName::new(&scene.map.surface).map_err(|_| {
            let mut problems = Diagnostics::new();
            if let Ok(code) = lint_code("map_without_link") {
                problems.push(Diagnostic::error(
                    code,
                    Stage::Lower,
                    at,
                    "The map's surface is not a surface name.",
                    "Name a surface of the kind `device_link` in the map.",
                ));
            }
            problems
        })?;
        if !scene.problems.is_empty() {
            return Err(scene.problems);
        }
        Ok(Lowered {
            surface,
            act,
            confirm,
            read_back,
            lookup: Vec::new(),
            settle_ms: 0,
            prefer_response: false,
        })
    }
}
