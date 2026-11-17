//! What an operation changes, and what else it could harm.

use crate::vocabulary::vocabulary;

vocabulary! {
    /// What kind of change an operation makes to the record it names (spec
    /// section 212). The order is from least to most serious, so the effect
    /// class of a pipeline is the greatest of its operations'.
    #[derive(PartialOrd, Ord)]
    pub enum EffectClass: "effect class" {
        /// Changes nothing.
        Read => "read",
        /// Creates or updates records.
        Write => "write",
        /// Cannot be taken back: delete, submit, send, pay.
        Irreversible => "irreversible",
    }
}

vocabulary! {
    /// What an operation could damage beyond the record it names (spec section
    /// 213). The order is from least to most serious, so the hazard of a
    /// pipeline is the greatest of its operations'.
    #[derive(PartialOrd, Ord)]
    pub enum Hazard: "hazard" {
        /// Nothing beyond the named record.
        None => "none",
        /// Data beyond the named record could be lost.
        DataElsewhere => "data_elsewhere",
        /// Something leaves the system and cannot be recalled.
        SendsOutside => "sends_outside",
        /// A device could be left unable to work.
        DeviceFunction => "device_function",
        /// Something in the world moves, heats, opens, or stops.
        Physical => "physical",
    }
}

#[cfg(test)]
mod tests {
    use super::{EffectClass, Hazard};

    #[test]
    fn the_worst_effect_of_several_is_the_greatest() {
        let worst = [
            EffectClass::Write,
            EffectClass::Read,
            EffectClass::Irreversible,
        ]
        .into_iter()
        .max();
        assert_eq!(worst, Some(EffectClass::Irreversible));
        assert!(EffectClass::Read < EffectClass::Write);
    }

    #[test]
    fn the_worst_hazard_of_several_is_the_greatest() {
        let worst = [Hazard::SendsOutside, Hazard::None, Hazard::DataElsewhere]
            .into_iter()
            .max();
        assert_eq!(worst, Some(Hazard::SendsOutside));
        assert!(Hazard::DeviceFunction < Hazard::Physical);
    }
}
