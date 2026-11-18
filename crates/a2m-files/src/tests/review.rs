//! Decision 0252: a record that is one fixed place, the review summary's
//! typed fields, and a sign-in declared before its probe is drafted.

use std::sync::Arc;

use super::example::example_files;
use crate::testing::{self, stand_set};
use crate::{
    BindingStatement, BindingStates, CompileMode, Compiled, CompilerSet, DRAFT_VERSION,
    Diagnostics, EnvironmentVariable, FixedPlace, JsonSchema, Lowered, Lowering, NOT_STATED,
    PROBE_PENDING, PROVIDER_NOT_HELD, PlaceReview, Possible, ProbeReview, ProgramRun,
    REACH_NOT_STATED, Reach, ReachGroup, RelPath, ResolvedBinding, SCRIPT_NOTICE,
    SENT_TO_NOT_STATED, SentAddress, SentTo, Statement, StepCommits, TargetRoot, VariableValue,
    compile, lint_one, load_files,
};
use a2m_core::{
    AddressScope, Digest, EffectClass, Environment, FileKindName, IdentityStrategy, Mode,
    ProbePlan, StepKind, SurfaceKind, Verb, Version, WriteEvidence,
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

fn compiled_with(
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
    let compiled = compiled_with(
        &with_settings(FIXED, "settings_put"),
        &set,
        CompileMode::Draft,
    )
    .unwrap_or_else(|problems| panic!("{problems}"));
    let operation = compiled
        .plan
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
    let example = compiled_with(&example_files(), &set, CompileMode::Draft).unwrap();
    assert!(example.review.items.iter().all(|item| item.place.is_none()));
}

#[test]
fn a_fixed_place_is_only_one_its_file_kind_declares_and_no_input_chooses() {
    let set = stand_set().unwrap();
    let refused = |files: &Files| -> String {
        let problems = compiled_with(files, &set, CompileMode::Draft).unwrap_err();
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
    let problems = compiled_with(
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
                sends_to_surface: true,
                sends_to: vec!["http://127.0.0.1:8080".to_owned()],
            }),
            "example_order_get" => Statement::Stated(BindingStatement {
                sends_to_surface: true,
                ..BindingStatement::default()
            }),
            "example_order_check" => Statement::Stated(BindingStatement::default()),
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
    let mut files = example_files();
    add(&mut files, "operations/order_check.yaml", ORDER_CHECK);
    files
}

#[test]
fn the_summary_shows_what_a_tool_starts_sets_and_reaches_by_the_kind_of_place() {
    let set = set_with(Arc::new(Stating {
        inner: testing::lowering(),
        names_place: true,
    }));
    let compiled = compiled_with(&stating_files(), &set, CompileMode::Draft)
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

#[test]
fn a_pack_that_states_nothing_is_shown_as_stating_nothing_and_never_as_nothing() {
    let set = stand_set().unwrap();
    let compiled = compiled_with(&example_files(), &set, CompileMode::Draft).unwrap();
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
    let mut compiled = compiled_with(&example_files(), &set, CompileMode::Draft).unwrap();
    let sign_in = &compiled.review.sign_ins[0];
    assert_eq!(sign_in.provider.as_str(), "form_login");
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
    let draft = compiled_with(&files, &set, CompileMode::Draft)
        .unwrap_or_else(|problems| panic!("{problems}"));
    assert_eq!(
        draft.plan.auth[0].probe,
        ProbePlan::Pending {
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
    let published = compiled_with(&files, &set, CompileMode::Publish).unwrap_err();
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
    let problems = compiled_with(&broken, &set, CompileMode::Draft).unwrap_err();
    assert!(
        problems.find("kernel.probe_pending").is_none(),
        "{problems}"
    );
}
