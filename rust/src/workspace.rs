use std::sync::Arc;

use basis::RunSpec;

use crate::{BasisError, Run, WorkspaceOptions, on_runtime};

/// An opened repository: its context, model and tools settled once, so runs
/// mint from it without settling them again.
#[derive(uniffi::Object)]
pub struct Workspace {
    inner: Arc<basis::Workspace>,
}

#[uniffi::export]
impl Workspace {
    /// Opens the workspace rooted at `path`.
    #[uniffi::constructor]
    pub async fn open(path: String, options: WorkspaceOptions) -> Result<Arc<Self>, BasisError> {
        on_runtime(async move {
            let workspace = options.apply(path.into()).open().await?;
            Ok(Arc::new(Self {
                inner: Arc::new(workspace),
            }))
        })
        .await
    }

    /// Mints a new conversation. Nothing is sent until [`Run::send`].
    pub fn prepare(&self) -> Result<Arc<Run>, BasisError> {
        let run = self.inner.prepare(RunSpec::new(""))?;
        Ok(Run::new(run))
    }

    /// Picks up the conversation `agent_id` names, from this workspace's
    /// history. Resuming another workspace's conversation is refused.
    pub fn resume(&self, agent_id: String) -> Result<Arc<Run>, BasisError> {
        let run = self.inner.resume(&agent_id, RunSpec::new(""))?;
        Ok(Run::new(run))
    }
}
