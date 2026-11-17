//! The protected policy settings of spec section 226, and how each is put in
//! force by the policy engine.
//!
//! Whether something may happen is the policy engine's to decide, so a
//! setting about it is in force only as far as the policy engine decides by
//! it. The kernel opens the policy engine with the built-in set of its
//! profile and the installation's policy settings in place of the set's own
//! (`BuiltIn::with_settings`); the policy engine keeps them in force under
//! any set published later, which can only tighten them (decision 0145).
//! A value the policy engine cannot work by stops the kernel with the
//! setting's name: a limit is never reported that is not kept.
//!
//! | Setting | In force through |
//! | --- | --- |
//! | `policy.approve_writes`, `policy.max_records_per_run`, `policy.approval_window_minutes`, `policy.agent_effect_ceiling`, `policy.writes_outside_tests`, `policy.two_person_irreversible`, `policy.pre_approval` | The policy engine's settings, handed in with `BuiltIn::with_settings` |
//! | `policy.approve_writes` = `caller_confirms`, or `named_approver` on one machine | Nothing: the policy engine has no such approver; the kernel stops |
//! | `policy.pre_approval` | Also the guard: with it off, a write is never begun on a pre-approval |
//! | `policy.preview_freshness_seconds` | The engine's freshness window |
//! | `files.allowed_roots` | The dispatcher's check of a stated project folder |

use a2m_core::{EffectClass, Value};
use a2m_policy::{ApproveWrites, BuiltIn, Profile};

use crate::codes;
use crate::settings::Settings;

/// Whether a pre-approval rule may let a write begin without a person.
pub(crate) const PRE_APPROVAL: &str = "policy.pre_approval";

/// A protected setting that says what is not in force.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NotInForce {
    /// The setting's key.
    pub(crate) key: &'static str,
    /// The refusal, which names the setting, the value it was given, the
    /// value in force, and where the setting was set.
    pub(crate) failure: a2m_core::Failure,
}

/// The built-in set of the profile with the installation's policy settings
/// carried into it, and every setting whose value the policy engine cannot
/// work by. What is refused keeps the built-in set's own value.
pub(crate) fn carried(settings: &Settings, built_in: BuiltIn) -> (BuiltIn, Vec<NotInForce>) {
    let mut policy = built_in.layers().settings.clone();
    let mut refused = Vec::new();
    let count = |key: &str| {
        settings
            .int(key)
            .and_then(|number| u32::try_from(number).ok())
    };
    if let Some(records) = count("policy.max_records_per_run") {
        policy.max_records_per_run = records;
    }
    if let Some(minutes) = count("policy.approval_window_minutes") {
        policy.approval_window_minutes = minutes;
    }
    if let Some(ceiling) = settings
        .text("policy.agent_effect_ceiling")
        .and_then(|word| word.parse::<EffectClass>().ok())
    {
        policy.agent_effect_ceiling = ceiling;
    }
    if let Some(flag) = settings.flag("policy.writes_outside_tests") {
        policy.writes_outside_tests = flag;
    }
    if let Some(flag) = settings.flag("policy.two_person_irreversible") {
        policy.two_person_irreversible = flag;
    }
    if let Some(flag) = settings.flag(PRE_APPROVAL) {
        policy.pre_approval = flag;
    }
    if let Some(word) = settings.text("policy.approve_writes") {
        let local = built_in.profile() == Profile::Local;
        match ApproveWrites::from_text(word) {
            Some(ApproveWrites::NamedApprover) if local => refused.push(not_in_force(
                settings,
                "policy.approve_writes",
                ApproveWrites::PersonEachRun.as_str(),
                "on one machine a write is approved by the person at it, and no other approver can be named",
            )),
            Some(approver) => policy.approve_writes = approver,
            None => refused.push(not_in_force(
                settings,
                "policy.approve_writes",
                policy.approve_writes.as_str(),
                "the policy engine has no approver of that kind",
            )),
        }
    }
    (built_in.with_settings(policy), refused)
}

fn not_in_force(settings: &Settings, key: &'static str, in_force: &str, why: &str) -> NotInForce {
    let (given, origin) = settings.entry(key).map_or_else(
        || ("another value".to_owned(), "a setting".to_owned()),
        |entry| {
            let given = match &entry.value {
                Value::Text(text) => text.clone(),
                Value::Int(number) => number.to_string(),
                Value::Bool(flag) => flag.to_string(),
                _ => "another value".to_owned(),
            };
            (given, entry.origin.to_string())
        },
    );
    NotInForce {
        key,
        failure: codes::policy(
            codes::SETTING_NOT_IN_FORCE,
            format!(
                "The setting `{key}` is set to `{given}` by {origin}, and what is in force is `{in_force}`: {why}. The engine does not work under a limit it would report and not keep."
            ),
            format!(
                "Take `{key}` out of {origin}, or set it to `{in_force}`, then start the engine again."
            ),
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    use a2m_core::{
        Digest, EffectClass, Environment, FailureClass, OperationName, PrincipalId, TargetName,
        Timestamp, WorkspaceId,
    };
    use a2m_ledger::{
        Durability, Fence, NewRun, Opened, RetentionClass, Store, StoreConfig, ToolKind,
    };
    use a2m_policy::{
        ApprovalSurface, Approved, BuiltIn, Channel, Context, LimitCheck, ModuleName, Outcome,
        Policy, PresenceCheck, PrincipalRef, Question, Request, Resource, ToolRef,
    };
    use a2m_telemetry::ManualClock;
    use a2m_vault::{MemoryKeyStore, RandomSource, Vault};

    use super::carried;
    use crate::codes;
    use crate::settings::{PRESETS, Settings, SourceFile, Sources};

    const START: i64 = 1_800_000_000_000;

    /// Random bytes that are the same on every run: a counter, hashed.
    struct Counted(AtomicU64);

    impl RandomSource for Counted {
        fn fill(&self, out: &mut [u8]) -> Result<(), a2m_core::Failure> {
            for chunk in out.chunks_mut(Digest::LENGTH) {
                let number = self.0.fetch_add(1, Ordering::SeqCst);
                let block = Digest::of(&number.to_le_bytes());
                chunk.copy_from_slice(&block.as_bytes()[..chunk.len()]);
            }
            Ok(())
        }
    }

    fn counted() -> Arc<Counted> {
        Arc::new(Counted(AtomicU64::new(0)))
    }

    fn user_id() -> PrincipalId {
        PrincipalId::from_bytes([1; 16])
    }

    fn workspace() -> WorkspaceId {
        WorkspaceId::from_bytes([2; 16])
    }

    fn local() -> BuiltIn {
        BuiltIn::local(user_id(), workspace())
    }

    fn user(text: &str) -> Settings {
        Settings::load(&Sources {
            user: Some(SourceFile::new("settings.toml", text)),
            ..Sources::none()
        })
        .unwrap()
    }

    /// The policy engine opened as the kernel opens it, over a store of its
    /// own, with its clock.
    async fn opened(text: &str) -> (Store, Policy, ApprovalSurface, Arc<ManualClock>) {
        let clock = Arc::new(ManualClock::starting_at(Timestamp::from_unix_millis(START)));
        let store = Store::open(
            StoreConfig::in_memory()
                .with_clock(Arc::clone(&clock) as Arc<dyn a2m_telemetry::Clock>)
                .with_random(counted()),
        )
        .await
        .unwrap();
        let vault = Arc::new(Vault::with_random(
            Arc::new(MemoryKeyStore::with_random(counted())) as Arc<dyn a2m_vault::KeyStore>,
            Arc::new(store.key_records()),
            counted(),
        ));
        store.attach(vault).await.unwrap();
        let (built_in, refused) = carried(&user(text), local());
        assert!(refused.is_empty(), "{refused:?}");
        let (policy, surface) = Policy::open(&store, built_in).await.unwrap();
        (store, policy, surface, clock)
    }

    fn tool() -> Resource {
        Resource::Tool(ToolRef {
            target: TargetName::new("example").unwrap(),
            module: ModuleName::new("orders").unwrap(),
            tool: OperationName::new("example_order_update").unwrap(),
        })
    }

    fn ask(question: Question, effect: EffectClass, agent: bool) -> Request {
        let mut context = Context::plain(workspace(), Environment::new("test").unwrap());
        context.effect = effect;
        context.records = Some(1);
        context.caller_is_agent = agent;
        Request {
            principal: PrincipalRef::person(user_id()),
            question,
            resource: tool(),
            context,
            run: Some(a2m_core::RunId::from_bytes([3; 16])),
            diff_hash: Some(Digest::of(b"diff")),
        }
    }

    /// A write run of `effect` waiting at `needs_approval`.
    async fn waiting(store: &Store, effect: EffectClass) -> Fence {
        let new = NewRun {
            workspace: workspace(),
            principal: user_id(),
            client_label: "test".to_owned(),
            tool_name: OperationName::new("example_order_update").unwrap(),
            tool_kind: ToolKind::Operation,
            target: TargetName::new("example").unwrap(),
            target_version: Digest::of(b"plan"),
            environment: Environment::new("test").unwrap(),
            identity: None,
            effect,
            input_hash: Digest::of(b"input"),
            input: None,
            retention: RetentionClass::Long,
        };
        store
            .write(Durability::Synced, move |tx| {
                let Opened::New(fence) = tx.ledger().open_run(new)? else {
                    panic!("the run exists already")
                };
                tx.ledger()
                    .set_state(&fence, a2m_core::RunState::Validating, None)?;
                tx.ledger().store_diff(&fence, Digest::of(b"diff"), None)?;
                tx.ledger()
                    .set_state(&fence, a2m_core::RunState::NeedsApproval, None)?;
                Ok(fence)
            })
            .await
            .unwrap()
    }

    async fn approve(surface: &ApprovalSurface, fence: Fence, at: i64) {
        surface
            .approve(Approved {
                run: fence.run,
                diff_hash: Digest::of(b"diff"),
                tool_version: Digest::of(b"plan"),
                approver: PrincipalRef::person(user_id()),
                channel: Channel::LocalPrompt,
                presence: PresenceCheck::OsPrompt,
                proved_at: Timestamp::from_unix_millis(at),
                prompt: None,
            })
            .await
            .unwrap();
    }

    async fn consume(
        store: &Store,
        policy: &Policy,
        fence: Fence,
    ) -> Result<(), a2m_core::Failure> {
        let policy = policy.clone();
        store
            .write(Durability::Synced, move |tx| {
                let clearance = policy.consume(tx, fence.run, &Digest::of(b"diff"))?;
                tx.ledger().begin_writes(&fence, clearance)
            })
            .await
    }

    #[test]
    fn the_defaults_are_carried_as_the_built_in_sets_own() {
        for built_in in [local(), BuiltIn::server()] {
            let before = built_in.layers().settings.clone();
            let (carried, refused) = carried(&Settings::defaults(), built_in);
            assert!(refused.is_empty());
            assert_eq!(carried.layers().settings, before);
        }
    }

    #[tokio::test]
    async fn max_records_per_run_is_kept_by_the_policy_engine() {
        for (text, refused) in [("", false), ("[policy]\nmax_records_per_run = 5\n", true)] {
            let (store, policy, _surface, _) = opened(text).await;
            let checked = store
                .write(Durability::Synced, move |tx| {
                    policy.check_limits(
                        tx,
                        &LimitCheck {
                            principal: PrincipalRef::person(user_id()),
                            workspace: workspace(),
                            records: Some(6),
                            count_call: false,
                        },
                    )
                })
                .await;
            assert_eq!(checked.is_err(), refused, "{text}");
        }
    }

    #[tokio::test]
    async fn approval_window_minutes_is_kept_by_the_policy_engine() {
        for (text, refused) in [
            ("", false),
            ("[policy]\napproval_window_minutes = 5\n", true),
        ] {
            let (store, policy, surface, clock) = opened(text).await;
            let fence = waiting(&store, EffectClass::Write).await;
            approve(&surface, fence, START).await;
            clock.advance(Duration::from_secs(6 * 60));
            let used = consume(&store, &policy, fence).await;
            assert_eq!(used.is_err(), refused, "{text}: {used:?}");
        }
    }

    #[tokio::test]
    async fn agent_effect_ceiling_is_kept_by_the_policy_engine() {
        for (text, refused) in [
            ("", false),
            ("[policy]\nagent_effect_ceiling = \"read\"\n", true),
        ] {
            let (_store, policy, _surface, _) = opened(text).await;
            let decision = policy
                .decide_alone(ask(Question::UseTool, EffectClass::Write, true))
                .await;
            assert_eq!(
                matches!(decision.outcome(), Outcome::Refused(_)),
                refused,
                "{text}"
            );
        }
    }

    #[tokio::test]
    async fn writes_outside_tests_is_kept_by_the_policy_engine() {
        for (text, refused) in [
            ("", false),
            ("[policy]\nwrites_outside_tests = false\n", true),
        ] {
            let (_store, policy, _surface, _) = opened(text).await;
            let decision = policy
                .decide_alone(ask(Question::WriteWithoutPerson, EffectClass::Write, false))
                .await;
            // The local set has no pre-approval rule: the write waits for a
            // person, unless the setting refuses it outright.
            assert_eq!(
                matches!(decision.outcome(), Outcome::Refused(_)),
                refused,
                "{text}"
            );
        }
    }

    #[tokio::test]
    async fn two_person_irreversible_is_kept_by_the_policy_engine() {
        for (text, refused) in [
            ("", false),
            ("[policy]\ntwo_person_irreversible = true\n", true),
        ] {
            let (store, policy, surface, _) = opened(text).await;
            let fence = waiting(&store, EffectClass::Irreversible).await;
            approve(&surface, fence, START).await;
            let used = consume(&store, &policy, fence).await;
            assert_eq!(used.is_err(), refused, "{text}: {used:?}");
        }
    }

    #[tokio::test]
    async fn pre_approval_is_kept_by_the_policy_engine() {
        for (text, off) in [("", false), ("[policy]\npre_approval = false\n", true)] {
            let (_store, policy, _surface, _) = opened(text).await;
            let decision = policy
                .decide_alone(ask(Question::WriteWithoutPerson, EffectClass::Write, false))
                .await;
            let named = decision
                .rules
                .iter()
                .any(|rule| rule.as_str() == "limit.no_pre_approval");
            assert_eq!(named, off, "{text}");
        }
    }

    #[test]
    fn an_approver_the_policy_engine_has_not_is_refused_by_name() {
        // A user file can only tighten it; `caller_confirms`, the loosest,
        // is refused by the settings loader before it gets here. On one
        // machine no approver can be named.
        let (_, refused) = carried(
            &user("[policy]\napprove_writes = \"named_approver\"\n"),
            local(),
        );
        assert_eq!(refused.len(), 1);
        assert_eq!(refused[0].key, "policy.approve_writes");
        let failure = &refused[0].failure;
        assert_eq!(failure.code(), &codes::SETTING_NOT_IN_FORCE);
        assert_eq!(failure.class(), FailureClass::Policy);
        assert!(failure.message().contains("settings.toml"), "{failure:?}");
        // A named approver is carried on a server.
        let (server, refused) = carried(
            &user("[policy]\napprove_writes = \"named_approver\"\n"),
            BuiltIn::server(),
        );
        assert!(refused.is_empty());
        assert_eq!(
            server.layers().settings.approve_writes,
            a2m_policy::ApproveWrites::NamedApprover
        );
    }

    #[test]
    fn the_presets_are_carried_and_stop_nothing() {
        for preset in PRESETS {
            let chosen = user(&format!("[profile]\npreset = \"{}\"\n", preset.name));
            let (_, refused) = carried(&chosen, local());
            assert!(refused.is_empty(), "{}: {refused:?}", preset.name);
        }
        let (cautious, _) = carried(&user("[profile]\npreset = \"cautious\"\n"), local());
        assert_eq!(
            cautious.layers().settings.agent_effect_ceiling,
            EffectClass::Read
        );
        let (ci, _) = carried(&user("[profile]\npreset = \"ci\"\n"), local());
        assert!(!ci.layers().settings.writes_outside_tests);
    }
}
