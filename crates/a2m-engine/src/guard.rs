//! Two guards around what the engine does not own: a call to the control
//! plane that never answers, and a provider's code that panics.
//!
//! Neither may take the engine down or hold a job for ever. A call to
//! `Control` is given up after the time the settings allow, and the engine
//! goes on as it would after any failure of that call: a write that was
//! not recorded is in doubt, never a success. A panic in a provider's code
//! is caught where the engine calls it, and comes back as a failure that
//! names it.

use std::future::Future;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use a2m_auth::{ExpirySign, SessionLease};
use a2m_core::{Digest, Failure, FailureClass, Plan, SurfaceName};
use a2m_ledger::{
    Acted, ActedToken, BlobRef, IntentToken, LockHeld, ReadBack, Reconciled, RecordIntent,
    RecordRef, Settled,
};
use a2m_provider::BoxFuture;

use crate::codes;
use crate::control::{
    Clearance, Control, EngineEvent, Evidence, GuardQuestion, KeptCapture, Preview, SessionWant,
};

/// A future whose panic is caught: `None` when it panicked.
pub(crate) struct Caught<'f, T>(Pin<Box<dyn Future<Output = T> + Send + 'f>>);

/// Runs a provider's future so that a panic in it ends the future and not
/// the engine.
pub(crate) fn caught<'f, T>(future: impl Future<Output = T> + Send + 'f) -> Caught<'f, T> {
    Caught(Box::pin(future))
}

impl<T> Future for Caught<'_, T> {
    type Output = Option<T>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // What the future held is not used again after a panic: the driver
        // context it worked on is marked as not to be used, and recycled.
        match catch_unwind(AssertUnwindSafe(|| self.0.as_mut().poll(cx))) {
            Ok(Poll::Ready(value)) => Poll::Ready(Some(value)),
            Ok(Poll::Pending) => Poll::Pending,
            Err(_) => Poll::Ready(None),
        }
    }
}

/// `Control`, with every call given up after a time limit.
pub(crate) struct Timed<'c> {
    inner: &'c dyn Control,
    limit: Duration,
}

impl<'c> Timed<'c> {
    pub fn new(inner: &'c dyn Control, limit: Duration) -> Self {
        Self { inner, limit }
    }

    async fn within<T>(
        &self,
        call: &'static str,
        answer: BoxFuture<'_, Result<T, Failure>>,
    ) -> Result<T, Failure> {
        match tokio::time::timeout(self.limit, answer).await {
            Ok(answer) => answer,
            Err(_) => Err(codes::fail(
                FailureClass::Busy,
                codes::CONTROL_UNANSWERED,
                format!(
                    "The control plane did not answer `{call}` within the time the engine waits."
                ),
                "See `anything2mcp engine status`, then resume the run.",
            )),
        }
    }
}

impl Control for Timed<'_> {
    fn plan<'a>(&'a self, hash: &'a Digest) -> BoxFuture<'a, Result<Arc<Plan>, Failure>> {
        Box::pin(self.within("plan", self.inner.plan(hash)))
    }

    fn decide<'a>(&'a self, question: &'a GuardQuestion) -> BoxFuture<'a, Result<(), Failure>> {
        Box::pin(self.within("decide", self.inner.decide(question)))
    }

    fn session_lease<'a>(
        &'a self,
        want: &'a SessionWant,
    ) -> BoxFuture<'a, Result<SessionLease, Failure>> {
        Box::pin(self.within("session_lease", self.inner.session_lease(want)))
    }

    fn report_expiry<'a>(&'a self, lease: &'a SessionLease, sign: ExpirySign) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            // A report that is not answered is given up: the lease is asked
            // for again either way.
            let _ = tokio::time::timeout(self.limit, self.inner.report_expiry(lease, sign)).await;
        })
    }

    fn rate<'a>(
        &'a self,
        surface: &'a SurfaceName,
        want: u32,
    ) -> BoxFuture<'a, Result<u32, Failure>> {
        Box::pin(self.within("rate", self.inner.rate(surface, want)))
    }

    fn store_preview<'a>(&'a self, preview: &'a Preview) -> BoxFuture<'a, Result<(), Failure>> {
        Box::pin(self.within("store_preview", self.inner.store_preview(preview)))
    }

    fn approval<'a>(&'a self, diff: &'a Digest) -> BoxFuture<'a, Result<Clearance, Failure>> {
        Box::pin(self.within("approval", self.inner.approval(diff)))
    }

    fn lock<'a>(&'a self, record: RecordRef) -> BoxFuture<'a, Result<LockHeld, Failure>> {
        Box::pin(self.within("lock", self.inner.lock(record)))
    }

    fn intend<'a>(
        &'a self,
        intents: Vec<RecordIntent>,
    ) -> BoxFuture<'a, Result<Vec<IntentToken>, Failure>> {
        Box::pin(self.within("intend", self.inner.intend(intents)))
    }

    fn acted<'a>(
        &'a self,
        token: IntentToken,
        acted: Acted,
    ) -> BoxFuture<'a, Result<ActedToken, Failure>> {
        Box::pin(self.within("acted", self.inner.acted(token, acted)))
    }

    fn verified<'a>(
        &'a self,
        token: &'a ActedToken,
        read_back: ReadBack,
    ) -> BoxFuture<'a, Result<(), Failure>> {
        Box::pin(self.within("verified", self.inner.verified(token, read_back)))
    }

    fn failed<'a>(
        &'a self,
        record: RecordRef,
        why: &'a Failure,
        evidence: Option<BlobRef>,
    ) -> BoxFuture<'a, Result<(), Failure>> {
        Box::pin(self.within("failed", self.inner.failed(record, why, evidence)))
    }

    fn settle<'a>(
        &'a self,
        record: RecordRef,
        seen: Reconciled,
    ) -> BoxFuture<'a, Result<Settled, Failure>> {
        Box::pin(self.within("settle", self.inner.settle(record, seen)))
    }

    fn send<'a>(&'a self, event: EngineEvent) -> BoxFuture<'a, Result<(), Failure>> {
        Box::pin(self.within("send", self.inner.send(event)))
    }

    fn evidence<'a>(&'a self, blob: Evidence) -> BoxFuture<'a, Result<BlobRef, Failure>> {
        Box::pin(self.within("evidence", self.inner.evidence(blob)))
    }

    fn keep_capture<'a>(&'a self, capture: KeptCapture) -> BoxFuture<'a, Result<BlobRef, Failure>> {
        Box::pin(self.within("keep_capture", self.inner.keep_capture(capture)))
    }

    fn acknowledged<'a>(
        &'a self,
        token: &'a ActedToken,
        evidence: Option<BlobRef>,
    ) -> BoxFuture<'a, Result<(), Failure>> {
        Box::pin(self.within("acknowledged", self.inner.acknowledged(token, evidence)))
    }

    fn project_folder(&self) -> Option<std::path::PathBuf> {
        self.inner.project_folder()
    }

    // Not timed here: the driver's question is held to the step's own
    // deadline by `Prepared::recorded_elsewhere`.
    fn other_records(
        &self,
        operation: &a2m_core::OperationName,
        asking: a2m_ledger::RecordRef,
    ) -> Option<std::sync::Arc<dyn a2m_provider::OtherRecords>> {
        self.inner.other_records(operation, asking)
    }

    fn step_observed(&self, step: &a2m_core::Step, observed: Option<&a2m_core::Values>) {
        self.inner.step_observed(step, observed);
    }

    fn keeps_shadow(&self) -> bool {
        self.inner.keeps_shadow()
    }

    fn shadow_rate<'a>(
        &'a self,
        surface: &'a SurfaceName,
        want: u32,
    ) -> BoxFuture<'a, Result<bool, Failure>> {
        Box::pin(self.within("shadow_rate", self.inner.shadow_rate(surface, want)))
    }

    fn shadow<'a>(
        &'a self,
        compared: crate::control::ShadowCompared,
    ) -> BoxFuture<'a, Result<(), Failure>> {
        Box::pin(self.within("shadow", self.inner.shadow(compared)))
    }
}
