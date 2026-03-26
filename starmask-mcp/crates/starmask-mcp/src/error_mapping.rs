use rmcp::{ErrorData, model::ErrorCode};
use serde_json::json;
use thiserror::Error;

use starmask_types::{SharedError, SharedErrorCode};

#[derive(Debug, Error)]
pub enum AdapterError {
    #[error(transparent)]
    Shared(#[from] SharedError),
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    #[error("daemon transport error: {0}")]
    Transport(String),
    #[error("daemon protocol error: {0}")]
    Protocol(String),
    #[error("serialization error: {0}")]
    Serialization(String),
}

impl From<serde_json::Error> for AdapterError {
    fn from(value: serde_json::Error) -> Self {
        Self::Serialization(value.to_string())
    }
}

impl From<starmask_types::IdValidationError> for AdapterError {
    fn from(value: starmask_types::IdValidationError) -> Self {
        Self::InvalidRequest(value.to_string())
    }
}

impl From<AdapterError> for ErrorData {
    fn from(value: AdapterError) -> Self {
        match value {
            AdapterError::Shared(error) => {
                let data = Some(json!({
                    "shared_code": error.code,
                    "retryable": error.retryable,
                    "details": error.details,
                }));
                match error.code {
                    SharedErrorCode::InvalidAccount
                    | SharedErrorCode::InvalidTransactionPayload
                    | SharedErrorCode::IdempotencyKeyConflict
                    | SharedErrorCode::ProtocolVersionMismatch => {
                        ErrorData::invalid_params(error.message, data)
                    }
                    _ => ErrorData::new(ErrorCode::INTERNAL_ERROR, error.message, data),
                }
            }
            AdapterError::InvalidRequest(message) => ErrorData::invalid_params(message, None),
            AdapterError::Transport(message)
            | AdapterError::Protocol(message)
            | AdapterError::Serialization(message) => ErrorData::internal_error(message, None),
        }
    }
}
