//! The session state machine: which moves between the eight states of spec
//! section 215 are legal, and what a call does in each state.

use a2m_core::SessionState;

/// What a call for a session does while the session is in a state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallRule {
    /// The call is given a lease.
    Run,
    /// The call waits for the state to change, up to its wait.
    Wait,
    /// The call stops with `needs_login`.
    NeedsLogin,
}

/// What a call does in `state` (the table of states in section 215).
#[must_use]
pub const fn call_rule(state: SessionState) -> CallRule {
    match state {
        SessionState::Valid => CallRule::Run,
        SessionState::SigningIn
        | SessionState::Suspect
        | SessionState::Renewing
        | SessionState::NeedsPerson => CallRule::Wait,
        SessionState::Absent | SessionState::Expired | SessionState::Revoked => {
            CallRule::NeedsLogin
        }
    }
}

/// Whether a session may move from `from` to `to`. Every move the keeper
/// makes passes this check, and no other move exists.
///
/// | From | To | When |
/// | --- | --- | --- |
/// | `absent`, `expired`, `revoked`, `needs_person` | `signing_in` | A sign-in starts |
/// | `signing_in` | `valid` | The sign-in succeeded |
/// | `signing_in` | `needs_person` | A stored credential was refused |
/// | `signing_in` | `absent` | The sign-in was abandoned or could not be made |
/// | `valid` | `suspect` | A check failed, or a driver reported a sign of expiry |
/// | `valid` | `renewing` | The credential's own expiry was reached |
/// | `valid` | `expired` | The maximum age was reached |
/// | `suspect` | `valid` | The one re-check passed |
/// | `suspect` | `renewing` | The one re-check failed |
/// | `renewing` | `valid` | The renewal succeeded |
/// | `renewing` | `needs_person` | The renewal needs a person |
/// | `renewing` | `expired` | The renewal failed and nobody can be asked |
/// | `needs_person` | `valid` | The person answered a light prompt |
/// | `needs_person` | `expired` | The session lapsed while it waited for a person |
/// | any but `absent` and `revoked` | `revoked` | The session was ended on purpose |
#[must_use]
pub const fn legal_move(from: SessionState, to: SessionState) -> bool {
    use SessionState::{
        Absent, Expired, NeedsPerson, Renewing, Revoked, SigningIn, Suspect, Valid,
    };
    matches!(
        (from, to),
        (Absent | Expired | Revoked | NeedsPerson, SigningIn)
            | (SigningIn, Valid | NeedsPerson | Absent | Revoked)
            | (Valid, Suspect | Renewing | Expired | Revoked)
            | (Suspect, Valid | Renewing | Revoked)
            | (Renewing, Valid | NeedsPerson | Expired | Revoked)
            | (NeedsPerson, Valid | Expired | Revoked)
            | (Expired, Revoked)
    )
}

#[cfg(test)]
mod tests {
    use a2m_core::SessionState;

    use super::{CallRule, call_rule, legal_move};

    /// Every legal move, written out a second time by name. The test walks
    /// every pair of states and holds the function to this list.
    const LEGAL: &[(&str, &str)] = &[
        ("absent", "signing_in"),
        ("expired", "signing_in"),
        ("revoked", "signing_in"),
        ("needs_person", "signing_in"),
        ("signing_in", "valid"),
        ("signing_in", "needs_person"),
        ("signing_in", "absent"),
        ("signing_in", "revoked"),
        ("valid", "suspect"),
        ("valid", "renewing"),
        ("valid", "expired"),
        ("valid", "revoked"),
        ("suspect", "valid"),
        ("suspect", "renewing"),
        ("suspect", "revoked"),
        ("renewing", "valid"),
        ("renewing", "needs_person"),
        ("renewing", "expired"),
        ("renewing", "revoked"),
        ("needs_person", "valid"),
        ("needs_person", "expired"),
        ("needs_person", "revoked"),
        ("expired", "revoked"),
    ];

    #[test]
    fn every_pair_of_states_is_legal_only_when_the_table_lists_it() {
        let mut legal = 0;
        for from in SessionState::ALL {
            for to in SessionState::ALL {
                let listed = LEGAL.contains(&(from.as_str(), to.as_str()));
                assert_eq!(legal_move(*from, *to), listed, "{from} to {to}");
                legal += usize::from(listed);
            }
        }
        assert_eq!(legal, LEGAL.len());
        assert_eq!(SessionState::ALL.len(), 8);
    }

    #[test]
    fn no_state_moves_to_itself_and_only_a_sign_in_leaves_an_ended_session() {
        for state in SessionState::ALL {
            assert!(!legal_move(*state, *state), "{state}");
        }
        for ended in [SessionState::Absent, SessionState::Revoked] {
            for to in SessionState::ALL {
                assert_eq!(
                    legal_move(ended, *to),
                    *to == SessionState::SigningIn,
                    "{ended} to {to}"
                );
            }
        }
    }

    #[test]
    fn a_call_runs_only_on_a_valid_session() {
        let rules: Vec<(&str, CallRule)> = SessionState::ALL
            .iter()
            .map(|state| (state.as_str(), call_rule(*state)))
            .collect();
        assert_eq!(
            rules,
            [
                ("absent", CallRule::NeedsLogin),
                ("signing_in", CallRule::Wait),
                ("valid", CallRule::Run),
                ("suspect", CallRule::Wait),
                ("renewing", CallRule::Wait),
                ("needs_person", CallRule::Wait),
                ("expired", CallRule::NeedsLogin),
                ("revoked", CallRule::NeedsLogin),
            ]
        );
    }
}
