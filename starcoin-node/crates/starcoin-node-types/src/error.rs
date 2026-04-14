use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SharedErrorCode {
    NodeUnavailable,
    RpcUnavailable,
    InvalidChainContext,
    SubmissionUnknown,
    SimulationFailed,
    SubmissionFailed,
    TransactionExpired,
    SequenceNumberStale,
    PermissionDenied,
    ApprovalRequired,
    RateLimited,
    PayloadTooLarge,
    UnsupportedOperation,
    MissingSender,
    MissingPublicKey,
    InvalidAddress,
    InvalidAsset,
    InvalidAmount,
    InvalidPackagePayload,
    BlockNotFound,
    TransactionNotFound,
}

#[derive(Clone, Debug, Deserialize, Error, JsonSchema, Serialize)]
#[error("{message}")]
pub struct SharedError {
    pub code: SharedErrorCode,
    pub message: String,
    pub retryable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

impl SharedError {
    pub fn new(code: SharedErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            retryable: false,
            details: None,
        }
    }

    pub fn retryable(code: SharedErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            retryable: true,
            details: None,
        }
    }

    pub fn with_details(mut self, details: Value) -> Self {
        self.details = Some(details);
        self
    }
}
