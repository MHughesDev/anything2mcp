//! The copy test prompt: a presence prompt that answers on a pipe a test
//! runner holds, for the copy test harness of decision 0310.
//!
//! The shipped program never constructs one: [`system_prompt`] and
//! [`system_prompt_for`] have no arm that returns it, and a test holds
//! that on every system. The harness, a separate program that is never
//! packaged, builds it with its own standard input and output.
//!
//! When asked, it writes the prompt's title and detail as one line and
//! reads one line. It passes only when that line is the short hash the
//! prompt showed, which is the last word of the detail. It refuses
//! without asking an approval of an operation declared `irreversible`, and
//! one whose effect class it was not told (rule 3, and failing closed).
//!
//! [`system_prompt`]: super::system_prompt
//! [`system_prompt_for`]: super::system_prompt_for

use a2m_actions::BoxFuture;
use a2m_core::EffectClass;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::Mutex;
use tokio::time::{Instant, timeout_at};

use super::{Availability, PresencePrompt, PromptAnswer, PromptFor, PromptName, PromptText};

/// A presence prompt that asks on `writer` and is answered on `reader`.
pub struct CopyTestPrompt<R, W> {
    pipes: Mutex<(R, W)>,
}

impl<R, W> CopyTestPrompt<R, W>
where
    R: AsyncBufRead + Unpin + Send,
    W: AsyncWrite + Unpin + Send,
{
    /// A copy test prompt over the two ends of which the runner holds the
    /// other sides.
    #[must_use]
    pub fn new(reader: R, writer: W) -> Self {
        Self {
            pipes: Mutex::new((reader, writer)),
        }
    }
}

/// Whether an ask is for a kind of change this prompt may pass: an act of
/// a person, or an approval of an operation that is known not to be
/// irreversible.
const fn passable(purpose: PromptFor) -> bool {
    match purpose {
        PromptFor::Act => true,
        PromptFor::Approval(Some(EffectClass::Irreversible) | None) => false,
        PromptFor::Approval(Some(_)) => true,
    }
}

/// The short hash a prompt showed: the last word of its detail.
fn shown_hash(detail: &str) -> &str {
    detail.split_whitespace().next_back().unwrap_or("")
}

/// One line of text: no line break the runner could mistake for the end.
fn one_line(text: &str) -> String {
    text.chars()
        .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
        .collect()
}

impl<R, W> PresencePrompt for CopyTestPrompt<R, W>
where
    R: AsyncBufRead + Unpin + Send,
    W: AsyncWrite + Unpin + Send,
{
    fn available(&self) -> BoxFuture<'_, Availability> {
        Box::pin(async {
            Availability::ready(
                PromptName::CopyTestHarness,
                "The copy test harness answers on a pipe that only the test runner holds.",
            )
        })
    }

    fn ask(&self, text: PromptText, deadline: Instant) -> BoxFuture<'_, PromptAnswer> {
        Box::pin(async move {
            if !passable(text.purpose) {
                return PromptAnswer::Denied;
            }
            let short = shown_hash(&text.detail).to_owned();
            let line = format!("{} | {}\n", one_line(&text.title), one_line(&text.detail));
            let mut pipes = self.pipes.lock().await;
            let (reader, writer) = &mut *pipes;
            let exchange = async {
                writer.write_all(line.as_bytes()).await.ok()?;
                writer.flush().await.ok()?;
                let mut answer = String::new();
                match reader.read_line(&mut answer).await {
                    Ok(0) | Err(_) => None,
                    Ok(_) => Some(answer),
                }
            };
            match timeout_at(deadline, exchange).await {
                Ok(Some(answer)) => {
                    let given = answer.trim_end_matches(['\n', '\r']);
                    if !short.is_empty() && given == short {
                        PromptAnswer::Present(PromptName::CopyTestHarness)
                    } else {
                        PromptAnswer::Denied
                    }
                }
                Ok(None) | Err(_) => PromptAnswer::Dismissed,
            }
        })
    }
}
