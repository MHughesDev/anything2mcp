//! A replay of recorded answers, in place of a driver (decision 0177).
//!
//! An offline test runs its operation through the engine with every step
//! answered from a fixture: nothing leaves the machine, no session is used,
//! and no program is started. The replay is a [`Driver`] that every pack
//! gets from the contract: a pack writes none, and the engine hands it to
//! each binding of an offline test through
//! [`Registry::replay_driver`](crate::Registry::replay_driver), with the
//! handle's checks of the mode the pack declared.
//!
//! A recorded answer is matched to a step only by what the contract knows
//! of every step: the entry of the pack's file the step refers to, when it
//! refers to one, and otherwise its kind; and, when the answer names them,
//! the values of the step's arguments. Answers for one name are given in the
//! order they were recorded, each once, except one recorded as repeating,
//! which answers every step of its name (decision 0180). A step no answer is left for is a
//! named failure of the test, `unknown_variant`, `fixture_no_answer`, and
//! nothing is sent anywhere (rule 7).

use std::sync::{Arc, Mutex, PoisonError};

use a2m_auth::SessionLease;
use a2m_core::{Step, Values};

use crate::BoxFuture;
use crate::codes;
use crate::driver::{
    Driver, DriverHealth, Observed, PrepareContext, Prepared, StepFailure, StepInput, StepOutcome,
    SurfaceRef,
};
use crate::supervisor::Cleanup;

/// What a recorded answer gives a step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Recorded {
    /// The step was done, and observed these values.
    Done(Values),
    /// The record the step looked for does not exist.
    Absent,
}

/// One recorded answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedAnswer {
    /// What the answer is for: the entry of the pack's file the step
    /// refers to, or, for a step that refers to none, its kind.
    pub name: String,
    /// The arguments the step must have been given, by name, for the answer
    /// to be its; `None` matches a step whatever its arguments.
    pub args: Option<Values>,
    /// The answer.
    pub answer: Recorded,
    /// Whether it answers every step of its name, as often as one is
    /// asked, rather than one step once: a fixture's answer written as one
    /// value, not a list (decision 0180).
    pub repeats: bool,
}

/// Whether an answer of this name is for the step: the name is the entry
/// the step refers to, or the last part of that entry after its last `/`
/// (an entry such as `surface/tool`, whose whole name no file can carry),
/// or, for a step that refers to none, its kind.
#[must_use]
pub fn answers_to(name: &str, step: &Step) -> bool {
    match &step.refers_to {
        Some(reference) => {
            name == reference.entry
                || reference
                    .entry
                    .rsplit_once('/')
                    .is_some_and(|(_, last)| name == last)
        }
        None => name == step.kind.as_str(),
    }
}

/// The name an answer must carry to be a step's, and the name a fixture's
/// file for the step carries: the last part of the entry the step refers to,
/// after its last `/`, or, for a step that refers to none, its kind. A file
/// cannot carry a whole entry such as `surface/tool`, so the name a test
/// records a save's request under, the name the engine is said to have
/// sent, and the name a refusal tells a person to write a file under are
/// all this one (copy test 6, finding 40).
#[must_use]
pub fn answer_name(step: &Step) -> &str {
    step.refers_to.as_ref().map_or_else(
        || step.kind.as_str(),
        |reference| {
            reference
                .entry
                .rsplit_once('/')
                .map_or(reference.entry.as_str(), |(_, last)| last)
        },
    )
}

/// The replay of one offline test: its recorded answers, each used once.
#[derive(Debug, Default)]
pub struct Replay {
    answers: Mutex<Vec<(RecordedAnswer, bool)>>,
    asked: Mutex<Vec<String>>,
    sent: Mutex<Vec<(String, Values)>>,
}

impl Replay {
    /// A replay of these answers.
    #[must_use]
    pub fn new(answers: Vec<RecordedAnswer>) -> Arc<Self> {
        Arc::new(Self {
            answers: Mutex::new(answers.into_iter().map(|answer| (answer, false)).collect()),
            asked: Mutex::new(Vec::new()),
            sent: Mutex::new(Vec::new()),
        })
    }

    /// What the engine sent to each committing step it asked the replay to
    /// answer, in order: the step's name, as [`answer_name`] gives it, and
    /// its arguments as the engine evaluated them. What an offline test of
    /// a write compares with the request its fixture recorded (decision
    /// 0180). A committing step is recorded whether or not an answer was
    /// left for it.
    #[must_use]
    pub fn sent(&self) -> Vec<(String, Values)> {
        self.sent
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The names of the steps it was asked to answer, in order: for a test
    /// to say what its fixture lacked.
    #[must_use]
    pub fn asked(&self) -> Vec<String> {
        self.asked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The names of the recorded answers no step used.
    #[must_use]
    pub fn unused(&self) -> Vec<String> {
        self.answers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter(|(_, used)| !used)
            .map(|(answer, _)| answer.name.clone())
            .collect()
    }

    fn answer(&self, step: &Step, args: &Values) -> Option<Recorded> {
        let name = answer_name(step);
        self.asked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(name.to_owned());
        if step.commits {
            self.sent
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push((name.to_owned(), args.clone()));
        }
        let mut answers = self.answers.lock().unwrap_or_else(PoisonError::into_inner);
        let found = answers.iter_mut().find(|(answer, used)| {
            (!*used || answer.repeats)
                && answers_to(&answer.name, step)
                && answer.args.as_ref().is_none_or(|wanted| {
                    wanted
                        .iter()
                        .all(|(field, value)| args.get(field) == Some(value))
                })
        })?;
        found.1 = true;
        Some(found.0.answer.clone())
    }
}

impl Driver for Replay {
    fn prepare<'a>(
        &'a self,
        _cx: &'a PrepareContext<'a>,
        _lease: Option<&'a SessionLease>,
    ) -> BoxFuture<'a, Result<Prepared, StepFailure>> {
        // Nothing is acquired: no client, no window, no file.
        Box::pin(async move { Ok(Prepared::new(())) })
    }

    fn act<'a>(
        &'a self,
        _prepared: &'a mut Prepared,
        step: &'a Step,
        input: &'a StepInput<'a>,
    ) -> BoxFuture<'a, Result<StepOutcome, StepFailure>> {
        Box::pin(async move {
            match self.answer(step, input.args) {
                Some(Recorded::Done(values)) => Ok(StepOutcome::Done(Observed {
                    values,
                    ..Observed::default()
                })),
                Some(Recorded::Absent) => Ok(StepOutcome::Absent(Observed::default())),
                None => Err(StepFailure::not_sent(codes::failure(
                    &codes::row::FIXTURE_NO_ANSWER,
                    format!(
                        "The fixture records no answer for the step `{}` ({}), so the test stops; nothing was sent.",
                        codes::shown(answer_name(step)),
                        step.kind
                    ),
                    "Record the step's answer in the test's fixture, as a file named after it.",
                ))),
            }
        })
    }

    fn health(&self, _surface: &SurfaceRef) -> DriverHealth {
        DriverHealth::Ready
    }

    fn release<'a>(&'a self, _prepared: Prepared, _poisoned: bool) -> BoxFuture<'a, Cleanup> {
        Box::pin(async move { Cleanup::complete() })
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        reason = "a test: a value that cannot be made stops the test"
    )]

    use std::time::Duration;

    use a2m_core::{
        FailsAs, FailureClass, FailureCode, PackPayload, PackRef, SectionIndex, StepKind,
    };

    use super::*;
    use crate::driver::Deadline;

    fn step(entry: Option<&str>, commits: bool) -> Step {
        Step {
            kind: StepKind::fixed("call_tool"),
            refers_to: entry.map(|entry| PackRef {
                section: SectionIndex::new(0),
                entry: entry.to_owned(),
            }),
            args: Vec::new(),
            timeout_ms: 1_000,
            fails_as: FailsAs {
                class: FailureClass::Drift,
                code: FailureCode::new("response_schema_mismatch").unwrap(),
            },
            commits,
            lookup: false,
            idempotent: false,
            when: None,
            payload: PackPayload::none(),
        }
    }

    /// Copy test 6, finding 40: a save whose entry is `surface/tool` is
    /// said to have been sent under the name a fixture's file carries, so
    /// the request recorded in `<tool>.request.json` is the one compared.
    #[tokio::test]
    async fn a_save_is_sent_under_the_name_its_fixture_file_carries() {
        let save = step(Some("orders/orders_update"), true);
        assert_eq!(answer_name(&save), "orders_update");
        assert_eq!(answer_name(&step(Some("order_put"), true)), "order_put");
        assert_eq!(answer_name(&step(None, false)), "call_tool");
        // Both spellings still answer the step.
        assert!(answers_to("orders_update", &save));
        assert!(answers_to("orders/orders_update", &save));

        let replay = Replay::new(vec![RecordedAnswer {
            name: answer_name(&save).to_owned(),
            args: None,
            answer: Recorded::Done(Values::new()),
            repeats: false,
        }]);
        let args = Values::new().with("order_id", "A-100");
        let input = StepInput::new(&args, Deadline::after(Duration::from_secs(5)));
        let mut prepared = Prepared::new(());
        let outcome = replay.act(&mut prepared, &save, &input).await;
        assert!(matches!(outcome, Ok(StepOutcome::Done(_))));
        assert_eq!(replay.sent(), vec![("orders_update".to_owned(), args)]);
        assert_eq!(replay.asked(), vec!["orders_update".to_owned()]);
        assert!(replay.unused().is_empty());
    }
}
