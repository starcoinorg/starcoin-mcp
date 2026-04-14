use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum RequestKind {
    SignTransaction,
    SignMessage,
    CreateAccount,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum RequestStatus {
    Created,
    Dispatched,
    PendingUserApproval,
    Approved,
    Rejected,
    Expired,
    Cancelled,
    Failed,
}

impl RequestStatus {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Approved | Self::Rejected | Self::Expired | Self::Cancelled | Self::Failed
        )
    }

    pub fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Created, Self::Dispatched)
                | (Self::Created, Self::Cancelled)
                | (Self::Created, Self::Expired)
                | (Self::Created, Self::Failed)
                | (Self::Dispatched, Self::Created)
                | (Self::Dispatched, Self::PendingUserApproval)
                | (Self::Dispatched, Self::Cancelled)
                | (Self::Dispatched, Self::Expired)
                | (Self::Dispatched, Self::Failed)
                | (Self::PendingUserApproval, Self::Created)
                | (Self::PendingUserApproval, Self::Approved)
                | (Self::PendingUserApproval, Self::Rejected)
                | (Self::PendingUserApproval, Self::Cancelled)
                | (Self::PendingUserApproval, Self::Expired)
                | (Self::PendingUserApproval, Self::Failed)
        )
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ResultKind {
    SignedTransaction,
    SignedMessage,
    CreatedAccount,
    None,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum LockState {
    Locked,
    Unlocked,
    Unknown,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "snake_case")]
pub enum BackendKind {
    StarmaskExtension,
    LocalAccountDir,
    PrivateKeyDev,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "snake_case")]
pub enum TransportKind {
    NativeMessaging,
    LocalSocket,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalSurface {
    BrowserUi,
    TtyPrompt,
    DesktopPrompt,
    None,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "snake_case")]
pub enum WalletCapability {
    Unlock,
    GetPublicKey,
    SignMessage,
    SignTransaction,
    CreateAccount,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Channel {
    Development,
    Staging,
    Production,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum RejectReasonCode {
    RequestRejected,
    WalletLocked,
    RequestExpired,
    UnsupportedOperation,
    InvalidTransactionPayload,
    InvalidMessagePayload,
    BackendUnavailable,
    BackendPolicyBlocked,
    InternalError,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Curve {
    Ed25519,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum MessageFormat {
    Utf8,
    Hex,
}

impl RequestKind {
    pub fn expected_result_kind(self) -> ResultKind {
        match self {
            Self::SignTransaction => ResultKind::SignedTransaction,
            Self::SignMessage => ResultKind::SignedMessage,
            Self::CreateAccount => ResultKind::CreatedAccount,
        }
    }
}
