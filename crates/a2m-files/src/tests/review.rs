//! Decision 0252: a record that is one fixed place, the review summary's
//! typed fields, and a sign-in declared before its probe is drafted.

use std::sync::Arc;

use super::example::example_files;
use crate::testing::{self, stand_set};
use crate::{
    BindingStatement, BindingStates, CompileMode, Compiled, CompilerSet, DRAFT_VERSION,
    Diagnostics, EnvironmentVariable, FixedPlace, JsonSchema, LAUNCHER_NOTICE, Lowered, Lowering,
    NOT_STATED, PROBE_PENDING, PROVIDER_NOT_HELD, PlaceReview, Possible, ProbeReview, ProgramRun,
    REACH_NOT_STATED, Reach, ReachGroup, RelPath, ResolvedBinding, SCRIPT_NOTICE,
    SENT_TO_NOT_STATED, Sends, SentAddress, SentTo, Statement, StepCommits, TargetRoot,
    VariableValue, compile, lint_one, load_files,
};
use a2m_core::{
    AddressScope, CompiledProbe, Digest, EffectClass, Environment, FileKindName, IdentityStrategy,
    Mode, StepKind, SurfaceKind, Verb, Version, WriteEvidence,
};

type Files = Vec<(RelPath, Vec<u8>)>;

fn edit(files: &mut Files, path: &str, find: &str, replace: &str) {
    let file = files
        .iter_mut()
        .find(|(known, _)| known.as_str() == path)
        .unwrap();
    let text = String::from_utf8(file.1.clone()).unwrap();
    assert!(text.contains(find), "`{find}` is not in {path}");
    file.1 = text.replacen(find, replace, 1).into_bytes();
}

fn add(files: &mut Files, path: &str, text: &str) {
    files.push((RelPath::new(path).unwrap(), text.as_bytes().to_vec()));
}

fn compiled_target_with(
    files: &Files,
    set: &CompilerSet,
    mode: CompileMode,
) -> Result<Compiled, Diagnostics> {
    let sources = load_files(&TargetRoot::new("example"), files, set)?;
    let version = match mode {
        CompileMode::Draft => DRAFT_VERSION,
        CompileMode::Publish => Version::new(1, 0, 0),
    };
    compile(&sources, set, mode, version)
}

// ---- 1. A record that is one fixed place ----

const SETTINGS_CALLS: &str = "call: settings_get\npath: \"/settings\"\neffect: read\n---\ncall: settings_put\npath: \"/settings\"\neffect: write\n";

/// A write of one fixed place: the stand's settings, which no input names.
fn settings_set(identity: &str, call: &str) -> String {
    format!(
        "operation: example_settings_set
title: Set the settings
effect: write
{identity}
commit_points: 1
inputs:
  mode: {{type: identifier, description: \"The mode, such as daily\"}}
outputs: {{}}
bindings:
  - mode: stand
    verb: upsert
    status: shadow
    call: {call}
    args: {{mode: input.mode}}
    read_back: {{call: settings_get}}
description: >
  Sets the mode of the settings, which are one place.
"
    )
}

fn with_settings(identity: &str, call: &str) -> Files {
    let mut files = example_files();
    add(&mut files, "calls/settings.yaml", SETTINGS_CALLS);
    add(
        &mut files,
        "operations/settings_set.yaml",
        &settings_set(identity, call),
    );
    files
}

const FIXED: &str = "identity: {strategy: natural, place: fixed}";

#[test]
fn a_write_of_one_fixed_place_compiles_with_no_identity_input() {
    let set = stand_set().unwrap();
    let compiled = compiled_target_with(
        &with_settings(FIXED, "settings_put"),
        &set,
        CompileMode::Draft,
    )
    .unwrap_or_else(|problems| panic!("{problems}"));
    let operation = compiled
        .compiled_target
        .operations
        .iter()
        .find(|operation| operation.name.as_str() == "example_settings_set")
        .unwrap();
    // The record's key is the place, which the files name: no input is in it.
    assert_eq!(operation.identity.strategy, IdentityStrategy::Natural);
    assert!(operation.identity.key.is_empty());
    let item = compiled
        .review
        .items
        .iter()
        .find(|item| item.tool.as_str() == "example_settings_set")
        .unwrap();
    assert_eq!(
        item.place,
        Some(PlaceReview {
            surface: "desk".parse().unwrap(),
            kind: testing::call_kind(),
            name: "settings_put".to_owned(),
        })
    );
    // The example names no fixed place, and its summary shows none.
    let example = compiled_target_with(&example_files(), &set, CompileMode::Draft).unwrap();
    assert!(example.review.items.iter().all(|item| item.place.is_none()));
}

#[test]
fn a_fixed_place_is_only_one_its_file_kind_declares_and_no_input_chooses() {
    let set = stand_set().unwrap();
    let refused = |files: &Files| -> String {
        let problems = compiled_target_with(files, &set, CompileMode::Draft).unwrap_err();
        let found = problems
            .find("kernel.place_not_fixed")
            .unwrap_or_else(|| panic!("{problems}"));
        assert!(!found.fix.is_empty());
        found.message.clone()
    };
    // A call whose path an input fills is not one place.
    let mut files = with_settings(FIXED, "settings_put");
    edit(
        &mut files,
        "calls/settings.yaml",
        "path: \"/settings\"\neffect: write",
        "path: \"/settings/{mode}\"\neffect: write",
    );
    assert!(refused(&files).contains("declares a fixed place"));
    // An identity input beside `place: fixed`.
    let files = with_settings(
        "identity: {strategy: natural, place: fixed, inputs: [mode]}",
        "settings_put",
    );
    assert!(refused(&files).contains("names inputs"));
    // A strategy that is not `natural`.
    let files = with_settings(
        "identity: {strategy: composite, place: fixed}",
        "settings_put",
    );
    assert!(refused(&files).contains("composite"));
    // A lowering that names no place: a pack that does not answer.
    let silent = set_with(Arc::new(Stating {
        inner: testing::lowering(),
        names_place: false,
    }));
    let problems = compiled_target_with(
        &with_settings(FIXED, "settings_put"),
        &silent,
        CompileMode::Draft,
    )
    .unwrap_err();
    assert!(
        problems
            .find("kernel.place_not_fixed")
            .unwrap()
            .message
            .contains("names no fixed place"),
        "{problems}"
    );
}

// ---- 2. The review summary ----

/// The stand's lowering, with what a pack states for the review summary.
struct Stating {
    inner: Arc<dyn Lowering>,
    names_place: bool,
}

fn digest(byte: u8) -> Digest {
    Digest::of(&[byte])
}

impl Lowering for Stating {
    fn mode(&self) -> Mode {
        self.inner.mode()
    }
    fn verbs(&self) -> &[Verb] {
        self.inner.verbs()
    }
    fn default_verb(&self) -> Option<Verb> {
        self.inner.default_verb()
    }
    fn reads(&self) -> &[(FileKindName, SurfaceKind)] {
        self.inner.reads()
    }
    fn step_kinds(&self) -> &[StepKind] {
        self.inner.step_kinds()
    }
    fn commits(&self, kind: &StepKind) -> StepCommits {
        self.inner.commits(kind)
    }
    fn changes(&self, kind: &StepKind) -> crate::StepChanges {
        self.inner.changes(kind)
    }
    fn least_effect(&self, binding: &ResolvedBinding<'_>) -> EffectClass {
        self.inner.least_effect(binding)
    }
    fn evidence(&self, binding: &ResolvedBinding<'_>, lowered: &Lowered) -> WriteEvidence {
        self.inner.evidence(binding, lowered)
    }
    fn binding_schema(&self) -> &JsonSchema {
        self.inner.binding_schema()
    }
    fn identity(&self, binding: &ResolvedBinding<'_>, strategy: IdentityStrategy) -> Possible {
        self.inner.identity(binding, strategy)
    }
    fn fixed_place(&self, binding: &ResolvedBinding<'_>) -> Option<FixedPlace> {
        if self.names_place {
            self.inner.fixed_place(binding)
        } else {
            None
        }
    }
    fn lower(
        &self,
        binding: &ResolvedBinding<'_>,
        cx: &crate::LowerContext<'_>,
    ) -> Result<Lowered, Diagnostics> {
        self.inner.lower(binding, cx)
    }
    /// The upsert starts a script and a program and sets three variables;
    /// `example_order_get` reaches its surface; `example_order_check`
    /// sends nothing anywhere.
    fn statement(
        &self,
        binding: &ResolvedBinding<'_>,
        _lowered: &Lowered,
        _cx: &crate::LowerContext<'_>,
    ) -> Statement {
        match binding.operation.name().as_str() {
            "example_line_item_upsert" => Statement::Stated(BindingStatement {
                runs: vec![
                    ProgramRun::Script {
                        interpreter: "/usr/bin/example-interpreter".to_owned(),
                        interpreter_digests: vec![digest(1)],
                        arguments: vec!["-I".to_owned()],
                        script: "scripts/put.example".to_owned(),
                        script_digest: digest(2),
                    },
                    ProgramRun::Program {
                        path: "/opt/example/bin/helper".to_owned(),
                        digests: Vec::new(),
                    },
                    ProgramRun::Launcher {
                        launcher: "/usr/bin/example-launcher".to_owned(),
                        launcher_digests: vec![digest(4)],
                        arguments: vec!["--fixed".to_owned()],
                        entry: "/opt/example/lib/server.example".to_owned(),
                        entry_digest: digest(5),
                    },
                ],
                environment: vec![
                    EnvironmentVariable {
                        name: "EXAMPLE_MODE".to_owned(),
                        value: VariableValue::Set("test".to_owned()),
                    },
                    EnvironmentVariable {
                        name: "EXAMPLE_KEY".to_owned(),
                        value: VariableValue::Secret,
                    },
                    EnvironmentVariable {
                        name: "LANG".to_owned(),
                        value: VariableValue::Passed,
                    },
                ],
                sends: Sends::Only {
                    surface: true,
                    addresses: vec!["http://127.0.0.1:8080".to_owned()],
                },
            }),
            "example_order_get" => Statement::Stated(BindingStatement {
                sends: Sends::Only {
                    surface: true,
                    addresses: Vec::new(),
                },
                ..BindingStatement::default()
            }),
            "example_order_check" => Statement::Stated(BindingStatement {
                sends: Sends::Only {
                    surface: false,
                    addresses: Vec::new(),
                },
                ..BindingStatement::default()
            }),
            // Starts a program, and cannot say where that program sends.
            "example_settings_set" => Statement::Stated(BindingStatement {
                runs: vec![ProgramRun::Program {
                    path: "/opt/example/bin/settings".to_owned(),
                    digests: vec![digest(3)],
                }],
                ..BindingStatement::default()
            }),
            _ => Statement::NotStated,
        }
    }
}

/// The stand pack, with `lowering` in place of its own.
fn set_with(lowering: Arc<dyn Lowering>) -> CompilerSet {
    let mut builder = CompilerSet::builder();
    for part in testing::STAND_PARTS {
        if *part == "lowering" {
            builder
                .lowering(testing::PACK, Arc::clone(&lowering))
                .unwrap();
        } else {
            testing::register_part(&mut builder, testing::PACK, part).unwrap();
        }
    }
    builder.build()
}

const ORDER_CHECK: &str = "operation: example_order_check
title: Check an order exists
effect: read
key: order_id
inputs:
  order_id: {type: identifier, description: \"Order number shown in the application\"}
outputs: {}
bindings:
  - mode: stand
    call: order_get
description: >
  Answers whether one order exists, and reads nothing else of it.
";

fn stating_files() -> Files {
    let mut files = with_settings(FIXED, "settings_put");
    add(&mut files, "operations/order_check.yaml", ORDER_CHECK);
    files
}

#[test]
fn the_summary_shows_what_a_tool_starts_sets_and_reaches_by_the_kind_of_place() {
    let set = set_with(Arc::new(Stating {
        inner: testing::lowering(),
        names_place: true,
    }));
    let compiled = compiled_target_with(&stating_files(), &set, CompileMode::Draft)
        .unwrap_or_else(|problems| panic!("{problems}"));
    let item = |name: &str| {
        compiled
            .review
            .items
            .iter()
            .find(|item| item.tool.as_str() == name)
            .unwrap()
    };
    let upsert = item("example_line_item_upsert");
    let BindingStates::Stated { runs, environment } = &upsert.bindings[0].states else {
        panic!("{:?}", upsert.bindings[0]);
    };
    assert_eq!(runs[0].notice, Some(SCRIPT_NOTICE));
    assert_eq!(runs[1].notice, None);
    // A server started through a launcher is its own kind, with its own
    // notice, and is never worded as a script the target keeps.
    assert!(matches!(runs[2].run, ProgramRun::Launcher { .. }));
    assert_eq!(runs[2].notice, Some(LAUNCHER_NOTICE));
    assert_ne!(LAUNCHER_NOTICE, SCRIPT_NOTICE);
    assert!(
        serde_json::to_string(&runs[2].run)
            .unwrap()
            .contains(r#""kind":"launcher""#)
    );
    assert_eq!(environment.len(), 3);
    assert_eq!(
        upsert.reach,
        Reach::Stated {
            groups: vec![
                ReachGroup {
                    scope: AddressScope::Name,
                    addresses: vec![
                        "https://app.example.com".to_owned(),
                        "https://test.example.com".to_owned(),
                    ],
                },
                ReachGroup {
                    scope: AddressScope::ThisMachine,
                    addresses: vec!["http://127.0.0.1:8080".to_owned()],
                },
            ],
            leaves_this_machine: true,
        }
    );
    // A secret variable's value is never in the summary; a set one's is.
    let json = serde_json::to_string(&compiled.review).unwrap();
    assert!(
        json.contains(r#"{"name":"EXAMPLE_KEY","value":{"kind":"secret"}}"#),
        "{json}"
    );
    assert!(json.contains(r#"{"name":"EXAMPLE_MODE","value":{"kind":"set","value":"test"}}"#));
    // A read that sends nothing anywhere says so: an empty, complete reach.
    let check = item("example_order_check");
    assert_eq!(
        check.reach,
        Reach::Stated {
            groups: Vec::new(),
            leaves_this_machine: false
        }
    );
    // A binding that starts a program states it, and cannot say where the
    // program sends: the reach says so, and is never an empty "nowhere".
    let settings = item("example_settings_set");
    assert!(matches!(
        &settings.bindings[0].states,
        BindingStates::Stated { runs, .. } if runs[0].notice.is_none()
    ));
    assert!(matches!(
        settings.reach,
        Reach::Partly {
            says: REACH_NOT_STATED,
            ..
        }
    ));
    // Of two reads, the one that reaches off the machine is shown first,
    // though its name sorts after.
    let reads: Vec<&str> = compiled
        .review
        .items
        .iter()
        .filter(|item| item.effect == EffectClass::Read)
        .map(|item| item.tool.as_str())
        .collect();
    assert_eq!(reads, ["example_order_get", "example_order_check"]);
}

/// Every text of a summary passes through the function a page gives,
/// such as the redactor; the fixed sentences, the checked names, and a
/// secret's absence stay as they are.
#[test]
fn every_text_of_the_summary_passes_through_the_function_given() {
    let set = set_with(Arc::new(Stating {
        inner: testing::lowering(),
        names_place: true,
    }));
    let mut review = compiled_target_with(&stating_files(), &set, CompileMode::Draft)
        .unwrap_or_else(|problems| panic!("{problems}"))
        .review;
    let test: Environment = "test".parse().unwrap();
    assert!(review.state_sent_to(
        0,
        &test,
        Some(vec![SentAddress {
            address: "https://test.example.com/login".to_owned(),
            what: "the sign-in form".to_owned(),
        }])
    ));
    let before = serde_json::to_string(&review).unwrap();
    let mapped = review.map_text(&|_| "hidden-text".to_owned());
    let after = serde_json::to_string(&mapped).unwrap();
    for text in [
        "/usr/bin/example-interpreter",
        "/usr/bin/example-launcher",
        "/opt/example/lib/server.example",
        "\"-I\"",
        "scripts/put.example",
        "/opt/example/bin/helper",
        "EXAMPLE_MODE",
        "\"test\"}",
        "http://127.0.0.1:8080",
        "https://test.example.com/login",
        "the sign-in form",
        "\"password\"",
    ] {
        assert!(before.contains(text), "{text}: {before}");
        assert!(!after.contains(text), "{text}: {after}");
    }
    for kept in [
        SCRIPT_NOTICE,
        LAUNCHER_NOTICE,
        REACH_NOT_STATED,
        r#""value":{"kind":"secret"}"#,
        "example_line_item_upsert",
        "vault://example/service/password",
    ] {
        assert!(after.contains(kept), "{kept}: {after}");
    }
    assert_eq!(mapped.items.len(), review.items.len());
    assert_eq!(mapped.sign_ins.len(), review.sign_ins.len());
}

#[test]
fn a_pack_that_states_nothing_is_shown_as_stating_nothing_and_never_as_nothing() {
    let set = stand_set().unwrap();
    let compiled = compiled_target_with(&example_files(), &set, CompileMode::Draft).unwrap();
    for item in &compiled.review.items {
        assert!(!item.bindings.is_empty(), "{}", item.tool);
        for binding in &item.bindings {
            assert_eq!(
                binding.states,
                BindingStates::NotStated { says: NOT_STATED }
            );
        }
        let Reach::Partly { says, .. } = &item.reach else {
            panic!("{:?}", item.reach);
        };
        assert_eq!(*says, REACH_NOT_STATED);
    }
    let json = serde_json::to_string(&compiled.review).unwrap();
    assert!(json.contains(NOT_STATED) && json.contains(REACH_NOT_STATED));
}

#[test]
fn a_sign_in_shows_its_provider_its_credentials_by_name_and_where_it_sends() {
    let set = stand_set().unwrap();
    let mut compiled = compiled_target_with(&example_files(), &set, CompileMode::Draft).unwrap();
    let sign_in = &compiled.review.sign_ins[0];
    assert_eq!(sign_in.provider.as_str(), "http_basic");
    assert_eq!(sign_in.credentials[0].role, "password");
    assert_eq!(
        sign_in.credentials[0].reference.to_string(),
        "vault://example/service/password"
    );
    assert_eq!(
        sign_in.probe,
        ProbeReview::Declared {
            name: "example_order_get".to_owned()
        }
    );
    // Until the session keeper says where the provider sends, the summary
    // says that it has not been said.
    assert!(sign_in.sent_to.iter().all(|sent| sent.sent_to
        == SentTo::NotStated {
            says: SENT_TO_NOT_STATED
        }));
    let test: Environment = "test".parse().unwrap();
    let production: Environment = "production".parse().unwrap();
    assert!(compiled.review.state_sent_to(
        0,
        &test,
        Some(vec![SentAddress {
            address: "https://test.example.com/login".to_owned(),
            what: "the sign-in form".to_owned(),
        }])
    ));
    assert!(compiled.review.state_sent_to(0, &production, None));
    assert!(!compiled.review.state_sent_to(1, &test, None));
    let sent: Vec<&SentTo> = compiled.review.sign_ins[0]
        .sent_to
        .iter()
        .map(|sent| &sent.sent_to)
        .collect();
    assert!(sent.contains(&&SentTo::ProviderNotHeld {
        says: PROVIDER_NOT_HELD
    }));
    assert!(sent.iter().any(|sent| matches!(
        sent,
        SentTo::Addresses { groups, leaves_this_machine: true }
            if groups[0].scope == AddressScope::Name
    )));
}

// ---- 3. A sign-in before its probe is drafted ----

fn with_pending_probe() -> Files {
    let mut files = example_files();
    edit(
        &mut files,
        "auth.yaml",
        "probe: example_order_get\n    probe_inputs: {order_id: \"SO-1001\"}",
        "probe: example_session_check",
    );
    files
}

#[test]
fn a_sign_in_may_name_its_probe_before_the_read_is_drafted_in_a_draft_only() {
    let set = stand_set().unwrap();
    let files = with_pending_probe();
    // A draft compiles, with the sign-in marked in the lint result and in
    // the summary.
    let draft = compiled_target_with(&files, &set, CompileMode::Draft)
        .unwrap_or_else(|problems| panic!("{problems}"));
    assert_eq!(
        draft.compiled_target.auth[0].probe,
        CompiledProbe::Pending {
            name: "example_session_check".to_owned()
        }
    );
    let warning = draft.warnings.find("kernel.probe_pending").unwrap();
    assert!(warning.message.contains("not confirmed"));
    assert_eq!(
        draft.review.sign_ins[0].probe,
        ProbeReview::Pending {
            name: "example_session_check".to_owned(),
            says: PROBE_PENDING,
        }
    );
    // `draft.lint` reports it the same way.
    let sources = load_files(&TargetRoot::new("example"), &files, &set).unwrap();
    let auth = sources
        .files
        .iter()
        .find(|file| file.path.as_str() == "auth.yaml")
        .cloned()
        .unwrap();
    assert!(
        lint_one(&sources, auth, &set)
            .find("kernel.probe_pending")
            .is_some()
    );
    // A publish never compiles one: a session nobody can check is a
    // session nobody can trust.
    let published = compiled_target_with(&files, &set, CompileMode::Publish).unwrap_err();
    assert!(
        published.find("kernel.unknown_name").is_some(),
        "{published}"
    );
    assert!(published.find("kernel.probe_pending").is_none());
    // A probe that names an operation with errors is that operation's
    // problem, never a probe still to come.
    let mut broken = with_pending_probe();
    edit(
        &mut broken,
        "auth.yaml",
        "probe: example_session_check",
        "probe: example_order_get",
    );
    edit(
        &mut broken,
        "operations/order_get.yaml",
        "call: order_get",
        "call: order_missing",
    );
    let problems = compiled_target_with(&broken, &set, CompileMode::Draft).unwrap_err();
    assert!(
        problems.find("kernel.probe_pending").is_none(),
        "{problems}"
    );
}

// ---- 3. The summary's order, whole (package 9.3r) ----

/// A tool that reads, harms nothing else, sends only to this machine, and
/// starts nothing: each case below differs from it in one key.
fn plain_item(name: &str) -> crate::ReviewItem {
    crate::ReviewItem {
        tool: a2m_core::OperationName::new(name).unwrap(),
        effect: EffectClass::Read,
        hazard: a2m_core::Hazard::None,
        commit_points: 0,
        surfaces: Vec::new(),
        destinations: Vec::new(),
        uploads: Vec::new(),
        downloads: Vec::new(),
        place: None,
        bindings: Vec::new(),
        reach: Reach::Stated {
            groups: Vec::new(),
            leaves_this_machine: false,
        },
    }
}

fn starting(name: &str, run: ProgramRun) -> crate::ReviewItem {
    let mut item = plain_item(name);
    item.bindings.push(crate::BindingReview {
        operation: item.tool.clone(),
        mode: Mode::new("stand").unwrap(),
        surface: a2m_core::SurfaceName::new("app").unwrap(),
        surface_programs: Vec::new(),
        states: BindingStates::Stated {
            runs: vec![crate::RunReview { run, notice: None }],
            environment: Vec::new(),
        },
    });
    item
}

fn sign_in(entry: usize, pending: bool) -> crate::SignInReview {
    crate::SignInReview {
        entry,
        surfaces: Vec::new(),
        provider: a2m_core::AuthProviderName::new("example_form").unwrap(),
        mode: a2m_core::SignInMode::ManagedSession,
        credentials: Vec::new(),
        probe: if pending {
            ProbeReview::Pending {
                name: format!("probe_{entry}"),
                says: PROBE_PENDING,
            }
        } else {
            ProbeReview::Declared {
                name: format!("probe_{entry}"),
            }
        },
        sent_to: Vec::new(),
    }
}

/// Every key of the order, one at a time, whatever order the tools come
/// in: hazard, then effect class, then what a tool exposes (an unknown
/// reach, then a reach off the machine, then what it starts: a script
/// before a program), then name. Each name sorts against its place, so no
/// case passes by its name. Sign-ins: one nobody can check yet first, then
/// the rest, each in the file's order.
#[test]
fn the_summary_is_ordered_by_hazard_then_effect_then_exposure_then_name() {
    // Each name sorts against its place, the riskiest last by name.
    let mut cases = Vec::new();
    let mut item = plain_item("example_z9_physical");
    item.hazard = a2m_core::Hazard::Physical;
    cases.push(item);
    let mut item = plain_item("example_z8_sends_outside");
    item.hazard = a2m_core::Hazard::SendsOutside;
    cases.push(item);
    let mut item = plain_item("example_z7_irreversible");
    item.effect = EffectClass::Irreversible;
    cases.push(item);
    let mut item = plain_item("example_z6_write");
    item.effect = EffectClass::Write;
    cases.push(item);
    let mut item = plain_item("example_z5_reach_unknown");
    item.reach = Reach::Partly {
        known: Vec::new(),
        says: REACH_NOT_STATED,
    };
    cases.push(item);
    let mut item = plain_item("example_z4_reach_leaves");
    item.reach = Reach::Stated {
        groups: vec![ReachGroup {
            scope: AddressScope::Name,
            addresses: vec!["https://app.example.com".to_owned()],
        }],
        leaves_this_machine: true,
    };
    cases.push(item);
    cases.push(starting(
        "example_z3_starts_script",
        ProgramRun::Script {
            interpreter: "/usr/bin/example-interpreter".to_owned(),
            interpreter_digests: vec![digest(1)],
            arguments: Vec::new(),
            script: "scripts/put.example".to_owned(),
            script_digest: digest(2),
        },
    ));
    cases.push(starting(
        "example_z2_starts_program",
        ProgramRun::Program {
            path: "/opt/example/bin/helper".to_owned(),
            digests: vec![digest(3)],
        },
    ));
    cases.push(plain_item("example_z0_plain"));
    cases.push(plain_item("example_z1_plain"));
    let expected: Vec<String> = cases.iter().map(|item| item.tool.to_string()).collect();
    // Every rotation of the cases, forward and backward, and one scramble.
    let mut orders: Vec<Vec<crate::ReviewItem>> = Vec::new();
    for turn in 0..cases.len() {
        let mut order = cases.clone();
        order.rotate_left(turn);
        orders.push(order.clone());
        order.reverse();
        orders.push(order);
    }
    orders.push(
        [3, 8, 0, 6, 9, 1, 5, 2, 7, 4]
            .iter()
            .map(|index| cases[*index].clone())
            .collect(),
    );
    for mut order in orders {
        crate::review::in_review_order(&mut order);
        let got: Vec<String> = order.iter().map(|item| item.tool.to_string()).collect();
        assert_eq!(got, expected);
    }
    // Two pending sign-ins and two declared ones, given out of order.
    let mut sign_ins = vec![
        sign_in(2, false),
        sign_in(3, true),
        sign_in(0, false),
        sign_in(1, true),
    ];
    crate::review::sign_ins_in_review_order(&mut sign_ins);
    let entries: Vec<usize> = sign_ins.iter().map(|sign_in| sign_in.entry).collect();
    assert_eq!(entries, [1, 3, 0, 2]);
}
