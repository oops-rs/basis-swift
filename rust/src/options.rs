use std::path::PathBuf;

use basis::{BuiltinProvider, ModelSelector, RuntimeBuilder, WorkspaceBuilder};

/// Which built-in provider answers, when the host names one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum Provider {
    Anthropic,
    Gemini,
    OpenAi,
    OpenRouter,
    Ollama,
    LmStudio,
}

impl From<Provider> for BuiltinProvider {
    fn from(provider: Provider) -> Self {
        match provider {
            Provider::Anthropic => Self::Anthropic,
            Provider::Gemini => Self::Gemini,
            Provider::OpenAi => Self::OpenAI,
            Provider::OpenRouter => Self::OpenRouter,
            Provider::Ollama => Self::Ollama,
            Provider::LmStudio => Self::LmStudio,
        }
    }
}

/// Where conversations are kept, so a later process can resume them.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum History {
    /// mentra's default location.
    Default,
    /// A directory the host names — an app's container, typically.
    Directory { path: String },
    /// In memory: resumable while the workspace lives, gone with the process.
    Ephemeral,
}

/// What a workspace is opened with. Every field is optional; unset means
/// basis's own default, and basis reads no config file it was not handed.
#[derive(Debug, Clone, uniffi::Record)]
pub struct WorkspaceOptions {
    #[uniffi(default = None)]
    pub provider: Option<Provider>,
    /// A model id; unset asks the provider for its newest.
    #[uniffi(default = None)]
    pub model: Option<String>,
    #[uniffi(default = None)]
    pub base_url: Option<String>,
    #[uniffi(default = None)]
    pub api_key: Option<String>,
    /// A `config.json` to read as the host's own, exactly as basis's
    /// `WorkspaceBuilder::with_config_file` does.
    #[uniffi(default = None)]
    pub config_file: Option<String>,
    /// Unset is [`History::Default`].
    #[uniffi(default = None)]
    pub history: Option<History>,
}

impl WorkspaceOptions {
    pub(crate) fn apply(self, path: PathBuf) -> WorkspaceBuilder {
        let mut runtime = RuntimeBuilder::default();
        if let Some(provider) = self.provider {
            runtime = runtime.with_provider(provider.into());
        }
        if let Some(model) = self.model {
            runtime = runtime.with_model(ModelSelector::Id(model));
        }
        if let Some(base_url) = self.base_url {
            runtime = runtime.with_base_url(base_url);
        }
        if let Some(api_key) = self.api_key {
            runtime = runtime.with_api_key(api_key);
        }
        runtime = match self.history.unwrap_or(History::Default) {
            History::Default => runtime,
            History::Directory { path } => runtime.with_store_dir(path),
            History::Ephemeral => runtime.with_ephemeral_history(),
        };

        let mut workspace = basis::Workspace::builder(path).with_runtime_builder(runtime);
        if let Some(config_file) = self.config_file {
            workspace = workspace.with_config_file(config_file);
        }
        workspace
    }
}
