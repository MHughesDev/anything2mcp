//! The lowering of the `desktop` mode, for reads only: one binding into a
//! `goto`, a `wait`, and a `read`, each naming its screen in the pack's
//! section of the plan.
//!
//! | Verb | Steps |
//! | --- | --- |
//! | `read`, `list` | `goto` (find the screen's window among the application's), `wait` (its `ready` condition), `read` (fields, or a table's columns as lists) |
//! | the write verbs | Not served: the mode declares none, and a binding on an operation declared to write is refused as `desktop.writes_not_built` |
//!
//! Every step only looks. None saves, and none is marked as a commit point.

use a2m_core::{
    EffectClass, FailsAs, FailureClass, FileKindName, IdentityStrategy, Mode, PackPayload, PackRef,
    SectionIndex, Step, StepKind, SurfaceKind, SurfaceName, ValueType, Verb,
};
use a2m_files::{
    Diagnostics, JsonSchema, LowerContext, Lowered, Lowering, Place, Possible, ResolvedBinding,
    Stage, StepCommits,
};

use crate::codes;
use crate::model::{PAYLOAD_FORMAT, ReadItem, ReadStep, Screen, ScreenSection};
use crate::screens::{DESKTOP_SURFACE, SCREEN_KIND, problem};

/// The step that finds a screen's window among the application's windows.
pub const GOTO: StepKind = StepKind::fixed("goto");
/// The step that waits until a screen's window can be read.
pub const WAIT: StepKind = StepKind::fixed("wait");
/// The step that reads fields, or the rows of a table.
pub const READ: StepKind = StepKind::fixed("read");
/// The step a discoverer sends: the shape of the application's windows.
pub const SNAPSHOT: StepKind = StepKind::fixed("snapshot");

const READ_VERBS: &[&str] = &["read", "list"];

/// How long a step may take when the target states no timeout under the
/// step kind's name.
const DEFAULT_TIMEOUT_MS: u32 = 30_000;

/// The lowering of the `desktop` mode.
pub(crate) struct DesktopLowering {
    mode: Mode,
    schema: JsonSchema,
    verbs: Vec<Verb>,
    step_kinds: Vec<StepKind>,
}

impl DesktopLowering {
    pub(crate) fn new(mode: Mode) -> Self {
        Self {
            mode,
            schema: JsonSchema::object().required("screen", JsonSchema::name()),
            verbs: READ_VERBS.iter().map(|verb| Verb::fixed(verb)).collect(),
            step_kinds: vec![GOTO, WAIT, READ],
        }
    }
}

struct Found<'a> {
    name: &'a str,
    at: &'a Place,
    section: SectionIndex,
    screen: Screen,
}

fn step(cx: &LowerContext<'_>, kind: StepKind, found: &Found<'_>, fails_as: FailsAs) -> Step {
    Step {
        timeout_ms: cx
            .target()
            .default_timeout_ms(kind.as_str())
            .unwrap_or(DEFAULT_TIMEOUT_MS),
        kind,
        refers_to: Some(PackRef {
            section: found.section,
            entry: found.name.to_owned(),
        }),
        args: Vec::new(),
        fails_as,
        commits: false,
        lookup: false,
        idempotent: true,
        when: None,
        payload: PackPayload::none(),
    }
}

impl Lowering for DesktopLowering {
    fn mode(&self) -> Mode {
        self.mode.clone()
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

    /// No step of this mode saves.
    fn commits(&self, _kind: &StepKind) -> StepCommits {
        StepCommits::Never
    }

    fn least_effect(&self, _binding: &ResolvedBinding<'_>) -> EffectClass {
        EffectClass::Read
    }

    fn binding_schema(&self) -> &JsonSchema {
        &self.schema
    }

    fn identity(&self, binding: &ResolvedBinding<'_>, strategy: IdentityStrategy) -> Possible {
        match strategy {
            IdentityStrategy::None | IdentityStrategy::Natural | IdentityStrategy::Composite
                if READ_VERBS.contains(&binding.verb.as_str()) =>
            {
                Possible::Yes
            }
            _ => Possible::No("a `desktop` binding only reads in this version.".to_owned()),
        }
    }

    fn lower(
        &self,
        binding: &ResolvedBinding<'_>,
        cx: &LowerContext<'_>,
    ) -> Result<Lowered, Diagnostics> {
        let at = &binding.doc.at;
        // The write half needs what this version does not have: a back end
        // that sets and invokes, a commit point, a confirmation, and a
        // read-back after the window is reached afresh. Until it is built,
        // a desktop binding on an operation that writes is refused by name.
        let reads = READ_VERBS.contains(&binding.verb.as_str());
        if !reads || cx.operation().effect() != EffectClass::Read {
            return Err(Diagnostics::from(problem(
                "writes_not_built",
                Stage::Analyze,
                at,
                "A `desktop` binding only reads in this version: setting a control, pressing an action, and reading the record back afterwards are not built.",
                "Make this operation a read, with `effect: read` and the verb `read` or `list`.",
            )));
        }
        let mut problems = Diagnostics::new();
        let named = binding.doc.get("screen");
        let name = named.and_then(|named| named.as_text()).unwrap_or_default();
        let name_at = named.map_or(at, |named| &named.at);
        let found = cx
            .section(&SCREEN_KIND, &DESKTOP_SURFACE)
            .and_then(|(index, section)| {
                let decoded = if section.format == PAYLOAD_FORMAT {
                    section.payload.decode::<ScreenSection>().ok()
                } else {
                    None
                };
                decoded
                    .and_then(|section| section.get(name).cloned())
                    .map(|screen| Found {
                        name,
                        at: name_at,
                        section: index,
                        screen,
                    })
            });
        let Some(found) = found else {
            return Err(Diagnostics::from(problem(
                "unknown_screen",
                Stage::Resolve,
                name_at,
                format!(
                    "No file under `screens/` defines a screen named `{}` on a surface of the kind `desktop`.",
                    codes::shown(name)
                ),
                "Name a screen that `screens/` defines on a `desktop` surface.",
            )));
        };
        let surface = SurfaceName::new(&found.screen.surface).map_err(|_| {
            Diagnostics::from(problem(
                "unknown_screen",
                Stage::Resolve,
                found.at,
                "The screen's surface has no well-formed name.",
                "Name a surface the target declares.",
            ))
        })?;

        // One value for each output: from the screen's field of the same
        // name, or, for a list, from its table's column of that name.
        let mut payload = ReadStep::default();
        for output in cx.operation().outputs() {
            let listed = matches!(output.ty, ValueType::List(_));
            let known = if listed {
                found
                    .screen
                    .rows
                    .as_ref()
                    .is_some_and(|rows| rows.columns.contains_key(&output.name))
            } else {
                found.screen.fields.contains_key(&output.name)
            };
            if !known {
                problems.push(problem(
                    "unknown_field",
                    Stage::Resolve,
                    found.at,
                    format!(
                        "The output `{}` matches no {} on the screen `{}`.",
                        output.name,
                        if listed { "column of its table" } else { "field" },
                        found.name
                    ),
                    "Add it to the screen, or rename the output.",
                ));
                continue;
            }
            let item = ReadItem {
                output: output.name.clone(),
                from: output.name.clone(),
            };
            if listed {
                payload.columns.push(item);
            } else {
                payload.fields.push(item);
            }
        }
        let encoded = match PackPayload::encode(&payload) {
            Ok(encoded) => encoded,
            Err(failure) => {
                problems.push(problem(
                    "not_encodable",
                    Stage::Lower,
                    found.at,
                    failure.message().to_owned(),
                    "Report this to the desktop pack's author.",
                ));
                PackPayload::none()
            }
        };
        if !problems.is_empty() {
            return Err(problems);
        }
        let unexpected = FailsAs {
            class: FailureClass::UnknownVariant,
            code: codes::UNEXPECTED_PAGE,
        };
        let mut read = step(
            cx,
            READ,
            &found,
            FailsAs {
                class: FailureClass::Drift,
                code: codes::LOCATOR_NOT_FOUND,
            },
        );
        read.payload = encoded;
        Ok(Lowered {
            surface,
            act: vec![
                step(cx, GOTO, &found, unexpected),
                step(cx, WAIT, &found, unexpected),
                read,
            ],
            confirm: Vec::new(),
            read_back: Vec::new(),
            lookup: Vec::new(),
            settle_ms: 0,
            prefer_response: false,
        })
    }
}
