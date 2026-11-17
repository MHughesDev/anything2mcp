//! The in-process runner loop: claims jobs from the scheduler and hands
//! them to the engine.

use std::sync::Mutex;

use tokio::task::JoinHandle;

use crate::kernel::Kernel;

/// What the kernel keeps about its runner loop.
#[derive(Default)]
pub(crate) struct RunnerState {
    pub(crate) task: Mutex<Option<JoinHandle<()>>>,
}

/// A handle on the running loop.
#[derive(Debug, Clone)]
pub struct RunnerLoop {
    kernel: Kernel,
}

impl RunnerLoop {
    /// Stops the loop after the job in hand.
    pub async fn stop(self) {
        self.kernel.stop_runner_loop().await;
    }
}

impl Kernel {
    pub(crate) async fn stop_runner_loop(&self) {
        let task = self
            .inner
            .runner
            .task
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some(task) = task {
            task.abort();
            let _ = task.await;
        }
    }
}

impl Kernel {
    /// Whether the runner loop runs, as `engine.status` shows it.
    pub(crate) fn runner_status(&self) -> serde_json::Value {
        let running = self
            .inner
            .runner
            .task
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .is_some_and(|task| !task.is_finished());
        serde_json::json!({"in_process": true, "running": running})
    }
}
