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
const GUARDRAILS_REVISION: u32 = 1;

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
    (
        ROLE_APPROVER,
        "The principal holds the approver role.",
    ),
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
        (Question::WriteWithoutPerson, Resource::Tool(tool)) => {
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
        (Question::Approve, Resource::Run(_)) => {
            if holds(set, principal, Role::Approver) {
                found.rules.push(ROLE_APPROVER.to_owned());
            }
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
        (Question::PerformAction, Resource::Action(action)) => {
            if holds(set, principal, action.needs) {
                found.rules.push(ROLE_ACTION.to_owned());
            }
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
