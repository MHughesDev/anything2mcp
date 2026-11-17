//! How far something has been proven, and how much was looked at.

use crate::vocabulary::vocabulary;

vocabulary! {
    /// How far a finding, a draft, or a tool has been proven (spec section
    /// 59). The order is the ladder, from the weakest level to the strongest.
    ///
    /// A level is earned, never declared: nothing in this crate lets a caller
    /// set one by hand, and the weakest is the default.
    #[derive(PartialOrd, Ord, Default)]
    pub enum EvidenceLevel: "evidence level" {
        /// Derived and never observed.
        #[default]
        Inferred => "inferred",
        /// Observed on the live system.
        Discovered => "discovered",
        /// A draft operation exists, and compiles.
        Reconstructed => "reconstructed",
        /// Real answers from the system matched the declared schemas.
        SchemaVerified => "schema_verified",
        /// The draft ran in the sandbox.
        Executed => "executed",
        /// The publish gates passed.
        BehaviorVerified => "behavior_verified",
        /// A person published it.
        Approved => "approved",
    }
}

vocabulary! {
    /// How much of what could have been looked at was looked at (spec section
    /// 58). Where it is not stated it is `unknown`, the weakest claim.
    #[derive(Default)]
    pub enum Coverage: "coverage" {
        /// Everything that could be looked at was.
        Complete => "complete",
        /// Some of it was, and the finding says which limits were hit.
        Partial => "partial",
        /// It is not known how much was looked at.
        #[default]
        Unknown => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::{Coverage, EvidenceLevel};

    #[test]
    fn the_ladder_runs_from_inferred_to_approved() {
        let mut climbed = EvidenceLevel::ALL.to_vec();
        climbed.sort();
        assert_eq!(climbed, EvidenceLevel::ALL);
        assert_eq!(EvidenceLevel::ALL.first(), Some(&EvidenceLevel::Inferred));
        assert_eq!(EvidenceLevel::ALL.last(), Some(&EvidenceLevel::Approved));
    }

    #[test]
    fn what_is_not_stated_takes_the_weakest_value() {
        assert_eq!(EvidenceLevel::default(), EvidenceLevel::Inferred);
        assert_eq!(Coverage::default(), Coverage::Unknown);
    }
}
