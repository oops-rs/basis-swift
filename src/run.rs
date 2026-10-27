use std::sync::Arc;

use basis::{AllowAll, CancellationToken, PreparedRun, TurnOptions};
use tokio::sync::Mutex;

use crate::approval::ForeignApprover;
use crate::events::ObserverSink;
use crate::{Approver, BasisError, EventObserver, on_runtime};

/// A conversation: send turns to it, and the model sees everything said so
/// far. Turns on one run are serialized; a second `send` waits for the first.
#[derive(uniffi::Object)]
pub struct Run {
    agent_id: String,
    inner: Arc<Mutex<PreparedRun>>,
}

impl Run {
    pub(crate) fn new(run: PreparedRun) -> Arc<Self> {
        Arc::new(Self {
            agent_id: run.agent_id().to_string(),
            inner: Arc::new(Mutex::new(run)),
        })
    }
}

#[uniffi::export]
impl Run {
    /// The handle `Workspace::resume` takes to pick this conversation up again.
    pub fn agent_id(&self) -> String {
        self.agent_id.clone()
    }

    /// Sends `prompt` and waits for the turn to end.
    ///
    /// No approver allows every call, as basis does for a run given none.
    /// A failed turn is a report with an error outcome, not a throw; this
    /// throws only when there was no turn to report on.
    pub async fn send(
        &self,
        prompt: String,
        observer: Option<Arc<dyn EventObserver>>,
        approver: Option<Arc<dyn Approver>>,
        control: Option<Arc<TurnControl>>,
    ) -> Result<RunReport, BasisError> {
        let inner = Arc::clone(&self.inner);
        on_runtime(async move {
            let mut run = inner.lock().await;
            let sink = ObserverSink::new(observer);
            let options = control.map_or_else(TurnOptions::default, |control| {
                TurnOptions::default()
                    .with_stop(control.stop.clone())
                    .with_cancel(control.cancel.clone())
            });
            let report = match approver {
                Some(approver) => {
                    run.send_with_options(prompt, sink, ForeignApprover(approver), options)
                        .await?
                }
                None => {
                    run.send_with_options(prompt, sink, AllowAll, options)
                        .await?
                }
            };
            Ok(RunReport::from(report))
        })
        .await
    }
}

/// The stop button for one or more turns.
#[derive(uniffi::Object)]
pub struct TurnControl {
    stop: CancellationToken,
    cancel: CancellationToken,
}

#[uniffi::export]
impl TurnControl {
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            stop: CancellationToken::default(),
            cancel: CancellationToken::default(),
        })
    }

    /// Ends the turn at the next round boundary and keeps what the model
    /// committed.
    pub fn stop(&self) {
        self.stop.cancel();
    }

    /// Abandons the turn and rolls it back, leaving the conversation as it
    /// was before the prompt.
    pub fn cancel(&self) {
        self.cancel.cancel();
    }
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum RunOutcome {
    Ok,
    Error { message: String },
}

/// Which bound ended a run, when one did rather than the work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum Bound {
    Deadline,
    ToolBudget,
    TokenBudget,
    /// A bound newer than this binding.
    Other,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct RunReport {
    pub session_id: String,
    pub model: String,
    pub provider: String,
    pub final_message: Option<String>,
    pub outcome: RunOutcome,
    pub stopped_by: Option<Bound>,
    pub usage: Usage,
}

impl<S> From<basis::RunReport<S>> for RunReport {
    fn from(report: basis::RunReport<S>) -> Self {
        let outcome = match report.outcome {
            basis::RunOutcome::Ok => RunOutcome::Ok,
            basis::RunOutcome::Error { message } => RunOutcome::Error { message },
            other => RunOutcome::Error {
                message: format!("{other:?}"),
            },
        };
        // A bound this binding does not know yet is still a bound; reporting
        // it as none would read as "the work ended the run".
        let stopped_by = report.stopped_by.map(|bound| match bound {
            basis::Bound::Deadline => Bound::Deadline,
            basis::Bound::ToolBudget => Bound::ToolBudget,
            basis::Bound::TokenBudget => Bound::TokenBudget,
            _ => Bound::Other,
        });
        let usage = Usage {
            input_tokens: report.usage.input_tokens,
            output_tokens: report.usage.output_tokens,
            cache_read_tokens: report.usage.cache_read_tokens,
            cache_creation_tokens: report.usage.cache_creation_tokens,
            total_tokens: report.usage.total_tokens(),
        };
        Self {
            session_id: report.session_id,
            model: report.model,
            provider: report.provider,
            final_message: report.final_message,
            outcome,
            stopped_by,
            usage,
        }
    }
}
