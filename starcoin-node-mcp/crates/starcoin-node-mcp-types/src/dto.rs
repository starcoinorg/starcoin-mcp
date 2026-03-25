use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::{
    ChainContext, GasUnitPriceSource, NextAction, SequenceNumberSource, SimulationStatus,
    SubmissionNextAction, SubmissionState, TransactionKind,
};

fn bool_true() -> bool {
    true
}

#[derive(Clone, Debug, Default, Deserialize, JsonSchema, Serialize)]
pub struct EmptyParams {}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct GetBlockInput {
    #[serde(default)]
    pub block_hash: Option<String>,
    #[serde(default)]
    pub block_number: Option<u64>,
    #[serde(default = "bool_true")]
    pub decode: bool,
    #[serde(default)]
    pub include_raw: bool,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct ListBlocksInput {
    #[serde(default)]
    pub from_block_number: Option<u64>,
    pub count: u64,
    #[serde(default = "bool_true")]
    pub reverse: bool,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct GetTransactionInput {
    pub txn_hash: String,
    #[serde(default = "bool_true")]
    pub include_events: bool,
    #[serde(default = "bool_true")]
    pub decode: bool,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct WatchTransactionInput {
    pub txn_hash: String,
    #[serde(default)]
    pub timeout_seconds: Option<u64>,
    #[serde(default)]
    pub poll_interval_seconds: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct GetEventsInput {
    #[serde(default)]
    pub from_block: Option<u64>,
    #[serde(default)]
    pub to_block: Option<u64>,
    #[serde(default)]
    pub event_keys: Vec<String>,
    #[serde(default)]
    pub addresses: Vec<String>,
    #[serde(default)]
    pub type_tags: Vec<String>,
    pub limit: u64,
    #[serde(default = "bool_true")]
    pub decode: bool,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct GetAccountOverviewInput {
    pub address: String,
    #[serde(default)]
    pub include_resources: bool,
    #[serde(default)]
    pub include_modules: bool,
    #[serde(default)]
    pub resource_limit: Option<u64>,
    #[serde(default)]
    pub module_limit: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct ListResourcesInput {
    pub address: String,
    #[serde(default)]
    pub resource_type: Option<String>,
    #[serde(default)]
    pub start_index: Option<u64>,
    #[serde(default)]
    pub max_size: Option<u64>,
    #[serde(default = "bool_true")]
    pub decode: bool,
    #[serde(default)]
    pub block_number: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct ListModulesInput {
    pub address: String,
    #[serde(default = "bool_true")]
    pub resolve_abi: bool,
    #[serde(default)]
    pub max_size: Option<u64>,
    #[serde(default)]
    pub block_number: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct ResolveFunctionAbiInput {
    pub function_id: String,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct ResolveStructAbiInput {
    pub struct_tag: String,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct ResolveModuleAbiInput {
    pub module_id: String,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct CallViewFunctionInput {
    pub function_id: String,
    #[serde(default)]
    pub type_args: Vec<String>,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct PrepareTransferInput {
    pub sender: String,
    #[serde(default)]
    pub sender_public_key: Option<String>,
    pub receiver: String,
    pub amount: String,
    #[serde(default)]
    pub token_code: Option<String>,
    #[serde(default)]
    pub sequence_number: Option<u64>,
    #[serde(default)]
    pub max_gas_amount: Option<u64>,
    #[serde(default)]
    pub gas_unit_price: Option<u64>,
    #[serde(default)]
    pub expiration_time_secs: Option<u64>,
    #[serde(default)]
    pub gas_token: Option<String>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct PrepareContractCallInput {
    pub sender: String,
    #[serde(default)]
    pub sender_public_key: Option<String>,
    pub function_id: String,
    #[serde(default)]
    pub type_args: Vec<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub sequence_number: Option<u64>,
    #[serde(default)]
    pub max_gas_amount: Option<u64>,
    #[serde(default)]
    pub gas_unit_price: Option<u64>,
    #[serde(default)]
    pub expiration_time_secs: Option<u64>,
    #[serde(default)]
    pub gas_token: Option<String>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct PreparePublishPackageInput {
    pub sender: String,
    #[serde(default)]
    pub sender_public_key: Option<String>,
    pub package_bcs_hex: String,
    #[serde(default)]
    pub sequence_number: Option<u64>,
    #[serde(default)]
    pub max_gas_amount: Option<u64>,
    #[serde(default)]
    pub gas_unit_price: Option<u64>,
    #[serde(default)]
    pub expiration_time_secs: Option<u64>,
    #[serde(default)]
    pub gas_token: Option<String>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct SimulateRawTransactionInput {
    pub raw_txn_bcs_hex: String,
    pub sender_public_key: String,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct SubmitSignedTransactionInput {
    pub signed_txn_bcs_hex: String,
    pub prepared_chain_context: ChainContext,
    #[serde(default)]
    pub blocking: bool,
    #[serde(default)]
    pub timeout_seconds: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct ChainStatusOutput {
    pub network: String,
    pub chain_id: u8,
    pub genesis_hash: String,
    pub head_block_number: u64,
    pub head_block_hash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub head_state_root: Option<String>,
    pub now_seconds: u64,
    pub peer_count: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sync_status: Option<Value>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct NodeHealthOutput {
    pub node_available: bool,
    pub node_info: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sync: Option<Value>,
    pub peers_summary: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub txpool_summary: Option<Value>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct GetBlockOutput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block: Option<Value>,
    pub source: Value,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct ListBlocksOutput {
    pub blocks: Vec<Value>,
    pub effective_count: u64,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct TransactionStatusSummary {
    pub found: bool,
    pub confirmed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vm_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gas_used: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct GetTransactionOutput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transaction: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transaction_info: Option<Value>,
    pub events: Vec<Value>,
    pub status_summary: TransactionStatusSummary,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct WatchTransactionOutput {
    pub txn_hash: String,
    pub found: bool,
    pub confirmed: bool,
    pub effective_timeout_seconds: u64,
    pub effective_poll_interval_seconds: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transaction_info: Option<Value>,
    pub events: Vec<Value>,
    pub status_summary: TransactionStatusSummary,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct GetEventsOutput {
    pub events: Vec<Value>,
    pub matched_count: u64,
    pub effective_limit: u64,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct GetAccountOverviewOutput {
    pub address: String,
    pub onchain_exists: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sequence_number: Option<u64>,
    pub balances: Vec<Value>,
    pub accepted_tokens: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<Vec<Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modules: Option<Vec<Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub applied_resource_limit: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub applied_module_limit: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_sequence_number_hint: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct ListResourcesOutput {
    pub address: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state_root: Option<String>,
    pub resources: Vec<Value>,
    pub effective_max_size: u64,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct ListModulesOutput {
    pub address: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state_root: Option<String>,
    pub modules: Vec<Value>,
    pub effective_max_size: u64,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct ResolveAbiOutput {
    #[serde(flatten)]
    pub inner: Value,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct CallViewFunctionOutput {
    pub return_values: Vec<Value>,
    pub decoded_return_values: Vec<Value>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct SimulationResult {
    pub executed: bool,
    pub vm_status: String,
    pub gas_used: u64,
    pub events: Vec<Value>,
    pub write_set_summary: Vec<Value>,
    pub raw: Value,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct PreparationResult {
    pub transaction_kind: TransactionKind,
    pub raw_txn_bcs_hex: String,
    pub raw_txn: Value,
    pub transaction_summary: Value,
    pub chain_context: ChainContext,
    pub prepared_at: String,
    pub sequence_number_source: SequenceNumberSource,
    pub gas_unit_price_source: GasUnitPriceSource,
    pub simulation_status: SimulationStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub simulation: Option<SimulationResult>,
    pub next_action: NextAction,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct SimulateRawTransactionOutput {
    pub simulation: Value,
    pub executed: bool,
    pub vm_status: String,
    pub gas_used: u64,
    pub events: Vec<Value>,
    pub write_set_summary: Vec<Value>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct SubmitSignedTransactionOutput {
    pub txn_hash: String,
    pub submission_state: SubmissionState,
    pub submitted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effective_timeout_seconds: Option<u64>,
    pub next_action: SubmissionNextAction,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub watch_result: Option<WatchTransactionOutput>,
}
