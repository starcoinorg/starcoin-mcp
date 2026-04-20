use std::fmt::{self, Display, Formatter};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum SharedErrorCode {
    WalletUnavailable,
    WalletLocked,
    WalletSelectionRequired,
    WalletInstanceNotFound,
    RequestNotOwned,
    LeaseMismatch,
    ExtensionNotConnected,
    BackendNotAllowed,
    InvalidBackendRegistration,
    BackendUnavailable,
    BackendPolicyBlocked,
    InvalidAccount,
    RequestNotFound,
    RequestExpired,
    RequestRejected,
    RequestCancelled,
    InvalidTransactionPayload,
    InvalidMessagePayload,
    UnsupportedChain,
    InvalidRequest,
    InternalBridgeError,
    ResultUnavailable,
    IdempotencyKeyConflict,
    ProtocolVersionMismatch,
    NodeUnavailable,
    RpcUnavailable,
    InvalidChainContext,
    SimulationFailed,
    SubmissionFailed,
    PermissionDenied,
    ApprovalRequired,
    RateLimited,
    UnsupportedOperation,
}

impl SharedErrorCode {
    pub fn retryable_by_default(self) -> bool {
        matches!(
            self,
            Self::WalletUnavailable
                | Self::WalletSelectionRequired
                | Self::ExtensionNotConnected
                | Self::BackendUnavailable
                | Self::InternalBridgeError
                | Self::NodeUnavailable
                | Self::RpcUnavailable
                | Self::RateLimited
        )
    }
}

impl Display for SharedErrorCode {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let value = serde_json::to_value(self).map_err(|_| fmt::Error)?;
        match value {
            Value::String(text) => f.write_str(&text),
            _ => Err(fmt::Error),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct SharedError {
    pub code: SharedErrorCode,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retryable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

impl SharedError {
    pub fn new(code: SharedErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            retryable: Some(code.retryable_by_default()),
            details: None,
        }
    }

    pub fn with_retryable(mut self, retryable: bool) -> Self {
        self.retryable = Some(retryable);
        self
    }

    pub fn with_details(mut self, details: Value) -> Self {
        self.details = Some(details);
        self
    }
}

impl Display for SharedError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for SharedError {}

#[derive(Debug, Error)]
pub enum IdValidationError {
    #[error("{kind} cannot be empty")]
    Empty { kind: &'static str },
}
