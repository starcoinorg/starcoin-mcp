use rmcp::{ErrorData, model::ErrorCode};
use serde_json::json;
use starcoin_node_mcp_types::{SharedError, SharedErrorCode};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AdapterError {
    #[error(transparent)]
    Shared(#[from] SharedError),
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    #[error("serialization error: {0}")]
    Serialization(String),
}

impl From<serde_json::Error> for AdapterError {
    fn from(value: serde_json::Error) -> Self {
        Self::Serialization(value.to_string())
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
                    SharedErrorCode::MissingSender
                    | SharedErrorCode::MissingPublicKey
                    | SharedErrorCode::InvalidPackagePayload
                    | SharedErrorCode::PayloadTooLarge
                    | SharedErrorCode::BlockNotFound
                    | SharedErrorCode::TransactionNotFound => {
                        ErrorData::invalid_params(error.message, data)
                    }
                    SharedErrorCode::UnsupportedOperation | SharedErrorCode::PermissionDenied => {
                        ErrorData::new(ErrorCode::METHOD_NOT_FOUND, error.message, data)
                    }
                    _ => ErrorData::new(ErrorCode::INTERNAL_ERROR, error.message, data),
                }
            }
            AdapterError::InvalidRequest(message) => ErrorData::invalid_params(message, None),
            AdapterError::Serialization(message) => ErrorData::internal_error(message, None),
        }
    }
}
