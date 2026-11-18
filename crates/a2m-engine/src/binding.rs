//! Which binding of an operation a test or a trial runs, and what names it
//! (audit finding AC-6).
//!
//! Evidence is earned by one binding. A test, a trial, and a recorded
//! fixture each run one binding, named by a [`BindingPin`]: its place in the
//! operation's bindings, its mode, and the digest of what it does
//! ([`binding_digest`]). The pin's digest is checked again when the job
//! chooses, so a pin made against a binding that has changed since never
//! runs another binding in its name.

use a2m_core::{
    BindingPlan, BindingStatus, CanonicalJson, Digest, ExprIndex, Failure, Mode, OperationName,
    OperationPlan, Plan, Step,
};
use serde_json::{Value as Json, json};

/// One binding of one operation, named so that the binding that ran is the
/// binding the evidence is about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindingPin {
    /// The operation whose binding this is. A pipeline's other operations
    /// are chosen for as ever.
    pub operation: OperationName,
    /// Its place among the operation's bindings, from 0, in file order.
    pub position: usize,
    /// Its mode, which the binding at that place must have.
    pub mode: Mode,
    /// The digest of what it does ([`binding_digest`]), which the binding at
    /// that place must still have.
    pub digest: Digest,
}

impl BindingPin {
    /// The pin of the binding at `position` of `operation`, as the plan
    /// holds it now. `None` when there is no binding there.
    #[must_use]
    pub fn of(plan: &Plan, operation: &OperationPlan, position: usize) -> Option<Self> {
        let binding = operation.bindings.get(position)?;
        Some(Self {
            operation: operation.name.clone(),
            position,
            mode: binding.mode.clone(),
            digest: binding_digest(plan, binding)?,
        })
    }

    /// The pin as evidence shows it: its place counted from 1, its mode,
    /// and its digest.
    #[must_use]
    pub fn to_json(&self) -> Json {
        json!({
            "position": self.position + 1,
            "mode": self.mode.as_str(),
            "digest": self.digest.to_string(),
        })
    }

    /// The pin evidence stored with [`BindingPin::to_json`], for
    /// `operation`. `None` when it is not one.
    #[must_use]
    pub fn from_json(operation: &OperationName, json: &Json) -> Option<Self> {
        let position = usize::try_from(json["position"].as_u64()?)
            .ok()?
            .checked_sub(1)?;
        Some(Self {
            operation: operation.clone(),
            position,
            mode: Mode::new(json["mode"].as_str()?).ok()?,
            digest: json["digest"].as_str()?.parse().ok()?,
        })
    }

    /// Whether this pin names the binding at its place of `operation` as
    /// the plan holds it now: the same mode and the same digest.
    #[must_use]
    pub fn holds_in(&self, plan: &Plan, operation: &OperationPlan) -> bool {
        operation.name == self.operation
            && operation
                .bindings
                .get(self.position)
                .is_some_and(|binding| {
                    binding.mode == self.mode && binding_digest(plan, binding) == Some(self.digest)
                })
    }
}

/// The binding a test or a trial that names none runs: the one that would
/// act if the operation were called live, the first `verified` binding;
/// where none is verified, the first that is not retired. `None` when every
/// binding is retired.
#[must_use]
pub fn default_binding(operation: &OperationPlan) -> Option<usize> {
    operation
        .bindings
        .iter()
        .position(|binding| binding.status == BindingStatus::Verified)
        .or_else(|| {
            operation
                .bindings
                .iter()
                .position(|binding| binding.status != BindingStatus::Retired)
        })
}

/// The digest of what one binding does: its mode, verb, surface, every
/// lowered step with the text of each expression it evaluates and the
/// whole of each pack section it refers to, its settle time, its evidence,
/// and its precondition. Its status is left out, so a promotion keeps the
/// digest. Indexes into the plan are replaced by what they name, so a change
/// elsewhere in the plan does not change it. `None` when the plan does not
/// hold what the binding refers to, or it cannot be encoded.
#[must_use]
pub fn binding_digest(plan: &Plan, binding: &BindingPlan) -> Option<Digest> {
    let steps = |steps: &[Step]| -> Option<Vec<Json>> {
        steps.iter().map(|step| step_json(plan, step)).collect()
    };
    let shown = json!({
        "format": 1,
        "mode": binding.mode.as_str(),
        "verb": binding.verb.as_str(),
        "surface": serde_json::to_value(plan.surface(binding.surface)?).ok()?,
        "act": steps(&binding.act)?,
        "confirm": steps(&binding.confirm)?,
        "read_back": steps(&binding.read_back)?,
        "lookup": steps(&binding.lookup)?,
        "settle_ms": binding.settle_ms,
        "evidence": serde_json::to_value(binding.evidence).ok()?,
        "precondition": match binding.precondition {
            Some(index) => expr_json(plan, index)?,
            None => Json::Null,
        },
    });
    CanonicalJson::encode(&shown)
        .ok()
        .map(|encoded| encoded.digest())
}

fn expr_json(plan: &Plan, index: ExprIndex) -> Option<Json> {
    let expr = plan.expression(index)?;
    Some(json!({
        "source": expr.source,
        "result": serde_json::to_value(&expr.result).ok()?,
        "library": expr.library,
    }))
}

fn step_json(plan: &Plan, step: &Step) -> Option<Json> {
    let refers_to = match &step.refers_to {
        Some(reference) => json!({
            "section": serde_json::to_value(plan.packs.get(reference.section.get())?).ok()?,
            "entry": reference.entry,
        }),
        None => Json::Null,
    };
    let args: Option<Vec<Json>> = step
        .args
        .iter()
        .map(|arg| Some(json!({"name": arg.name, "value": expr_json(plan, arg.value)?})))
        .collect();
    Some(json!({
        "kind": serde_json::to_value(&step.kind).ok()?,
        "refers_to": refers_to,
        "args": args?,
        "timeout_ms": step.timeout_ms,
        "fails_as": serde_json::to_value(&step.fails_as).ok()?,
        "commits": step.commits,
        "lookup": step.lookup,
        "idempotent": step.idempotent,
        "when": match step.when {
            Some(index) => expr_json(plan, index)?,
            None => Json::Null,
        },
        "payload": serde_json::to_value(&step.payload).ok()?,
    }))
}

/// The refusal of a pin whose binding is no longer the one it names.
pub(crate) fn changed(pin: &BindingPin) -> Failure {
    crate::codes::fail(
        &crate::codes::row::BINDING_CHANGED,
        format!(
            "The binding {} (`{}`) of `{}` is not the binding the test or trial named: it moved, changed its mode, or changed what it does.",
            pin.position + 1,
            pin.mode,
            pin.operation
        ),
        "Run the test or the trial again against the drafts as they are now.",
    )
}
