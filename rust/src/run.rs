use std::sync::Arc;

use std::time::{Duration, SystemTime};

use basis::{AllowAll, CancellationToken, PreparedRun, TurnOptions};
use tokio::sync::Mutex;

use crate::approval::ForeignApprover;
use crate::events::ObserverSink;
use crate::tools::{ActiveTurns, TurnBounds};
use crate::{Approver, BasisError, EventObserver, on_runtime};

/// How hard the model reasons, as basis's `Effort`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum Effort {
    Low,
    Medium,
    High,
    XHigh,
    Max,
}

impl From<Effort> for basis::Effort {
    fn from(effort: Effort) -> Self {
        match effort {
            Effort::Low => Self::Low,
            Effort::Medium => Self::Medium,
            Effort::High => Self::High,
            Effort::XHigh => Self::XHigh,
            Effort::Max => Self::Max,
        }
    }
}

/// How the Responses wire reaches its endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ResponsesTransport {
    HttpSse,
    WebSocket,
}

/// What a new conversation is prepared with. basis fixes these for the
/// conversation's life: a resumed run keeps the ones it was prepared with,
/// which [`Run::conversation_options`] reads back.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ConversationOptions {
    #[uniffi(default = None)]
    pub session_name: Option<String>,
    #[uniffi(default = None)]
    pub effort: Option<Effort>,
    #[uniffi(default = None)]
    pub responses_transport: Option<ResponsesTransport>,
    /// Whether a Responses endpoint may store the response server-side.
    #[uniffi(default = None)]
    pub responses_store: Option<bool>,
    /// Replay the whole conversation each round instead of chaining on the
    /// endpoint's stored responses.
    #[uniffi(default = false)]
    pub responses_replay_only: bool,
}

impl ConversationOptions {
    pub(crate) fn request_options(&self) -> basis::ProviderRequestOptions {
        let mut options = basis::ProviderRequestOptions {
            reasoning: self.effort.map(|effort| basis::ReasoningOptions {
                effort: Some(basis::Effort::from(effort).into()),
                summary: None,
            }),
            ..Default::default()
        };
        if let Some(transport) = self.responses_transport {
            options.responses.transport = match transport {
                ResponsesTransport::HttpSse => basis::runtime::ResponsesTransport::HttpSse,
                ResponsesTransport::WebSocket => basis::runtime::ResponsesTransport::WebSocket,
            };
        }
        if self.responses_replay_only {
            options.responses.state_mode = basis::runtime::ResponsesStateMode::ReplayOnly;
        }
        options.responses.store = self.responses_store;
        options
    }

    fn saved(options: &basis::ProviderRequestOptions) -> Self {
        let effort = options
            .reasoning
            .as_ref()
            .and_then(|reasoning| reasoning.effort)
            .and_then(|effort| {
                [
                    Effort::Low,
                    Effort::Medium,
                    Effort::High,
                    Effort::XHigh,
                    Effort::Max,
                ]
                .into_iter()
                .find(|candidate| {
                    let mapped = Some(basis::Effort::from(*candidate).into());
                    mapped == Some(effort)
                })
            });
        Self {
            session_name: None,
            effort,
            responses_transport: Some(match options.responses.transport {
                basis::runtime::ResponsesTransport::WebSocket => ResponsesTransport::WebSocket,
                _ => ResponsesTransport::HttpSse,
            }),
            responses_store: options.responses.store,
            responses_replay_only: options.responses.state_mode
                == basis::runtime::ResponsesStateMode::ReplayOnly,
        }
    }
}

/// The bounds of one turn, as basis's `TurnOptions`. Unset is unbounded, or
/// basis's own default.
#[derive(Debug, Clone, Default, uniffi::Record)]
pub struct TurnLimits {
    #[uniffi(default = None)]
    pub tool_budget: Option<u32>,
    #[uniffi(default = None)]
    pub model_budget: Option<u32>,
    #[uniffi(default = None)]
    pub token_budget: Option<u64>,
    #[uniffi(default = None)]
    pub retry_budget: Option<u32>,
    /// Seconds from the start of the turn.
    #[uniffi(default = None)]
    pub deadline_seconds: Option<u64>,
}

/// A conversation: send turns to it, and the model sees everything said so
/// far. Turns on one run are serialized; a second `send` waits for the first.
#[derive(uniffi::Object)]
pub struct Run {
    agent_id: String,
    inner: Arc<Mutex<PreparedRun>>,
    turns: Arc<ActiveTurns>,
}

impl Run {
    pub(crate) fn new(run: PreparedRun, turns: Arc<ActiveTurns>) -> Arc<Self> {
        Arc::new(Self {
            agent_id: run.agent_id().to_string(),
            inner: Arc::new(Mutex::new(run)),
            turns,
        })
    }
}

#[uniffi::export]
impl Run {
    /// The handle `Workspace::resume` takes to pick this conversation up again.
    pub fn agent_id(&self) -> String {
        self.agent_id.clone()
    }

    /// The options this conversation was prepared with, as saved.
    pub async fn conversation_options(&self) -> ConversationOptions {
        let mut run = self.inner.lock().await;
        ConversationOptions::saved(&run.session_mut().config().provider_request_options)
    }

    /// Sets, or with `None` clears, the reasoning effort later turns ask for.
    #[uniffi::method(default(effort = None))]
    pub async fn set_effort(&self, effort: Option<Effort>) -> Result<(), BasisError> {
        let mut run = self.inner.lock().await;
        run.set_effort(effort.map(Into::into))?;
        Ok(())
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
        limits: Option<TurnLimits>,
    ) -> Result<RunReport, BasisError> {
        let inner = Arc::clone(&self.inner);
        let turns = Arc::clone(&self.turns);
        let agent_id = self.agent_id.clone();
        on_runtime(async move {
            let mut run = inner.lock().await;
            let sink = ObserverSink::new(observer);
            let control = control.unwrap_or_else(TurnControl::new);
            let limits = limits.unwrap_or_default();
            let mut options = TurnOptions::default()
                .with_stop(control.stop.clone())
                .with_cancel(control.cancel.clone());
            let mut deadline = None;
            if let Some(seconds) = limits.deadline_seconds {
                let duration = Duration::from_secs(seconds);
                options = options.with_absolute_deadline(SystemTime::now() + duration);
                deadline = Some(tokio::time::Instant::now() + duration);
            }
            if let Some(budget) = limits.tool_budget {
                options = options.with_tool_budget(budget as usize);
            }
            if let Some(budget) = limits.model_budget {
                options = options.with_model_budget(budget as usize);
            }
            if let Some(budget) = limits.token_budget {
                options = options.with_token_budget(budget);
            }
            if let Some(budget) = limits.retry_budget {
                options = options.with_retry_budget(budget as usize);
            }
            turns.begin(
                &agent_id,
                TurnBounds {
                    cancel: control.cancel.clone(),
                    deadline,
                },
            );
            let _turn = TurnGuard(&turns, &agent_id);
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

struct TurnGuard<'a>(&'a ActiveTurns, &'a str);

impl Drop for TurnGuard<'_> {
    fn drop(&mut self) {
        self.0.end(self.1);
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
    pub reasoning_tokens: u64,
    pub thoughts_tokens: u64,
    pub total_tokens: u64,
    /// Completed model responses that reported usage: zero beside nonzero
    /// work means the provider reported none, not that nothing was spent.
    pub model_responses: u32,
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
            reasoning_tokens: report.usage.reasoning_tokens,
            thoughts_tokens: report.usage.thoughts_tokens,
            total_tokens: report.usage.total_tokens(),
            model_responses: report.usage.model_responses,
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
