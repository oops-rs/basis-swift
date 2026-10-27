/// Every failure that crosses into Swift, as a thrown error.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum BasisError {
    /// basis refused or failed the operation; `message` is its own wording.
    #[error("{message}")]
    Run { message: String },
    /// An argument the binding could not turn into a basis value.
    #[error("{message}")]
    InvalidInput { message: String },
    /// A foreign callback threw, or could not be reached.
    #[error("{message}")]
    Callback { message: String },
    /// The binding itself failed: a panicked task or a lost runtime.
    #[error("{message}")]
    Internal { message: String },
}

impl From<basis::RunError> for BasisError {
    fn from(error: basis::RunError) -> Self {
        Self::Run {
            message: error.to_string(),
        }
    }
}

impl From<uniffi::UnexpectedUniFFICallbackError> for BasisError {
    fn from(error: uniffi::UnexpectedUniFFICallbackError) -> Self {
        Self::Callback {
            message: error.reason,
        }
    }
}
