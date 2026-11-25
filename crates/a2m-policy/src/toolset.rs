//! Toolsets and role grants, compiled to the policy engine's form.
//!
//! A toolset is a named selection of tools with who may connect to it, the
//! highest effect class its runs may have, the identity kind its runs use,
//! and whether a write there may go without a person. It is a lower layer:
//! it only narrows. It holds no permit, so it cannot give a caller anything
//! a grant does not; the evaluator consults it as a set of forbids that
//! follow the request's toolset (spec sections 212 and 214).
//!
//! A role grant is a grant given to a role instead of to a subject, so a
//! role is a bundle of grants: everyone who holds the role holds each one.

use a2m_core::{EffectClass, Failure};
use serde::{Deserialize, Serialize};

use crate::failure;
use crate::model::{IdentityKind, Question, Request, Resource, Role, ToolRef, ToolsetName};
use crate::set::{PolicyLayers, Scope, Subject};

/// A toolset: a named selection of tools for a group of callers. It only
/// narrows what the grants allow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Toolset {
    /// Its name, as a call through it states.
    pub name: ToolsetName,
    /// Who may connect to it. Nobody not listed does.
    pub who: Vec<Subject>,
    /// The tools it selects. A tool no scope covers is not in it.
    pub tools: Vec<Scope>,
    /// The highest effect class a run through it may have.
    pub max_effect: EffectClass,
    /// The identity kind every run through it uses.
    pub identity_mode: IdentityKind,
    /// Whether a rule may let a write through it proceed without a person.
    /// False means every write through it waits for one.
    pub allow_pre_approval: bool,
}

impl Toolset {
    /// Whether this toolset selects `tool`.
    #[must_use]
    pub fn selects(&self, tool: &ToolRef) -> bool {
        self.tools.iter().any(|scope| scope.covers(tool))
    }
}

/// A grant given to a role: everyone who holds `role` may use the tools
/// `scope` covers, up to `max_effect`, through `identity_mode`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoleGrant {
    /// The role that holds it.
    pub role: Role,
    /// The tools it covers.
    pub scope: Scope,
    /// The highest effect class it allows.
    pub max_effect: EffectClass,
    /// The identity kind a run under it may use.
    pub identity_mode: IdentityKind,
}

/// The rule that stands for a toolset the set does not define.
pub(crate) const UNKNOWN_TOOLSET: &str = "toolset.unknown";

/// What the toolsets of a set forbid for one request.
pub(crate) struct ToolsetVerdict {
    /// Every forbid that matched, as `toolset.<name>.<reason>`.
    pub(crate) rules: Vec<String>,
    /// What a refused caller is told.
    pub(crate) refusal: Option<Failure>,
}

const fn is_work(question: Question) -> bool {
    matches!(
        question,
        Question::UseTool | Question::ActAs | Question::WriteWithoutPerson
    )
}

fn refuse(question: Question) -> Failure {
    match question {
        Question::ActAs => failure::identity_not_allowed(),
        _ => failure::not_granted(),
    }
}

/// The forbids of the toolset a request came through, if it forbids any.
/// Once a set defines toolsets, a toolset it does not define is unknown, and
/// unknown means stop. A set with no toolsets imposes nothing here.
pub(crate) fn forbids(layers: &PolicyLayers, request: &Request) -> Option<ToolsetVerdict> {
    let named = request.context.toolset.as_ref()?;
    if layers.toolsets.is_empty() || !is_work(request.question) {
        return None;
    }
    let Some(toolset) = layers.toolsets.iter().find(|set| set.name == *named) else {
        return Some(ToolsetVerdict {
            rules: vec![UNKNOWN_TOOLSET.to_owned()],
            refusal: Some(refuse(request.question)),
        });
    };
    let name = toolset.name.as_str();
    let mut found: Vec<(&str, Option<Failure>)> = Vec::new();
    if !toolset.who.iter().any(|who| who.covers(&request.principal)) {
        found.push(("not_connectable", Some(refuse(request.question))));
    }
    if request.question != Question::ActAs {
        if let Resource::Tool(tool) = &request.resource
            && !toolset.selects(tool)
        {
            found.push(("not_selected", Some(failure::not_granted())));
        }
        if request.context.effect > toolset.max_effect {
            found.push(("effect_ceiling", Some(failure::effect_too_high())));
        }
    }
    if request.question == Question::ActAs
        && request.context.identity_mode != Some(toolset.identity_mode)
    {
        found.push(("identity_mode", Some(failure::identity_not_allowed())));
    }
    if request.question == Question::WriteWithoutPerson && !toolset.allow_pre_approval {
        // Not a refusal: the run waits for a person.
        found.push(("no_pre_approval", None));
    }
    if found.is_empty() {
        return None;
    }
    let refusal = found.iter_mut().find_map(|(_, failure)| failure.take());
    let rules = found
        .iter()
        .map(|(reason, _)| format!("toolset.{name}.{reason}"))
        .collect();
    Some(ToolsetVerdict { rules, refusal })
}

/// What a toolset rule says, in words, for `explain`.
pub(crate) fn describe(layers: &PolicyLayers, rest: &str) -> Option<String> {
    let (name, reason) = rest.rsplit_once('.')?;
    let toolset = layers
        .toolsets
        .iter()
        .find(|set| set.name.as_str() == name)?;
    Some(match reason {
        "not_connectable" => format!(
            "The toolset `{name}` is not open to this caller: only the principals it lists may connect, and it only narrows what the grants allow."
        ),
        "not_selected" => format!("The toolset `{name}` does not select this tool."),
        "effect_ceiling" => format!(
            "The toolset `{name}` allows runs up to the effect class `{}`, and this run is above it.",
            toolset.max_effect
        ),
        "identity_mode" => format!(
            "The toolset `{name}` runs only with the identity kind `{}`.",
            toolset.identity_mode
        ),
        "no_pre_approval" => format!(
            "The toolset `{name}` lets no write proceed without a person, so every write through it waits for one."
        ),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use a2m_core::{CanonicalJson, EffectClass};

    use super::Toolset;
    use crate::model::{IdentityKind, ToolsetName};
    use crate::set::{CandidateSet, DeploymentShape, PolicyLayers, Scope, Subject};

    fn toolset(name: &str) -> Toolset {
        Toolset {
            name: ToolsetName::new(name).unwrap(),
            who: vec![Subject::Anyone],
            tools: vec![Scope::Pattern("example_*".to_owned())],
            max_effect: EffectClass::Read,
            identity_mode: IdentityKind::PerUserSession,
            allow_pre_approval: false,
        }
    }

    fn compiles(layers: &PolicyLayers) -> bool {
        CandidateSet::compile(layers, DeploymentShape::Server).is_ok()
    }

    #[test]
    fn a_toolset_that_names_no_one_or_no_tool_or_repeats_does_not_compile() {
        let good = PolicyLayers::default().with_toolset(toolset("reporting"));
        assert!(compiles(&good));
        let mut nobody = toolset("reporting");
        nobody.who.clear();
        assert!(!compiles(&PolicyLayers::default().with_toolset(nobody)));
        let mut nothing = toolset("reporting");
        nothing.tools.clear();
        assert!(!compiles(&PolicyLayers::default().with_toolset(nothing)));
        let mut bad_pattern = toolset("reporting");
        bad_pattern.tools = vec![Scope::Pattern("Example Order".to_owned())];
        assert!(!compiles(
            &PolicyLayers::default().with_toolset(bad_pattern)
        ));
        let mut twice = good;
        twice.toolsets.push(toolset("reporting"));
        assert!(!compiles(&twice));
    }

    #[test]
    fn a_set_with_no_toolsets_is_stored_as_before() {
        // The new layers leave no trace in a set that does not use them, so
        // a version stored before they existed still reads back.
        let text = CanonicalJson::encode(&PolicyLayers::default()).unwrap();
        let text = String::from_utf8(text.as_bytes().to_vec()).unwrap();
        for absent in ["toolsets", "role_grants", "second_admin_publish"] {
            assert!(!text.contains(absent), "{text}");
        }
    }

    #[test]
    fn a_toolset_is_replaced_by_name_and_removed_by_name() {
        let layers = PolicyLayers::default().with_toolset(toolset("reporting"));
        let mut wider = toolset("reporting");
        wider.max_effect = EffectClass::Write;
        let layers = layers.with_toolset(wider);
        assert_eq!(layers.toolsets.len(), 1);
        assert_eq!(layers.toolsets[0].max_effect, EffectClass::Write);
        let name = ToolsetName::new("reporting").unwrap();
        let (layers, removed) = layers.without_toolset(&name);
        assert!(removed && layers.toolsets.is_empty());
        assert!(!layers.without_toolset(&name).1);
    }
}
