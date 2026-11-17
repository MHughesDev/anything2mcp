//! The kernel's three health states (spec section 224).

use a2m_core::{EffectClass, Failure};
use serde_json::{Value as Json, json};

use crate::def::HealthNeed;

/// How healthy the kernel is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HealthState {
    /// Everything works.
    Ok,
    /// Something needed for writes is impaired: reads only.
    Degraded,
    /// Something needed for any call is impaired: nothing but status.
    Stopped,
}

impl HealthState {
    /// The word, as `engine.status` shows it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Degraded => "degraded",
            Self::Stopped => "stopped",
        }
    }
}

/// A part of the kernel that can be impaired.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Part {
    /// The store and its migrations.
    Store,
    /// The vault and its key store.
    Vault,
    /// The space evidence is kept in.
    EvidenceSpace,
    /// The policy set.
    Policy,
    /// The session keeper.
    Keeper,
    /// The scheduler.
    Scheduler,
    /// The provider registry.
    Providers,
}

impl Part {
    /// The part's name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Store => "store",
            Self::Vault => "vault",
            Self::EvidenceSpace => "evidence_space",
            Self::Policy => "policy",
            Self::Keeper => "keeper",
            Self::Scheduler => "scheduler",
            Self::Providers => "providers",
        }
    }

    /// The state the kernel is in when this part cannot be opened.
    #[must_use]
    pub const fn when_impaired(self) -> HealthState {
        match self {
            Self::Store | Self::Policy | Self::Keeper | Self::Scheduler => HealthState::Stopped,
            Self::Vault | Self::EvidenceSpace => HealthState::Degraded,
            Self::Providers => HealthState::Ok,
        }
    }
}

/// One impaired part, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Impaired {
    /// The part.
    pub part: Part,
    /// What is wrong with it, with the fix.
    pub failure: Failure,
}

/// The kernel's health: its state, and every impaired part.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Health {
    /// The state: the worst any impaired part brings.
    pub state: HealthState,
    /// The impaired parts, in order of part. Empty when the state is `ok`
    /// and nothing is worth a look.
    pub reasons: Vec<Impaired>,
}

impl Health {
    /// The health that `reasons` make.
    #[must_use]
    pub fn of(mut reasons: Vec<Impaired>) -> Self {
        reasons.sort_by_key(|reason| reason.part);
        let state = reasons
            .iter()
            .map(|reason| reason.part.when_impaired())
            .max()
            .unwrap_or(HealthState::Ok);
        Self { state, reasons }
    }

    /// Whether an action with this need and effect may run now. A write
    /// never runs unless the state is `ok`; a read that needs the store
    /// runs unless the state is `stopped`; an action that needs nothing
    /// always runs.
    #[must_use]
    pub fn allows(&self, needs: HealthNeed, effect: EffectClass) -> bool {
        match self.state {
            HealthState::Ok => true,
            HealthState::Degraded => needs != HealthNeed::Writes && effect == EffectClass::Read,
            HealthState::Stopped => needs == HealthNeed::Nothing,
        }
    }

    /// The cause of the state, in a sentence: the first part that brings it.
    #[must_use]
    pub fn cause(&self) -> String {
        self.reasons
            .iter()
            .find(|reason| reason.part.when_impaired() == self.state)
            .map_or_else(
                || "nothing is impaired".to_owned(),
                |reason| {
                    format!(
                        "the {} is impaired ({})",
                        reason.part.as_str(),
                        reason.failure.code()
                    )
                },
            )
    }

    /// The health as `engine.status` shows it.
    #[must_use]
    pub fn to_json(&self) -> Json {
        json!({
            "state": self.state.as_str(),
            "reasons": self.reasons.iter().map(|reason| json!({
                "part": reason.part.as_str(),
                "brings": reason.part.when_impaired().as_str(),
                "class": reason.failure.class().to_string(),
                "code": reason.failure.code().to_string(),
                "message": reason.failure.message(),
                "fix": reason.failure.next_step(),
            })).collect::<Vec<_>>(),
        })
    }
}

#[cfg(test)]
mod tests {
    use a2m_core::{EffectClass, Failure, FailureClass, FailureCode};

    use super::{Health, HealthState, Impaired, Part};
    use crate::def::HealthNeed;

    fn impaired(part: Part) -> Impaired {
        Impaired {
            part,
            failure: Failure::new(
                FailureClass::Busy,
                FailureCode::fixed("store_unavailable"),
                "It cannot be opened.",
                "Free some space.",
            ),
        }
    }

    #[test]
    fn the_state_is_the_worst_any_impaired_part_brings() {
        assert_eq!(Health::of(Vec::new()).state, HealthState::Ok);
        assert_eq!(
            Health::of(vec![impaired(Part::Vault)]).state,
            HealthState::Degraded
        );
        assert_eq!(
            Health::of(vec![impaired(Part::Vault), impaired(Part::Store)]).state,
            HealthState::Stopped
        );
        assert_eq!(
            Health::of(vec![impaired(Part::Providers)]).state,
            HealthState::Ok
        );
    }

    #[test]
    fn a_write_needs_the_healthy_state_and_status_needs_nothing() {
        let degraded = Health::of(vec![impaired(Part::Vault)]);
        assert!(degraded.allows(HealthNeed::Reads, EffectClass::Read));
        assert!(!degraded.allows(HealthNeed::Reads, EffectClass::Write));
        assert!(!degraded.allows(HealthNeed::Writes, EffectClass::Read));
        let stopped = Health::of(vec![impaired(Part::Store)]);
        assert!(stopped.allows(HealthNeed::Nothing, EffectClass::Read));
        assert!(!stopped.allows(HealthNeed::Reads, EffectClass::Read));
        assert!(stopped.cause().contains("store"));
    }
}
