//! The copy test prompt: it passes only the short hash it showed, only for
//! what is known not to be irreversible, and the shipped program never
//! builds it.

use std::time::Duration;

use a2m_core::EffectClass;
use a2m_policy::PresenceCheck;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, DuplexStream, duplex};
use tokio::time::Instant;

use crate::presence::PresenceKind;
use crate::surface::recorded_check;
use crate::{
    CopyTestPrompt, PresencePrompt, PromptAnswer, PromptFor, PromptName, PromptText, system_prompt,
    system_prompt_for,
};

const SHORT: &str = "0123456789ab";

type Prompt = CopyTestPrompt<BufReader<DuplexStream>, DuplexStream>;

/// A copy test prompt, the end the runner reads its lines from, and the end
/// the runner answers on.
fn stand() -> (Prompt, BufReader<DuplexStream>, DuplexStream) {
    let (runner_writes, prompt_reads) = duplex(4096);
    let (prompt_writes, runner_reads) = duplex(4096);
    (
        CopyTestPrompt::new(BufReader::new(prompt_reads), prompt_writes),
        BufReader::new(runner_reads),
        runner_writes,
    )
}

fn text(purpose: PromptFor) -> PromptText {
    PromptText {
        title: "Approve a write by `example_orders_item_upsert`".to_owned(),
        detail: format!("Change {SHORT}"),
        purpose,
    }
}

fn soon() -> Instant {
    Instant::now() + Duration::from_secs(30)
}

const WRITE: PromptFor = PromptFor::Approval(Some(EffectClass::Write));

#[tokio::test]
async fn the_short_hash_it_showed_passes_and_is_recorded_as_the_copy_test_harness() {
    let (prompt, mut seen, mut answers) = stand();
    answers
        .write_all(format!("{SHORT}\n").as_bytes())
        .await
        .unwrap();
    let answer = prompt.ask(text(WRITE), soon()).await;
    assert_eq!(answer, PromptAnswer::Present(PromptName::CopyTestHarness));
    // The prompt wrote its text as one line first.
    let mut line = String::new();
    seen.read_line(&mut line).await.unwrap();
    assert_eq!(
        line,
        format!("Approve a write by `example_orders_item_upsert` | Change {SHORT}\n")
    );
    // The ledger's word is its own, never an operating system's.
    assert_eq!(PromptName::CopyTestHarness.as_str(), "field_harness");
    let check = recorded_check(PromptName::CopyTestHarness);
    assert_eq!(check, PresenceCheck::CopyTestHarness);
    assert_eq!(check.as_str(), "field_harness");
    assert!(check.proves_presence());
    // A person's act passes the same way.
    answers
        .write_all(format!("{SHORT}\n").as_bytes())
        .await
        .unwrap();
    assert_eq!(
        prompt.ask(text(PromptFor::Act), soon()).await,
        PromptAnswer::Present(PromptName::CopyTestHarness)
    );
}

#[tokio::test]
async fn a_line_with_another_hash_refuses() {
    let (prompt, _seen, mut answers) = stand();
    for wrong in ["ba9876543210\n", "\n", "0123456789abc\n", " 0123456789ab\n"] {
        answers.write_all(wrong.as_bytes()).await.unwrap();
        assert_eq!(
            prompt.ask(text(WRITE), soon()).await,
            PromptAnswer::Denied,
            "{wrong:?}"
        );
    }
}

#[tokio::test]
async fn an_approval_of_an_irreversible_operation_is_refused_without_asking() {
    let (prompt, mut seen, mut answers) = stand();
    // The right hash is waiting, and still nothing passes.
    answers
        .write_all(format!("{SHORT}\n").as_bytes())
        .await
        .unwrap();
    for purpose in [
        PromptFor::Approval(Some(EffectClass::Irreversible)),
        // An effect nobody told it is treated as irreversible.
        PromptFor::Approval(None),
    ] {
        assert_eq!(
            prompt.ask(text(purpose), soon()).await,
            PromptAnswer::Denied,
            "{purpose:?}"
        );
    }
    // Nothing was written to the runner, so nothing was asked.
    drop(prompt);
    let mut rest = String::new();
    seen.read_to_string(&mut rest).await.unwrap();
    assert_eq!(rest, "");
}

#[tokio::test]
async fn no_answer_in_time_or_a_closed_pipe_is_a_dismissal() {
    let (prompt, _seen, answers) = stand();
    let late = Instant::now() + Duration::from_millis(50);
    assert_eq!(prompt.ask(text(WRITE), late).await, PromptAnswer::Dismissed);
    drop(answers);
    assert_eq!(
        prompt.ask(text(WRITE), soon()).await,
        PromptAnswer::Dismissed
    );
}

#[tokio::test]
async fn the_shipped_program_never_builds_the_copy_test_prompt() {
    let folder = std::env::temp_dir().join("a2m-copy-test-never");
    let paths = a2m_actions::Paths::under(&folder);
    for prompt in [system_prompt(), system_prompt_for(&paths)] {
        let available = prompt.available().await;
        assert_ne!(available.prompt, Some(PromptName::CopyTestHarness));
        assert!(
            available.kind != PresenceKind::OsPrompt
                || available.prompt != Some(PromptName::CopyTestHarness)
        );
    }
}
