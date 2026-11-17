//! The decision core: guardrails, permits, and the two semantics that join
//! them. A request is denied unless a permit applies, and one matching
//! forbid denies it whatever permits apply.
//!
//! This is the crate's own evaluator (decision 0082). Every rule is typed
//! data or a function over typed facts, so the only thing that can fail
//! while deciding is a pre-approval rule's condition, and that failure is a
//! denial.

use a2m_core::{
    CanonicalJson, Digest, EffectClass, Environment, Failure, Hazard, Identity, OperationName,
    PrincipalId, RunId, TargetName, Value, WorkspaceId,
};
use a2m_expr::Bindings;
use serde::Serialize;

use crate::failure;
use crate::model::{
    ActingFor, ActionName, ActionRef, Context, IdentityMode, ModuleName, PrincipalKind,
    PrincipalRef, Question, Request, Resource, Role, RunRef, ToolRef, word,
};
use crate::set::{ApproveWrites, Compiled, PolicyTest, Settings};

word! {
    /// The answer to a question: a plain allow or deny.
    Answer {
        /// It may happen.
        Allow => "allow",
        /// It may not.
        Deny => "deny",
    }
}

/// Raised whenever the logic of a guardrail changes, so that the guardrails'
/// hash, and with it every set's version, changes too.
const GUARDRAILS_REVISION: u32 = 2;

/// A built-in forbid. No policy set can remove one: they are not in a set's
/// data, they are consulted before any permit, and their hash is part of
/// every version.
pub(crate) struct Guardrail {
    pub(crate) id: &'static str,
    pub(crate) says: &'static str,
    applies: fn(&Settings, &Request) -> bool,
    refusal: fn() -> Option<Failure>,
}

fn caller_is_not_a_person(request: &Request) -> bool {
    request.principal.kind != PrincipalKind::Person || request.context.caller_is_agent
}

/// Whether the caller is an agent: an agent identity, or any principal a
/// front door says an agent calls as.
fn caller_is_an_agent(request: &Request) -> bool {
    request.principal.kind == PrincipalKind::Agent || request.context.caller_is_agent
}

/// Whether the request is about a write: a tool above `read`, the question
/// whether it may proceed without a person, or a person's approval of it.
fn about_a_write(request: &Request) -> bool {
    request.context.effect != EffectClass::Read
        && matches!(
            request.question,
            Question::UseTool | Question::WriteWithoutPerson | Question::Approve
        )
}

/// Whether the principal is `person`, or was proved to act for them.
fn is_or_acts_for(request: &Request, person: &PrincipalId) -> bool {
    let acts_for = request
        .context
        .acting_for
        .as_ref()
        .and_then(ActingFor::proven_person);
    match request.principal.kind {
        PrincipalKind::Person => request.principal.id == *person,
        // An agent with its own identity, or a program, needs a proved link.
        PrincipalKind::Agent | PrincipalKind::Program => acts_for == Some(person),
    }
}

pub(crate) const GUARDRAILS: &[Guardrail] = &[
    Guardrail {
        id: "guardrail.only_a_person_approves",
        says: "Only a person can approve a run. An agent or a program never can, whatever role it was given.",
        applies: |_, request| {
            request.question == Question::Approve && caller_is_not_a_person(request)
        },
        refusal: || Some(failure::approval_refused()),
    },
    Guardrail {
        id: "guardrail.named_approver_is_not_the_caller",
        says: "Where the policy set names an approver, the person whose call started the run cannot approve it.",
        applies: |settings, request| {
            request.question == Question::Approve
                && settings.approve_writes == ApproveWrites::NamedApprover
                && matches!(&request.resource, Resource::Run(run) if run.started_by == request.principal.id)
        },
        refusal: || Some(failure::approval_refused()),
    },
    Guardrail {
        id: "guardrail.irreversible_needs_a_person",
        says: "An irreversible operation never runs without a person's approval of that run, so no pre-approval rule or standing approval covers it.",
        applies: |_, request| {
            request.question == Question::WriteWithoutPerson
                && request.context.effect == EffectClass::Irreversible
        },
        refusal: || None,
    },
    Guardrail {
        id: "guardrail.hazard_needs_a_person",
        says: "An operation with any hazard other than `none` needs a person's approval of every run, so no pre-approval rule or standing approval covers it.",
        applies: |_, request| {
            request.question == Question::WriteWithoutPerson
                && request.context.hazard != Hazard::None
        },
        refusal: || None,
    },
    Guardrail {
        id: "guardrail.a_session_belongs_to_its_owner",
        says: "A session signed in as a person is used only by that person or an agent proved to act for them. No role, including administrator, runs as someone else.",
        applies: |_, request| {
            request.question == Question::ActAs
                && matches!(&request.resource, Resource::Identity(Identity::Person(owner)) if !is_or_acts_for(request, owner))
        },
        refusal: || Some(failure::identity_not_allowed()),
    },
    // The forbids below follow the settings in force, so a set, or the
    // installation's settings, can only switch them on (decision 0145).
    Guardrail {
        id: "limit.agent_effect_ceiling",
        says: "When the caller is an agent, a tool whose effect class is above the setting `agent_effect_ceiling` is not used, whatever a grant gives.",
        applies: |settings, request| {
            matches!(
                request.question,
                Question::UseTool | Question::WriteWithoutPerson
            ) && caller_is_an_agent(request)
                && request.context.effect > settings.agent_effect_ceiling
        },
        refusal: || Some(failure::effect_too_high()),
    },
    Guardrail {
        id: "limit.writes_only_to_test_records",
        says: "With the setting `writes_outside_tests` off, a write proceeds, and is approved, only when every record it writes is a declared test record of a test environment.",
        applies: |settings, request| {
            !settings.writes_outside_tests
                && request.question != Question::UseTool
                && about_a_write(request)
                && !request.context.test_records
        },
        refusal: || Some(failure::not_granted()),
    },
    Guardrail {
        id: "limit.no_pre_approval",
        says: "With the setting `pre_approval` off, no pre-approval rule lets a write proceed without a person: every write waits for one.",
        applies: |settings, request| {
            !settings.pre_approval && request.question == Question::WriteWithoutPerson
        },
        refusal: || None,
    },
];

/// The hash of the guardrails compiled into this binary.
pub(crate) fn guardrails_digest() -> Digest {
    #[derive(Serialize)]
    struct Text<'a> {
        revision: u32,
        rules: Vec<(&'a str, &'a str)>,
    }
    let text = Text {
        revision: GUARDRAILS_REVISION,
        rules: GUARDRAILS.iter().map(|g| (g.id, g.says)).collect(),
    };
    // The text is static and holds no float, so it always encodes; if it ever
    // did not, the digest of nothing would still change every version.
    CanonicalJson::encode(&text).map_or_else(|_| Digest::of(b""), |json| json.digest())
}

/// The rules that are not a set's own data, with what each says, for `explain`.
pub(crate) const BASE_RULES: &[(&str, &str)] = &[
    (
        DEFAULT_DENY,
        "Nothing is allowed unless a rule permits it, and no rule permitted this.",
    ),
    (
        EVALUATION_ERROR,
        "The policy engine could not evaluate a rule, or no valid policy set was loaded, so the answer is no.",
    ),
    (
        KILL_SWITCH,
        "A kill switch covered the request. Kill switches are checked before any rule.",
    ),
    (ROLE_APPROVER, "The principal holds the approver role."),
    (
        ROLE_ACTION,
        "The principal holds the role the action needs.",
    ),
];

pub(crate) const DEFAULT_DENY: &str = "default.deny";
pub(crate) const EVALUATION_ERROR: &str = "engine.evaluation_failed";
pub(crate) const KILL_SWITCH: &str = "limiter.kill_switch";
const ROLE_APPROVER: &str = "role.approver";
const ROLE_ACTION: &str = "role.action";

/// What the decision core found, before it is recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Verdict {
    pub(crate) answer: Answer,
    /// The rules that determined the answer, in the set's order.
    pub(crate) rules: Vec<String>,
    /// What a refused caller is told. `None` on allow, and on the denial of
    /// `WriteWithoutPerson`, which is not a refusal: the run waits for a person.
    pub(crate) refusal: Option<Failure>,
}

impl Verdict {
    pub(crate) fn deny(rule: &str, refusal: Failure) -> Self {
        Self {
            answer: Answer::Deny,
            rules: vec![rule.to_owned()],
            refusal: Some(refusal),
        }
    }
}

/// Decides one request against one set. Pure: the same set and the same
/// request give the same verdict.
pub(crate) fn evaluate(set: &Compiled, request: &Request) -> Verdict {
    let settings = &set.layers.settings;
    let forbids: Vec<&Guardrail> = GUARDRAILS
        .iter()
        .filter(|guardrail| (guardrail.applies)(settings, request))
        .collect();
    if let Some(first) = forbids.first() {
        return Verdict {
            answer: Answer::Deny,
            rules: forbids.iter().map(|g| g.id.to_owned()).collect(),
            refusal: (first.refusal)(),
        };
    }
    match permits(set, request) {
        Err(_) => Verdict::deny(EVALUATION_ERROR, failure::evaluation_failed()),
        Ok(Permits { rules, .. }) if !rules.is_empty() => Verdict {
            answer: Answer::Allow,
            rules,
            refusal: None,
        },
        Ok(Permits { effect_too_low, .. }) => Verdict {
            answer: Answer::Deny,
            rules: vec![DEFAULT_DENY.to_owned()],
            refusal: match request.question {
                Question::WriteWithoutPerson => None,
                Question::ActAs => Some(failure::identity_not_allowed()),
                Question::Approve => Some(failure::approval_refused()),
                Question::UseTool if effect_too_low => Some(failure::effect_too_high()),
                Question::UseTool
                | Question::SeeLabel
                | Question::SetOption
                | Question::PerformAction => Some(failure::not_granted()),
            },
        },
    }
}

#[derive(Default)]
struct Permits {
    rules: Vec<String>,
    /// A grant covered the caller and the tool, but not at this effect class.
    effect_too_low: bool,
}

fn holds(set: &Compiled, principal: &PrincipalRef, role: Role) -> bool {
    set.layers
        .roles
        .iter()
        .any(|binding| binding.role == role && binding.who.covers(principal))
}

fn permits(set: &Compiled, request: &Request) -> Result<Permits, Failure> {
    let layers = &set.layers;
    let principal = &request.principal;
    let context = &request.context;
    let mut found = Permits::default();
    match (request.question, &request.resource) {
        (Question::UseTool, Resource::Tool(tool)) => {
            for (index, grant) in layers.grants.iter().enumerate() {
                if grant.who.covers(principal) && grant.scope.covers(tool) {
                    if grant.max_effect >= context.effect {
                        found.rules.push(format!("grant.{index}"));
                    } else {
                        found.effect_too_low = true;
                    }
                }
            }
        }
        (Question::ActAs, Resource::Identity(identity)) => {
            let fits = |mode: IdentityMode| match identity {
                Identity::Person(_) => mode != IdentityMode::ServiceAccount,
                Identity::Account(_) => mode == IdentityMode::ServiceAccount,
            };
            for (index, grant) in layers.grants.iter().enumerate() {
                if grant.who.covers(principal)
                    && context.identity_mode == Some(grant.identity_mode)
                    && fits(grant.identity_mode)
                {
                    found.rules.push(format!("grant.{index}"));
                }
            }
        }
        // A write is let through without a person only for one run and one
        // diff: the decision is bound to both, and the ledger checks them.
        (Question::WriteWithoutPerson, Resource::Tool(tool))
            if request.run.is_some() && request.diff_hash.is_some() =>
        {
            for (index, rule) in layers.pre_approvals.iter().enumerate() {
                let within = context
                    .records
                    .is_some_and(|records| records <= rule.max_records);
                if !(rule.who.covers(principal) && rule.scope.covers(tool) && within) {
                    continue;
                }
                let holds = match set.conditions.get(index) {
                    Some(None) => true,
                    Some(Some(program)) => condition_holds(program, tool, context)?,
                    None => return Err(failure::evaluation_failed()),
                };
                if holds {
                    found.rules.push(format!("pre_approval.{}", rule.name));
                }
            }
        }
        (Question::Approve, Resource::Run(_)) if holds(set, principal, Role::Approver) => {
            found.rules.push(ROLE_APPROVER.to_owned());
        }
        (Question::SeeLabel, Resource::Label(label)) => {
            for (index, grant) in layers.labels.iter().enumerate() {
                if grant.who.covers(principal) && *label <= grant.up_to {
                    found.rules.push(format!("label.{index}"));
                }
            }
        }
        (Question::SetOption, Resource::Option(option)) => {
            for (index, grant) in layers.options.iter().enumerate() {
                if grant.who.covers(principal)
                    && grant.option.as_ref().is_none_or(|named| named == option)
                {
                    found.rules.push(format!("option.{index}"));
                }
            }
        }
        (Question::PerformAction, Resource::Action(action))
            if holds(set, principal, action.needs) =>
        {
            found.rules.push(ROLE_ACTION.to_owned());
        }
        // A question about the wrong kind of resource has no permit.
        _ => {}
    }
    Ok(found)
}

fn condition_holds(
    program: &a2m_expr::Program,
    tool: &ToolRef,
    context: &Context,
) -> Result<bool, Failure> {
    let text = |text: &str| Value::Text(text.to_owned());
    let effect = text(context.effect.as_str());
    let hazard = text(context.hazard.as_str());
    let records = Value::Int(i64::from(context.records.unwrap_or(0)));
    let environment = text(context.environment.as_str());
    let toolset = text(context.toolset.as_ref().map_or("", |name| name.as_str()));
    let target = text(tool.target.as_str());
    let module = text(tool.module.as_str());
    let name = text(tool.tool.as_str());
    let agent = Value::Bool(context.caller_is_agent);
    let bindings = Bindings::new()
        .with("effect", &effect)
        .with("hazard", &hazard)
        .with("records", &records)
        .with("environment", &environment)
        .with("toolset", &toolset)
        .with("target", &target)
        .with("module", &module)
        .with("tool", &name)
        .with("caller_is_agent", &agent);
    match program.evaluate(&bindings)? {
        Value::Bool(holds) => Ok(holds),
        _ => Err(failure::evaluation_failed()),
    }
}

/// What a rule says, in words, for `explain`.
pub(crate) fn describe(set: Option<&Compiled>, rule: &str) -> String {
    if let Some(guardrail) = GUARDRAILS.iter().find(|g| g.id == rule) {
        return guardrail.says.to_owned();
    }
    if let Some((_, says)) = BASE_RULES.iter().find(|(id, _)| *id == rule) {
        return (*says).to_owned();
    }
    let from_set = set.and_then(|set| {
        let (kind, rest) = rule.split_once('.')?;
        let layers = &set.layers;
        match kind {
            "grant" => {
                let grant = layers.grants.get(rest.parse::<usize>().ok()?)?;
                Some(format!(
                    "A grant covers this principal and this tool, up to the effect class `{}`, with the identity mode `{}`.",
                    grant.max_effect, grant.identity_mode
                ))
            }
            "label" => {
                let grant = layers.labels.get(rest.parse::<usize>().ok()?)?;
                Some(format!(
                    "A rule lets this principal see fields labelled up to `{}`.",
                    grant.up_to
                ))
            }
            "option" => Some("A rule lets this principal set this per-call option.".to_owned()),
            "pre_approval" => {
                let found = layers.pre_approvals.iter().find(|r| r.name == rest)?;
                Some(format!(
                    "The pre-approval rule `{}` lets a write of at most {} records proceed without a person.",
                    found.name, found.max_records
                ))
            }
            _ => None,
        }
    });
    from_set.unwrap_or_else(|| "A rule of the policy version that made the decision.".to_owned())
}

/// The test table of the built-in set of one machine.
pub(crate) fn local_tests(user: PrincipalId, workspace: WorkspaceId) -> Vec<PolicyTest> {
    // Fixed values: a test table is hashed, so nothing in it may vary.
    let stranger = PrincipalId::from_bytes([0xee; 16]);
    let names = (|| {
        Some((
            TargetName::new("example").ok()?,
            ModuleName::new("orders").ok()?,
            OperationName::new("example_orders_order_update").ok()?,
            Environment::new("test").ok()?,
            ActionName::new("grant.add").ok()?,
        ))
    })();
    let Some((target, module, tool, environment, action)) = names else {
        return Vec::new();
    };
    let tool = Resource::Tool(ToolRef {
        target,
        module,
        tool,
    });
    let context = |effect: EffectClass| Context {
        effect,
        records: Some(1),
        caller_is_agent: true,
        acting_for: Some(ActingFor::LocalUser(user)),
        ..Context::plain(workspace, environment.clone())
    };
    let run = RunRef {
        run: RunId::from_bytes([0x11; 16]),
        started_by: user,
    };
    let test = |name: &str,
                principal: PrincipalRef,
                question: Question,
                resource: Resource,
                context: Context,
                expect: Answer| PolicyTest {
        name: name.to_owned(),
        request: Request {
            principal,
            question,
            resource,
            context,
            run: None,
            diff_hash: None,
        },
        expect,
    };
    vec![
        test(
            "the user may call a tool that writes",
            PrincipalRef::person(user),
            Question::UseTool,
            tool.clone(),
            context(EffectClass::Write),
            Answer::Allow,
        ),
        test(
            "nobody else may call a tool",
            PrincipalRef::person(stranger),
            Question::UseTool,
            tool.clone(),
            context(EffectClass::Read),
            Answer::Deny,
        ),
        test(
            "a write waits for a person",
            PrincipalRef::person(user),
            Question::WriteWithoutPerson,
            tool,
            context(EffectClass::Write),
            Answer::Deny,
        ),
        test(
            "the user, present, may approve their run",
            PrincipalRef::person(user),
            Question::Approve,
            Resource::Run(run.clone()),
            Context::plain(workspace, environment.clone()),
            Answer::Allow,
        ),
        test(
            "the user's agent may not approve",
            PrincipalRef::person(user),
            Question::Approve,
            Resource::Run(run),
            context(EffectClass::Write),
            Answer::Deny,
        ),
        test(
            "the user may perform an admin action",
            PrincipalRef::person(user),
            Question::PerformAction,
            Resource::Action(ActionRef {
                name: action,
                needs: Role::Admin,
            }),
            Context::plain(workspace, environment),
            Answer::Allow,
        ),
    ]
}

#[cfg(test)]
mod tests {
    use a2m_core::{
        AccountName, Digest, EffectClass, Environment, Hazard, Identity, OperationName,
        PrincipalId, RunId, Sensitivity, TargetName, WorkspaceId,
    };

    use super::{Answer, GUARDRAILS, evaluate, guardrails_digest};
    use crate::failure::{
        APPROVAL_REFUSED, EFFECT_TOO_HIGH, IDENTITY_NOT_ALLOWED, NOT_GRANTED,
        POLICY_EVALUATION_FAILED,
    };
    use crate::model::{
        ActingFor, ActionName, ActionRef, Context, IdentityMode, ModuleName, OptionName,
        PrincipalKind, PrincipalRef, Question, Request, Resource, Role, RunRef, ToolRef,
    };
    use crate::set::{
        ApproveWrites, BuiltIn, CandidateSet, Grant, LabelGrant, OptionGrant, PolicyLayers,
        PolicyTest, PreApproval, Profile, RoleBinding, Scope, Settings, Subject,
    };

    fn id(byte: u8) -> PrincipalId {
        PrincipalId::from_bytes([byte; 16])
    }

    fn workspace() -> WorkspaceId {
        WorkspaceId::from_bytes([7; 16])
    }

    fn tool(name: &str) -> Resource {
        Resource::Tool(ToolRef {
            target: TargetName::new("example").unwrap(),
            module: ModuleName::new("orders").unwrap(),
            tool: OperationName::new(name).unwrap(),
        })
    }

    fn context(effect: EffectClass) -> Context {
        Context {
            effect,
            records: Some(3),
            ..Context::plain(workspace(), Environment::new("test").unwrap())
        }
    }

    fn ask(
        principal: PrincipalRef,
        question: Question,
        resource: Resource,
        context: Context,
    ) -> Request {
        Request {
            principal,
            question,
            resource,
            context,
            run: Some(RunId::from_bytes([3; 16])),
            diff_hash: Some(Digest::of(b"diff")),
        }
    }

    fn local() -> CandidateSet {
        let built_in = BuiltIn::local(id(1), workspace());
        CandidateSet::compile(built_in.layers(), Profile::Local).unwrap()
    }

    /// The loosest set anyone could write: everyone holds every role and a
    /// grant on everything, and every write is pre-approved.
    fn loosest() -> CandidateSet {
        let layers = PolicyLayers {
            roles: Role::ALL
                .iter()
                .map(|role| RoleBinding {
                    who: Subject::Anyone,
                    role: *role,
                })
                .collect(),
            grants: IdentityMode::ALL
                .iter()
                .map(|mode| Grant {
                    who: Subject::Anyone,
                    scope: Scope::Pattern("*".to_owned()),
                    max_effect: EffectClass::Irreversible,
                    identity_mode: *mode,
                })
                .collect(),
            labels: vec![LabelGrant {
                who: Subject::Anyone,
                up_to: Sensitivity::Restricted,
            }],
            options: vec![OptionGrant {
                who: Subject::Anyone,
                option: None,
            }],
            pre_approvals: vec![PreApproval {
                name: "everything".to_owned(),
                who: Subject::Anyone,
                scope: Scope::Pattern("*".to_owned()),
                max_records: 1_000_000,
                condition: Some("true".to_owned()),
            }],
            quotas: Vec::new(),
            settings: Settings {
                approve_writes: ApproveWrites::NamedApprover,
                max_records_per_run: 1_000_000,
                approval_window_minutes: Settings::LONGEST_WINDOW_MINUTES,
                agent_effect_ceiling: EffectClass::Irreversible,
                writes_outside_tests: true,
                two_person_irreversible: false,
                pre_approval: true,
            },
            tests: Vec::new(),
        };
        CandidateSet::compile(&layers, Profile::Server).unwrap()
    }

    #[test]
    fn with_no_permit_the_answer_is_deny() {
        let empty = CandidateSet::compile(&PolicyLayers::default(), Profile::Server).unwrap();
        let person = PrincipalRef::person(id(1));
        let questions = [
            (Question::UseTool, tool("example_orders_order_get")),
            (
                Question::WriteWithoutPerson,
                tool("example_orders_order_update"),
            ),
            (Question::ActAs, Resource::Identity(Identity::Person(id(1)))),
            (
                Question::Approve,
                Resource::Run(RunRef {
                    run: RunId::from_bytes([3; 16]),
                    started_by: id(2),
                }),
            ),
            (Question::SeeLabel, Resource::Label(Sensitivity::Public)),
            (
                Question::SetOption,
                Resource::Option(OptionName::new("pin_mode").unwrap()),
            ),
            (
                Question::PerformAction,
                Resource::Action(ActionRef {
                    name: ActionName::new("grant.add").unwrap(),
                    needs: Role::Viewer,
                }),
            ),
        ];
        for (question, resource) in questions {
            let mut context = context(EffectClass::Read);
            context.identity_mode = Some(IdentityMode::PerUserSession);
            let verdict = evaluate(
                &empty.compiled,
                &ask(person.clone(), question, resource, context),
            );
            assert_eq!(verdict.answer, Answer::Deny, "{question}");
            assert_eq!(verdict.rules, ["default.deny"], "{question}");
        }
        // A question about the wrong kind of resource has no permit, even
        // under the loosest set.
        let wrong = ask(
            person,
            Question::UseTool,
            Resource::Label(Sensitivity::Public),
            context(EffectClass::Read),
        );
        assert_eq!(evaluate(&loosest().compiled, &wrong).answer, Answer::Deny);
    }

    #[test]
    fn the_local_set_lets_its_user_work_and_makes_writes_wait_for_a_person() {
        let set = local();
        assert!(set.test().ok(), "{:?}", set.test());
        assert_eq!(set.test().passed, 6);
        let user = PrincipalRef::person(id(1));

        let call = ask(
            user.clone(),
            Question::UseTool,
            tool("example_orders_order_update"),
            context(EffectClass::Write),
        );
        assert_eq!(evaluate(&set.compiled, &call).answer, Answer::Allow);

        let write = ask(
            user.clone(),
            Question::WriteWithoutPerson,
            tool("example_orders_order_update"),
            context(EffectClass::Write),
        );
        let verdict = evaluate(&set.compiled, &write);
        assert_eq!(verdict.answer, Answer::Deny);
        assert_eq!(verdict.refusal, None, "a write that waits is not a refusal");

        let stranger = ask(
            PrincipalRef::person(id(2)),
            Question::UseTool,
            tool("example_orders_order_get"),
            context(EffectClass::Read),
        );
        let verdict = evaluate(&set.compiled, &stranger);
        assert_eq!(verdict.answer, Answer::Deny);
        assert_eq!(verdict.refusal.unwrap().code(), &NOT_GRANTED);
    }

    #[test]
    fn a_grant_below_the_effect_class_says_so() {
        let layers = PolicyLayers {
            grants: vec![Grant {
                who: Subject::Principal(id(1)),
                scope: Scope::Module(
                    TargetName::new("example").unwrap(),
                    ModuleName::new("orders").unwrap(),
                ),
                max_effect: EffectClass::Read,
                identity_mode: IdentityMode::ServiceAccount,
            }],
            ..PolicyLayers::default()
        };
        let set = CandidateSet::compile(&layers, Profile::Server).unwrap();
        let user = PrincipalRef::person(id(1));
        let read = ask(
            user.clone(),
            Question::UseTool,
            tool("example_orders_order_get"),
            context(EffectClass::Read),
        );
        assert_eq!(evaluate(&set.compiled, &read).rules, ["grant.0"]);
        let write = ask(
            user,
            Question::UseTool,
            tool("example_orders_order_update"),
            context(EffectClass::Write),
        );
        assert_eq!(
            evaluate(&set.compiled, &write).refusal.unwrap().code(),
            &EFFECT_TOO_HIGH
        );
    }

    #[test]
    fn a_pre_approval_stands_in_only_within_its_conditions() {
        let mut layers = BuiltIn::local(id(1), workspace()).layers().clone();
        layers.pre_approvals.push(PreApproval {
            name: "small_test_updates".to_owned(),
            who: Subject::Principal(id(1)),
            scope: Scope::Pattern("example_orders_*".to_owned()),
            max_records: 5,
            condition: Some("environment == \"test\" && records <= 3".to_owned()),
        });
        layers.tests.clear();
        let set = CandidateSet::compile(&layers, Profile::Local).unwrap();
        let user = PrincipalRef::person(id(1));
        let write = |records: u32, effect: EffectClass, hazard: Hazard| {
            let mut context = context(effect);
            context.records = Some(records);
            context.hazard = hazard;
            ask(
                user.clone(),
                Question::WriteWithoutPerson,
                tool("example_orders_order_update"),
                context,
            )
        };
        let allowed = evaluate(&set.compiled, &write(3, EffectClass::Write, Hazard::None));
        assert_eq!(allowed.answer, Answer::Allow);
        assert_eq!(allowed.rules, ["pre_approval.small_test_updates"]);
        // Over the condition, over the effect class, or with a hazard: a person.
        assert_eq!(
            evaluate(&set.compiled, &write(4, EffectClass::Write, Hazard::None)).answer,
            Answer::Deny
        );
        assert_eq!(
            evaluate(
                &set.compiled,
                &write(3, EffectClass::Irreversible, Hazard::None)
            )
            .answer,
            Answer::Deny
        );
        assert_eq!(
            evaluate(
                &set.compiled,
                &write(3, EffectClass::Write, Hazard::SendsOutside)
            )
            .answer,
            Answer::Deny
        );
        // Bound to a run and a diff, or it does not stand in.
        let mut unbound = write(3, EffectClass::Write, Hazard::None);
        unbound.diff_hash = None;
        assert_eq!(evaluate(&set.compiled, &unbound).answer, Answer::Deny);
        // Another tool is outside its scope.
        let other = ask(
            user.clone(),
            Question::WriteWithoutPerson,
            tool("example_stock_item_update"),
            context(EffectClass::Write),
        );
        assert_eq!(evaluate(&set.compiled, &other).answer, Answer::Deny);
    }

    #[test]
    fn no_policy_input_loosens_a_guardrail() {
        let set = loosest();
        let person = PrincipalRef::person(id(1));
        let run = Resource::Run(RunRef {
            run: RunId::from_bytes([3; 16]),
            started_by: id(1),
        });
        let other_run = Resource::Run(RunRef {
            run: RunId::from_bytes([3; 16]),
            started_by: id(2),
        });
        let mut agent_context = context(EffectClass::Write);
        agent_context.caller_is_agent = true;
        let mut session = context(EffectClass::Read);
        session.identity_mode = Some(IdentityMode::PerUserSession);
        let mut delegated = session.clone();
        delegated.acting_for = Some(ActingFor::Delegation(id(2)));
        let mut hazard = context(EffectClass::Write);
        hazard.hazard = Hazard::DataElsewhere;

        // Each row would be allowed by the set's permits; a guardrail denies it.
        let cases = [
            (
                "an agent identity approves",
                ask(
                    PrincipalRef::agent(id(9)),
                    Question::Approve,
                    other_run.clone(),
                    context(EffectClass::Write),
                ),
                "guardrail.only_a_person_approves",
            ),
            (
                "a person's agent approves",
                ask(
                    person.clone(),
                    Question::Approve,
                    other_run.clone(),
                    agent_context,
                ),
                "guardrail.only_a_person_approves",
            ),
            (
                "the caller confirms where an approver is named",
                ask(
                    person.clone(),
                    Question::Approve,
                    run,
                    context(EffectClass::Write),
                ),
                "guardrail.named_approver_is_not_the_caller",
            ),
            (
                "an irreversible write without a person",
                ask(
                    person.clone(),
                    Question::WriteWithoutPerson,
                    tool("example_orders_order_delete"),
                    context(EffectClass::Irreversible),
                ),
                "guardrail.irreversible_needs_a_person",
            ),
            (
                "a hazard without a person",
                ask(
                    person.clone(),
                    Question::WriteWithoutPerson,
                    tool("example_orders_order_update"),
                    hazard,
                ),
                "guardrail.hazard_needs_a_person",
            ),
            (
                "another person's session",
                ask(
                    person.clone(),
                    Question::ActAs,
                    Resource::Identity(Identity::Person(id(2))),
                    session.clone(),
                ),
                "guardrail.a_session_belongs_to_its_owner",
            ),
            (
                "an agent with an unproved link",
                ask(
                    PrincipalRef::agent(id(9)),
                    Question::ActAs,
                    Resource::Identity(Identity::Person(id(2))),
                    delegated,
                ),
                "guardrail.a_session_belongs_to_its_owner",
            ),
        ];
        for (name, request, guardrail) in cases {
            let verdict = evaluate(&set.compiled, &request);
            assert_eq!(verdict.answer, Answer::Deny, "{name}");
            assert!(
                verdict.rules.iter().any(|rule| rule == guardrail),
                "{name}: {:?}",
                verdict.rules
            );
        }
        // The same set does allow what no guardrail covers, so the denials
        // above are the guardrails' and not a missing permit.
        let own = ask(
            person.clone(),
            Question::ActAs,
            Resource::Identity(Identity::Person(id(1))),
            session.clone(),
        );
        assert_eq!(evaluate(&set.compiled, &own).answer, Answer::Allow);
        let approver = ask(
            person.clone(),
            Question::Approve,
            other_run,
            context(EffectClass::Write),
        );
        assert_eq!(evaluate(&set.compiled, &approver).answer, Answer::Allow);
        let plain = ask(
            person,
            Question::WriteWithoutPerson,
            tool("example_orders_order_update"),
            context(EffectClass::Write),
        );
        assert_eq!(evaluate(&set.compiled, &plain).answer, Answer::Allow);
        // Every hazard other than `none`, and nothing in a set can name a guardrail.
        for hazard in Hazard::ALL.iter().filter(|hazard| **hazard != Hazard::None) {
            let mut context = context(EffectClass::Write);
            context.hazard = *hazard;
            let request = ask(
                PrincipalRef::person(id(1)),
                Question::WriteWithoutPerson,
                tool("example_orders_order_update"),
                context,
            );
            assert_eq!(
                evaluate(&set.compiled, &request).answer,
                Answer::Deny,
                "{hazard}"
            );
        }
    }

    #[test]
    fn refusals_carry_a_code_and_never_a_rule() {
        let set = local();
        let agent = ask(
            PrincipalRef::agent(id(9)),
            Question::Approve,
            Resource::Run(RunRef {
                run: RunId::from_bytes([3; 16]),
                started_by: id(1),
            }),
            context(EffectClass::Write),
        );
        let refusal = evaluate(&set.compiled, &agent).refusal.unwrap();
        assert_eq!(refusal.code(), &APPROVAL_REFUSED);
        let mut session = context(EffectClass::Read);
        session.identity_mode = Some(IdentityMode::PerUserSession);
        let other = ask(
            PrincipalRef::person(id(1)),
            Question::ActAs,
            Resource::Identity(Identity::Person(id(2))),
            session.clone(),
        );
        let refusal = evaluate(&set.compiled, &other).refusal.unwrap();
        assert_eq!(refusal.code(), &IDENTITY_NOT_ALLOWED);
        for text in [refusal.message(), refusal.next_step()] {
            assert!(
                !text.contains("guardrail") && !text.contains("grant."),
                "{text}"
            );
        }
        // An account is acted as only through the service-account mode.
        let account = Resource::Identity(Identity::Account(
            AccountName::new("example_service").unwrap(),
        ));
        let as_session = ask(
            PrincipalRef::person(id(1)),
            Question::ActAs,
            account.clone(),
            session,
        );
        assert_eq!(evaluate(&set.compiled, &as_session).answer, Answer::Deny);
        let mut service = context(EffectClass::Read);
        service.identity_mode = Some(IdentityMode::ServiceAccount);
        let as_service = ask(
            PrincipalRef::person(id(1)),
            Question::ActAs,
            account,
            service,
        );
        assert_eq!(evaluate(&set.compiled, &as_service).answer, Answer::Allow);
    }

    #[test]
    fn an_error_while_evaluating_is_a_denial() {
        // A condition that passes the checker and fails when it runs: a
        // number that is not one, parsed at evaluation.
        let mut layers = BuiltIn::local(id(1), workspace()).layers().clone();
        layers.tests.clear();
        layers.pre_approvals.push(PreApproval {
            name: "breaks".to_owned(),
            who: Subject::Anyone,
            scope: Scope::Pattern("*".to_owned()),
            max_records: 5,
            condition: Some("parse_int(tool) > 0".to_owned()),
        });
        let set = CandidateSet::compile(&layers, Profile::Local).unwrap();
        let write = ask(
            PrincipalRef::person(id(1)),
            Question::WriteWithoutPerson,
            tool("example_orders_order_update"),
            context(EffectClass::Write),
        );
        let verdict = evaluate(&set.compiled, &write);
        assert_eq!(verdict.answer, Answer::Deny);
        assert_eq!(verdict.rules, ["engine.evaluation_failed"]);
        assert_eq!(verdict.refusal.unwrap().code(), &POLICY_EVALUATION_FAILED);
    }

    /// A small fixed generator, so the property test is the same on every run.
    struct Fixed(u64);
    impl Fixed {
        fn next(&mut self, below: usize) -> usize {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            usize::try_from(self.0 % (below as u64)).unwrap()
        }
    }

    fn random_request(random: &mut Fixed) -> Request {
        let principal = PrincipalRef {
            id: id(u8::try_from(1 + random.next(3)).unwrap()),
            kind: PrincipalKind::ALL[random.next(3)],
            groups: Vec::new(),
        };
        let resources = [
            tool("example_orders_order_update"),
            tool("example_stock_item_get"),
            Resource::Identity(Identity::Person(id(
                u8::try_from(1 + random.next(3)).unwrap()
            ))),
            Resource::Label(Sensitivity::ALL[random.next(3)]),
            Resource::Run(RunRef {
                run: RunId::from_bytes([3; 16]),
                started_by: id(u8::try_from(1 + random.next(3)).unwrap()),
            }),
            Resource::Option(OptionName::new("pin_mode").unwrap()),
            Resource::Action(ActionRef {
                name: ActionName::new("grant.add").unwrap(),
                needs: Role::ALL[random.next(7)],
            }),
        ];
        let mut context = context(EffectClass::ALL[random.next(3)]);
        context.hazard = Hazard::ALL[random.next(5)];
        context.records = [None, Some(1), Some(60)][random.next(3)];
        context.caller_is_agent = random.next(2) == 1;
        context.identity_mode = [
            None,
            Some(IdentityMode::PerUserSession),
            Some(IdentityMode::ServiceAccount),
        ][random.next(3)];
        context.acting_for = [
            None,
            Some(ActingFor::LocalUser(id(1))),
            Some(ActingFor::Delegation(id(2))),
        ][random.next(3)]
        .clone();
        ask(
            principal,
            Question::ALL[random.next(7)],
            resources[random.next(7)].clone(),
            context,
        )
    }

    #[test]
    fn the_same_request_on_the_same_version_gives_the_same_decision() {
        let mut random = Fixed(0x2545_f491_4f6c_dd1d);
        let (first, again) = (loosest(), loosest());
        assert_eq!(first.version(), again.version());
        let local = local();
        let mut allowed = 0;
        for _ in 0..4000 {
            let request = random_request(&mut random);
            let verdict = evaluate(&first.compiled, &request);
            assert_eq!(verdict, evaluate(&first.compiled, &request));
            assert_eq!(verdict, evaluate(&again.compiled, &request));
            // A forbid always wins: whenever a guardrail applies, the answer
            // is deny, under the loosest set and under the local one.
            let forbidden = GUARDRAILS
                .iter()
                .any(|g| (g.applies)(&first.compiled.layers.settings, &request));
            if forbidden {
                assert_eq!(verdict.answer, Answer::Deny);
            }
            if GUARDRAILS
                .iter()
                .any(|g| (g.applies)(&local.compiled.layers.settings, &request))
            {
                assert_eq!(evaluate(&local.compiled, &request).answer, Answer::Deny);
            }
            allowed += usize::from(verdict.answer == Answer::Allow);
        }
        assert!(
            allowed > 200,
            "the generator must reach permits too: {allowed}"
        );
    }

    #[test]
    fn the_version_covers_the_guardrails_the_layers_and_the_tests() {
        let base = local();
        // The guardrails' hash is pinned: a change to their text or revision
        // changes every version, and must be made on purpose.
        assert_eq!(guardrails_digest(), guardrails_digest());
        assert_eq!(GUARDRAILS.len(), 5);

        let mut layers = base.layers().clone();
        layers.settings.max_records_per_run = 10;
        let tighter = CandidateSet::compile(&layers, Profile::Local).unwrap();
        assert_ne!(tighter.version(), base.version());
        assert!(!tighter.loosens(base.layers()));
        assert!(base.loosens(tighter.layers()));

        let mut layers = base.layers().clone();
        layers.tests.push(PolicyTest {
            name: "one more".to_owned(),
            request: ask(
                PrincipalRef::person(id(1)),
                Question::UseTool,
                tool("example_orders_order_get"),
                context(EffectClass::Read),
            ),
            expect: Answer::Allow,
        });
        let more_tests = CandidateSet::compile(&layers, Profile::Local).unwrap();
        assert_ne!(more_tests.version(), base.version());

        // A stored set reads back as itself, and not under another version.
        let loaded = crate::set::load(
            base.version(),
            &base.layers_text,
            &base.tests_text,
            Profile::Local,
        )
        .unwrap();
        assert_eq!(loaded.version, base.version());
        assert!(
            crate::set::load(
                tighter.version(),
                &base.layers_text,
                &base.tests_text,
                Profile::Local
            )
            .is_err()
        );
    }

    #[test]
    fn a_set_that_says_what_it_may_not_does_not_compile() {
        let base = BuiltIn::local(id(1), workspace()).layers().clone();
        type Change = fn(&mut PolicyLayers);
        let broken: [(&str, Change); 7] = [
            ("no record limit", |l| l.settings.max_records_per_run = 0),
            ("an endless approval window", |l| {
                l.settings.approval_window_minutes = 100_000
            }),
            ("a named approver on one machine", |l| {
                l.settings.approve_writes = ApproveWrites::NamedApprover
            }),
            ("a pattern that is not one", |l| {
                l.grants[0].scope = Scope::Pattern("Example *".to_owned())
            }),
            ("a condition that is not true or false", |l| {
                l.pre_approvals.push(PreApproval {
                    name: "bad".to_owned(),
                    who: Subject::Anyone,
                    scope: Scope::Pattern("*".to_owned()),
                    max_records: 1,
                    condition: Some("records + 1".to_owned()),
                })
            }),
            ("a condition that reads an unknown name", |l| {
                l.pre_approvals.push(PreApproval {
                    name: "bad".to_owned(),
                    who: Subject::Anyone,
                    scope: Scope::Pattern("*".to_owned()),
                    max_records: 1,
                    condition: Some("approved_by_agent == true".to_owned()),
                })
            }),
            ("a pre-approval over the record limit", |l| {
                l.pre_approvals.push(PreApproval {
                    name: "bad".to_owned(),
                    who: Subject::Anyone,
                    scope: Scope::Pattern("*".to_owned()),
                    max_records: 51,
                    condition: None,
                })
            }),
        ];
        for (name, change) in broken {
            let mut layers = base.clone();
            change(&mut layers);
            let failure = CandidateSet::compile(&layers, Profile::Local).unwrap_err();
            assert_eq!(failure.class(), a2m_core::FailureClass::Input, "{name}");
        }
    }

    #[test]
    fn a_failing_test_table_is_reported() {
        let mut layers = BuiltIn::local(id(1), workspace()).layers().clone();
        layers.tests.push(PolicyTest {
            name: "a viewer writes".to_owned(),
            request: ask(
                PrincipalRef::person(id(2)),
                Question::UseTool,
                tool("example_orders_order_update"),
                context(EffectClass::Write),
            ),
            expect: Answer::Allow,
        });
        let report = CandidateSet::compile(&layers, Profile::Local)
            .unwrap()
            .test();
        assert!(!report.ok());
        assert_eq!(report.failed[0].name, "a viewer writes");
        assert_eq!(report.failed[0].got, Answer::Deny);
    }
}
