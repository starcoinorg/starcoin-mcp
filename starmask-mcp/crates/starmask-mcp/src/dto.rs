use rmcp::schemars;
use rmcp::schemars::JsonSchema;
use serde::Deserialize;

use starmask_types::{DurationSeconds, MessageFormat, WalletInstanceId};

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct EmptyParams {}

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct WalletListAccountsInput {
    pub(crate) wallet_instance_id: Option<String>,
    #[serde(default)]
    pub(crate) include_public_key: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WalletGetPublicKeyInput {
    pub(crate) address: String,
    pub(crate) wallet_instance_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WalletRequestSignTransactionInput {
    pub(crate) client_request_id: String,
    pub(crate) account_address: String,
    pub(crate) wallet_instance_id: Option<String>,
    pub(crate) chain_id: u64,
    pub(crate) raw_txn_bcs_hex: String,
    pub(crate) tx_kind: String,
    pub(crate) display_hint: Option<String>,
    pub(crate) client_context: Option<String>,
    pub(crate) ttl_seconds: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WalletGetRequestStatusInput {
    pub(crate) request_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WalletCancelRequestInput {
    pub(crate) request_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WalletSignMessageInput {
    pub(crate) client_request_id: String,
    pub(crate) account_address: String,
    pub(crate) wallet_instance_id: Option<String>,
    pub(crate) message: String,
    pub(crate) format: MessageFormatInput,
    pub(crate) display_hint: Option<String>,
    pub(crate) client_context: Option<String>,
    pub(crate) ttl_seconds: Option<u64>,
}

impl WalletListAccountsInput {
    pub fn wallet_instance_id(
        &self,
    ) -> Result<Option<WalletInstanceId>, starmask_types::IdValidationError> {
        self.wallet_instance_id
            .as_ref()
            .map(|value| WalletInstanceId::new(value.clone()))
            .transpose()
    }
}

impl WalletGetPublicKeyInput {
    pub fn wallet_instance_id(
        &self,
    ) -> Result<Option<WalletInstanceId>, starmask_types::IdValidationError> {
        self.wallet_instance_id
            .as_ref()
            .map(|value| WalletInstanceId::new(value.clone()))
            .transpose()
    }
}

impl WalletRequestSignTransactionInput {
    pub fn wallet_instance_id(
        &self,
    ) -> Result<Option<WalletInstanceId>, starmask_types::IdValidationError> {
        self.wallet_instance_id
            .as_ref()
            .map(|value| WalletInstanceId::new(value.clone()))
            .transpose()
    }

    pub fn ttl_seconds(&self) -> Option<DurationSeconds> {
        self.ttl_seconds.map(DurationSeconds::new)
    }
}

impl WalletSignMessageInput {
    pub fn wallet_instance_id(
        &self,
    ) -> Result<Option<WalletInstanceId>, starmask_types::IdValidationError> {
        self.wallet_instance_id
            .as_ref()
            .map(|value| WalletInstanceId::new(value.clone()))
            .transpose()
    }

    pub fn ttl_seconds(&self) -> Option<DurationSeconds> {
        self.ttl_seconds.map(DurationSeconds::new)
    }
}
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MessageFormatInput {
    Utf8,
    Hex,
}

impl From<MessageFormatInput> for MessageFormat {
    fn from(value: MessageFormatInput) -> Self {
        match value {
            MessageFormatInput::Utf8 => MessageFormat::Utf8,
            MessageFormatInput::Hex => MessageFormat::Hex,
        }
    }
}
