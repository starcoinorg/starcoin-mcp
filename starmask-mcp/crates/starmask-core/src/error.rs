use thiserror::Error;

use starmask_types::{RequestStatus, SharedError, SharedErrorCode};

use crate::repo::RepositoryError;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error(transparent)]
    Shared(#[from] SharedError),
    #[error(transparent)]
    Repository(#[from] RepositoryError),
    #[error("invalid state transition from {from:?} to {to:?}")]
    InvalidStateTransition {
        from: RequestStatus,
        to: RequestStatus,
    },
    #[error("validation error: {0}")]
    Validation(String),
    #[error("internal invariant violated: {0}")]
    Invariant(String),
}

impl CoreError {
    pub fn shared(code: SharedErrorCode, message: impl Into<String>) -> Self {
        Self::Shared(SharedError::new(code, message))
    }
}

pub type CoreResult<T> = Result<T, CoreError>;
