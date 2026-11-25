//! How the `device` mode lowers its verbs into steps.
//!
//! A step is one of five kinds: `open` identifies the device, `build_frame`
//! builds and records the frame of a write and sends nothing, `write_frame`
//! sends one frame, `read_frame` reads its reply, and `expect` reads the
//! reply in the form the map declares. What each step does is in its
//! payload, which only this pack reads.

use std::sync::Arc;

use a2m_core::{
    EffectClass, FailsAs, FileKindName, Hazard, Mode, PackPayload, PackRef, RecordMatching,
    SectionIndex, Step, StepArg, StepKind, SurfaceKind, SurfaceName, ValueType, Verb,
    WriteEvidence,
};
use a2m_files::{
    BindingStatement, Diagnostic, Diagnostics, Doc, JsonSchema, Located, LowerContext, Lowered,
    Lowering, Place, Possible, ResolvedBinding, Sends, Stage, Statement, StepChanges, StepCommits,
};
use serde::{Deserialize, Serialize};

use crate::codes;
use crate::map::{CompiledMap, CompiledRegister, MapSection, ReadEffect, ValueKind, scalar_text};
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
    /// `build_frame`: check one argument against its register's range,
    /// build the frame that writes it, and give its bytes out to be
    /// recorded. Nothing is sent. The step before the one that commits.
    Build {
        /// The register, by its name in the map.
        register: String,
        /// The step argument that holds the value.
        arg: String,
        /// How many frames the binding's read-back sends after the write.
        /// The frame is not built when the run's frames would not cover
        /// the write and these.
        reads_after: u32,
    },
    /// `write_frame`: send the frame the step before built for this
    /// register and this value, and no other.
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
    /// `expect`: the reply holds a number, and it is the one a precondition
    /// of the update declares. Nothing is written until each one holds.
    Precondition {
        /// The register, by its name in the map.
        register: String,
        /// The number it must hold, as an exact decimal.
        equals: String,
    },
    /// `build_frame`: check the image against the map's mechanism, the
    /// operation's list of digests, and the version it names, give out
    /// what is to be recorded (the image's digest, its length, how it will
    /// be cut into blocks, what the operation declares), and send nothing.
    /// The step before the one that commits.
    BuildFirmware {
        /// The step argument that holds the image, as hexadecimal text.
        image: String,
        /// The step argument that holds the version the image carries.
        version: String,
        /// The digests the operation accepts; none listed accepts any.
        accepts: Vec<String>,
        /// What the operation declares: the recovery route, what cannot be
        /// undone, and whether the device checks the maker's signature.
        recovery: String,
        cannot_undo: Vec<String>,
        signature_checked: bool,
        /// How many frames the read-back sends after the update.
        reads_after: u32,
    },
    /// `write_frame`: carry out the device's own sequence for the image the
    /// step before built, and send its commit, whose reply the confirm
    /// steps read. The one step that commits.
    Install {
        /// The step argument that holds the image.
        image: String,
        /// The step argument that holds the version.
        version: String,
    },
    /// `open`, after the commit: wait for the device to come back within
    /// the map's restart window on a link of its own, identify it, and
    /// compare its model and serial with the map and the surface. Its
    /// version is given out under `output`.
    AfterUpdate {
        /// The name the version the device reports is given out under.
        output: String,
    },
    /// `expect`: the reply holds the device's checksum of its image, and it
    /// is the checksum the image carries.
    Checksum {
        /// The register, by its name in the map.
        register: String,
        /// The step argument that holds the image.
        image: String,
        /// The name the checksum is given out under, when the operation
        /// declares it as an output.
        output: Option<String>,
    },
}

/// The name a value read before a write is kept under, for reconcile.
pub(crate) fn before_name(register: &str) -> String {
    format!("before_{register}")
}

/// The name the frame of a write is kept under, for reconcile and for the
/// record of what was sent.
pub(crate) fn frame_name(register: &str) -> String {
    format!("frame_{register}")
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
                names.build_frame.clone(),
                names.write_frame.clone(),
                names.read_frame.clone(),
                names.expect.clone(),
            ],
            names,
            schema: JsonSchema::object()
                .required("device_map", JsonSchema::name())
                .optional("read", JsonSchema::map_of(JsonSchema::name()))
                .optional("registers", JsonSchema::map_of(JsonSchema::expression()))
                .optional("read_back", JsonSchema::map_of(JsonSchema::name()))
                // The device's own update, one operation with one commit
                // point. The recovery route is checked where the binding is
                // lowered, so that its absence has a rule of its own.
                .optional(
                    "firmware",
                    JsonSchema::object()
                        .required("image", JsonSchema::expression())
                        .required("version", JsonSchema::expression())
                        .optional("recovery", JsonSchema::text())
                        .required(
                            "cannot_undo",
                            JsonSchema::non_empty_list(JsonSchema::text()),
                        )
                        .required("signature_checked", JsonSchema::bool())
                        .optional("preconditions", JsonSchema::map_of(JsonSchema::decimal()))
                        .optional("accepts", JsonSchema::list(JsonSchema::text())),
                ),
        }
    }
}

/// What a lowering needs of one binding, found once.
struct Scene<'a> {
    names: &'a Names,
    section: SectionIndex,
    map_name: String,
    map: CompiledMap,
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
        fails_as: &a2m_core::CodeEntry,
        args: Vec<StepArg>,
        at: &Place,
    ) -> Option<Step> {
        // The one step that changes the device: the frame that writes.
        let commits = matches!(
            payload,
            StepPayload::Write { .. } | StepPayload::Install { .. }
        );
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
            fails_as: FailsAs::from(fails_as),
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
            &codes::row::DEVICE_IDENTITY_MISMATCH,
            Vec::new(),
            at,
        )
    }

    fn reply(&mut self, at: &Place) -> Option<Step> {
        let kind = self.names.read_frame.clone();
        self.step(
            &kind,
            &StepPayload::Reply,
            &codes::row::TIMEOUT,
            Vec::new(),
            at,
        )
    }

    fn expect(&mut self, payload: &StepPayload, at: &Place) -> Option<Step> {
        let kind = self.names.expect.clone();
        self.step(
            &kind,
            payload,
            &codes::row::UNEXPECTED_REPLY,
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
            &codes::row::CONNECTION_LOST,
            Vec::new(),
            at,
        )?;
        Some(vec![ask, self.reply(at)?, self.expect(expect, at)?])
    }

    /// A register that may be read without changing it, by its name.
    fn readable(&mut self, name: &Located<Doc>) -> Option<String> {
        self.readable_named(name.as_text()?, &name.at)
    }

    /// The same, for a register named by a key.
    fn readable_named(&mut self, register: &str, at: &Place) -> Option<String> {
        let name = (register, at);
        match self.map.registers.get(register) {
            None => {
                self.unknown_register(register, name.1);
                None
            }
            Some(found) if !found.access.readable() => {
                self.problem(
                    "register_not_readable",
                    name.1,
                    format!("The register `{register}` is a command and has nothing to read."),
                    "Name a register whose `access` is `read` or `read_write`.",
                );
                None
            }
            Some(found) if found.read_effect != ReadEffect::None => {
                self.problem(
                    "read_has_effect",
                    name.1,
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
    fn read_all(&mut self, pairs: &Located<Doc>, outputs: &[&str]) -> (Vec<Step>, Vec<String>) {
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
    ) -> Option<(String, CompiledRegister, &'d Located<Doc>)> {
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
        register: &CompiledRegister,
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

    /// The two steps of a write: the frame is built from the value and
    /// given out to be recorded, and then that frame is sent by the one
    /// step that commits. Both are given the same value.
    fn write_steps(
        scene: &mut Scene<'_>,
        cx: &LowerContext<'_>,
        name: &str,
        register: &CompiledRegister,
        value: &Located<Doc>,
        reads_after: u32,
    ) -> Option<[Step; 2]> {
        let expected = value_type(register.kind);
        let expr = match cx.expr(value, expected.as_ref()) {
            Ok(expr) => expr,
            Err(problem) => {
                scene.problems.push(problem);
                return None;
            }
        };
        let arg = || {
            vec![StepArg {
                name: name.to_owned(),
                value: expr,
            }]
        };
        let kind = scene.names.build_frame.clone();
        let build = scene.step(
            &kind,
            &StepPayload::Build {
                register: name.to_owned(),
                arg: name.to_owned(),
                reads_after,
            },
            &codes::row::VALUE_OUT_OF_RANGE,
            arg(),
            &value.at,
        )?;
        let kind = scene.names.write_frame.clone();
        let write = scene.step(
            &kind,
            &StepPayload::Write {
                register: name.to_owned(),
                arg: name.to_owned(),
            },
            &codes::row::NO_CONFIRMATION,
            arg(),
            &value.at,
        )?;
        Some([build, write])
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

impl DeviceLowering {
    /// A binding that writes to a device names a unit: the surface declares
    /// the serial of at least one device. The driver holds the run's own
    /// environment to it.
    fn serial_declared(scene: &mut Scene<'_>, cx: &LowerContext<'_>, at: &Place) {
        let names_a_unit = cx
            .target()
            .surface(&scene.map.surface)
            .and_then(|surface| surface.doc.get("serials"))
            .is_some_and(|serials| !serials.entries().is_empty());
        if !names_a_unit {
            let surface = scene.map.surface.clone();
            scene.problem(
                "write_without_serial",
                at,
                format!("The binding writes to a device, and the surface `{surface}` declares the serial of no device."),
                "Add `serials:` to the surface, with the serial of the unit that answers in each environment.",
            );
        }
    }

    /// The steps of the device's own update. Every one of them is declared
    /// before anything is published: the recovery route, what cannot be
    /// undone, and whether the device checks the maker's signature.
    #[allow(clippy::too_many_lines)] // one declaration after another, in the order the stages run
    fn firmware(
        scene: &mut Scene<'_>,
        binding: &ResolvedBinding<'_>,
        cx: &LowerContext<'_>,
        firmware: &Located<Doc>,
        outputs: &[&str],
        lists: [&mut Vec<Step>; 3],
    ) {
        let [act, confirm, read_back] = lists;
        let at = &firmware.at;
        let Some(update) = scene.map.firmware_update.clone() else {
            let map = scene.map_name.clone();
            scene.problem(
                "firmware_update_not_declared",
                at,
                format!("The map `{map}` declares no `firmware_update`, so an update of its device is not built on it."),
                "Declare the device's own update mechanism under `firmware_update:` in the map, from the device's documentation.",
            );
            return;
        };
        let operation = binding.operation;
        if operation.effect() != EffectClass::Irreversible
            || operation.hazard() < Hazard::DeviceFunction
        {
            scene.problem(
                "firmware_not_irreversible",
                &binding.doc.at,
                format!("A firmware update declares the effect `{}` and the hazard `{}`; it is always `irreversible`, with the hazard `device_function`.", operation.effect(), operation.hazard()),
                "Declare `effect: irreversible` and `hazard: device_function` on the operation, so that a person approves every run.",
            );
        }
        let recovery = firmware
            .text("recovery")
            .map(str::trim)
            .filter(|route| !route.is_empty());
        if recovery.is_none() {
            scene.problem(
                "firmware_without_recovery",
                at,
                "The update declares no recovery route, and a firmware operation without one is not published.".to_owned(),
                "Add `recovery:` with how a person gets the device running again if it does not start, from the device's documentation.",
            );
        }
        let cannot_undo: Vec<String> = firmware
            .get("cannot_undo")
            .map(Located::items)
            .unwrap_or_default()
            .iter()
            .filter_map(|item| item.as_text().map(|text| text.trim().to_owned()))
            .collect();
        if cannot_undo.iter().any(String::is_empty) {
            scene.problem(
                "firmware_without_recovery",
                at,
                "An entry of `cannot_undo` is empty.".to_owned(),
                "Say what an update cannot undo, such as a counter that only rises, or leave the entry out.",
            );
        }
        let accepts: Vec<String> = firmware
            .get("accepts")
            .map(Located::items)
            .unwrap_or_default()
            .iter()
            .filter_map(|item| {
                let digest = item.as_text()?;
                if digest.len() != 64
                    || !digest
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                {
                    scene.problem(
                        "firmware_accepts_malformed",
                        &item.at,
                        "This is not the digest of an image.".to_owned(),
                        "Write the 64 lowercase hexadecimal characters of the image's digest.",
                    );
                    return None;
                }
                Some(digest.to_owned())
            })
            .collect();
        if !outputs.contains(&"version") {
            scene.problem(
                "unknown_output",
                at,
                "The operation has no output `version`; the update reads the version the device reports back into it.".to_owned(),
                "Declare an output `version` on the operation.",
            );
        }
        let text = Some(ValueType::Text);
        let (Some(image), Some(version)) = (firmware.get("image"), firmware.get("version")) else {
            return;
        };
        let (image, version) = match (
            cx.expr(image, text.as_ref()),
            cx.expr(version, text.as_ref()),
        ) {
            (Ok(image), Ok(version)) => (image, version),
            (Err(problem), _) | (_, Err(problem)) => {
                scene.problems.push(problem);
                return;
            }
        };
        let arg = |name: &str, expr| StepArg {
            name: name.to_owned(),
            value: expr,
        };
        let args = || vec![arg("image", image), arg("version", version)];
        Self::serial_declared(scene, cx, &binding.doc.at);

        // The conditions: the device is not already in an update, and each
        // one the operation declares.
        let mut conditions = vec![(update.state.clone(), "0".to_owned(), at.clone())];
        for entry in firmware
            .get("preconditions")
            .map(Located::entries)
            .unwrap_or_default()
        {
            if let Some(register) = scene.readable_named(&entry.key.value, &entry.key.at) {
                let equals = scalar_text(&entry.value);
                conditions.push((register, equals, entry.value.at.clone()));
            }
        }
        for (register, equals, place) in conditions {
            let expect = StepPayload::Precondition {
                register: register.clone(),
                equals,
            };
            act.extend(scene.read(&register, &expect, &place).unwrap_or_default());
        }
        // The checksum before, for reconcile.
        let before = StepPayload::Before {
            register: update.checksum.clone(),
        };
        act.extend(
            scene
                .read(&update.checksum, &before, at)
                .unwrap_or_default(),
        );

        let reads_after = 2;
        let kind = scene.names.build_frame.clone();
        let build = scene.step(
            &kind,
            &StepPayload::BuildFirmware {
                image: "image".to_owned(),
                version: "version".to_owned(),
                accepts,
                recovery: recovery.unwrap_or_default().to_owned(),
                cannot_undo,
                signature_checked: firmware
                    .get("signature_checked")
                    .and_then(Located::as_bool)
                    .unwrap_or(false),
                reads_after,
            },
            &codes::row::IMAGE_NOT_ACCEPTED,
            args(),
            at,
        );
        let kind = scene.names.write_frame.clone();
        let install = scene.step(
            &kind,
            &StepPayload::Install {
                image: "image".to_owned(),
                version: "version".to_owned(),
            },
            &codes::row::NO_CONFIRMATION,
            args(),
            at,
        );
        act.extend(build);
        act.extend(install);
        confirm.extend(scene.reply(at));
        confirm.extend(scene.expect(&StepPayload::Acknowledged, at));

        // After the restart window: identify again, then the device's own
        // checksum of the image it runs.
        let kind = scene.names.open.clone();
        read_back.extend(scene.step(
            &kind,
            &StepPayload::AfterUpdate {
                output: "version".to_owned(),
            },
            &codes::row::DEVICE_NOT_RETURNED,
            Vec::new(),
            at,
        ));
        let checksum = StepPayload::Checksum {
            register: update.checksum.clone(),
            image: "image".to_owned(),
            output: outputs.contains(&"checksum").then(|| "checksum".to_owned()),
        };
        let kind = scene.names.write_frame.clone();
        let ask = scene.step(
            &kind,
            &StepPayload::AskRead {
                register: update.checksum,
            },
            &codes::row::CONNECTION_LOST,
            Vec::new(),
            at,
        );
        read_back.extend(ask);
        read_back.extend(scene.reply(at));
        let kind = scene.names.expect.clone();
        read_back.extend(scene.step(
            &kind,
            &checksum,
            &codes::row::UNEXPECTED_REPLY,
            vec![arg("image", image)],
            at,
        ));
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

    /// `write_frame` is the one kind that can save, and it does both: the
    /// same step sends the frame that writes a register, and the frame that
    /// asks for a register's value. It saves only when it sends a write, and
    /// the lowering marks exactly that step. Every other kind never saves:
    /// `build_frame` builds the write's bytes and sends nothing.
    fn commits(&self, kind: &StepKind) -> StepCommits {
        if *kind == self.names.write_frame {
            StepCommits::AsDeclared
        } else {
            StepCommits::Never
        }
    }

    /// A `write_frame` changes the device exactly when it sends a write,
    /// and the lowering marks exactly that step. `open`, `build_frame`,
    /// `read_frame`, and `expect` send no write.
    fn changes(&self, kind: &StepKind) -> StepChanges {
        if *kind == self.names.write_frame {
            StepChanges::AsDeclared
        } else {
            StepChanges::Never
        }
    }

    /// By the verb: an `update` writes a register and reads it back; a
    /// `submit` writes a command that cannot be read back, so it cannot be
    /// verified and is `irreversible`. A register's hazard asks for more, by
    /// the pack's own rule `device.hazard_not_declared`.
    fn least_effect(&self, binding: &ResolvedBinding<'_>) -> EffectClass {
        if *binding.verb == self.names.submit || binding.field("firmware").is_some() {
            EffectClass::Irreversible
        } else if *binding.verb == self.names.update {
            EffectClass::Write
        } else {
            EffectClass::Read
        }
    }

    /// A `submit` that reads nothing back writes a command, and the device's
    /// echo is the only evidence there can be. An `update` keeps its read-back
    /// by address.
    fn evidence(&self, binding: &ResolvedBinding<'_>, lowered: &Lowered) -> WriteEvidence {
        if *binding.verb == self.names.submit && lowered.read_back.is_empty() {
            WriteEvidence::Acknowledged
        } else {
            WriteEvidence::ReadBack
        }
    }

    /// A binding of this pack starts no program and sets no variable. It
    /// sends frames over one link, to its surface's address and to no other:
    /// the address is a socket address the driver parses without a lookup,
    /// and it dials only an address among the run's destinations. A firmware
    /// update is frames over the same link. An input never names an address
    /// (decision 0334).
    fn statement(
        &self,
        _binding: &ResolvedBinding<'_>,
        _lowered: &Lowered,
        _cx: &LowerContext<'_>,
    ) -> Statement {
        Statement::Stated(BindingStatement {
            sends: Sends::Only {
                surface: true,
                addresses: Vec::new(),
            },
            ..BindingStatement::default()
        })
    }

    fn binding_schema(&self) -> &JsonSchema {
        &self.schema
    }

    fn identity(&self, _binding: &ResolvedBinding<'_>, strategy: RecordMatching) -> Possible {
        match strategy {
            RecordMatching::Natural | RecordMatching::None => Possible::Yes,
            RecordMatching::Composite | RecordMatching::Stamped | RecordMatching::Find => {
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
        let firmware = binding
            .field("firmware")
            .filter(|_| *binding.verb == self.names.update);
        if let Some(found) = binding.field("firmware")
            && firmware.is_none()
        {
            let verb = binding.verb.as_str().to_owned();
            scene.problem(
                "field_not_of_verb",
                &found.at,
                format!("A `{verb}` binding does not take `firmware`."),
                "Remove it, or use `verb: update`.",
            );
        }
        if let Some(firmware) = firmware {
            Self::absent(&mut scene, binding, &["read", "registers", "read_back"]);
            Self::firmware(
                &mut scene,
                binding,
                cx,
                firmware,
                &outputs,
                [&mut act, &mut confirm, &mut read_back],
            );
        } else if *binding.verb == self.names.read {
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
            // A map proven on one unit must not write to another: the
            // surface names the unit. The driver compares it, and refuses
            // a write in an environment that has none.
            Self::serial_declared(&mut scene, cx, at);
            let hazard = Self::hazard(&mut scene, binding, &name, &register);
            let updating = *binding.verb == self.names.update;
            // How many frames the read-back sends after the write.
            let mut reads_after = 0;
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
                        reads_after = u32::try_from(registers.len()).unwrap_or(u32::MAX);
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
                        format!(
                            "The binding writes the register `{name}` and does not read it back."
                        ),
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
            act.extend(
                Self::write_steps(&mut scene, cx, &name, &register, value, reads_after)
                    .into_iter()
                    .flatten(),
            );
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
