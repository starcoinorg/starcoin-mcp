use serde::{Deserialize, Serialize};

use crate::lifecycle::{Curve, MessageFormat, ResultKind};

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct TransactionPayload {
    pub chain_id: u64,
    pub raw_txn_bcs_hex: String,
    pub tx_kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_hint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_context: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct MessagePayload {
    pub message: String,
    pub format: MessageFormat,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_hint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_context: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct CreateAccountPayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_hint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_context: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(tag = "kind", content = "payload", rename_all = "snake_case")]
pub enum RequestPayload {
    SignTransaction(TransactionPayload),
    SignMessage(MessagePayload),
    CreateAccount(CreateAccountPayload),
}

impl RequestPayload {
    pub fn result_kind(&self) -> ResultKind {
        match self {
            Self::SignTransaction(_) => ResultKind::SignedTransaction,
            Self::SignMessage(_) => ResultKind::SignedMessage,
            Self::CreateAccount(_) => ResultKind::CreatedAccount,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RequestResult {
    SignedTransaction {
        signed_txn_bcs_hex: String,
    },
    SignedMessage {
        signature: String,
    },
    CreatedAccount {
        address: String,
        public_key: String,
        curve: Curve,
        is_default: bool,
        is_locked: bool,
    },
}

impl RequestResult {
    pub fn result_kind(&self) -> ResultKind {
        match self {
            Self::SignedTransaction { .. } => ResultKind::SignedTransaction,
            Self::SignedMessage { .. } => ResultKind::SignedMessage,
            Self::CreatedAccount { .. } => ResultKind::CreatedAccount,
        }
    }
}
