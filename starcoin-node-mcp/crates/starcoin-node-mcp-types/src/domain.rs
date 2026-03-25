use clap::ValueEnum;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[value(name = "read_only", alias = "read-only")]
    ReadOnly,
    #[value(name = "transaction")]
    Transaction,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum VmProfile {
    #[value(name = "auto")]
    Auto,
    #[value(name = "vm2_only", alias = "vm2-only")]
    Vm2Only,
    #[value(name = "legacy_compatible", alias = "legacy-compatible")]
    LegacyCompatible,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
pub struct ChainContext {
    pub chain_id: u8,
    pub network: String,
    pub genesis_hash: String,
    pub head_block_hash: String,
    pub head_block_number: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub head_state_root: Option<String>,
    pub observed_at: String,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
pub struct EffectiveProbe {
    pub supports_node_info: bool,
    pub supports_chain_info: bool,
    pub supports_block_lookup: bool,
    pub supports_block_listing: bool,
    pub supports_transaction_lookup: bool,
    pub supports_transaction_info_lookup: bool,
    pub supports_transaction_events_by_hash: bool,
    pub supports_account_state_lookup: bool,
    pub supports_events_query: bool,
    pub supports_resource_listing: bool,
    pub supports_module_listing: bool,
    pub supports_abi_resolution: bool,
    pub supports_view_call: bool,
    pub supports_transaction_submission: bool,
    pub supports_raw_dry_run: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TransactionKind {
    Transfer,
    ContractCall,
    PublishPackage,
    Unknown,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SimulationStatus {
    Performed,
    SkippedMissingPublicKey,
    SkippedByPolicy,
    Failed,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NextAction {
    SignTransaction,
    GetPublicKeyThenSimulateOrSign,
    SimulateThenSign,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SubmissionState {
    Accepted,
    Unknown,
    Rejected,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SubmissionNextAction {
    WatchTransaction,
    ReconcileByTxnHash,
    ReprepareThenResign,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SequenceNumberSource {
    Caller,
    Onchain,
    Txpool,
    MaxOfOnchainAndTxpool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GasUnitPriceSource {
    Caller,
    Txpool,
    DefaultConfig,
}
