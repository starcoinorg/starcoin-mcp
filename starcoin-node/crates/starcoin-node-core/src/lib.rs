#![forbid(unsafe_code)]

mod bootstrap;
mod helpers;
mod queries;
mod submission;
#[cfg(test)]
mod tests;
mod transaction;

use std::{
    collections::HashMap,
    str::FromStr,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::Context;
use serde_json::{Value, json};
use starcoin_node_rpc::NodeRpcClient;
use starcoin_node_types::{
    CallViewFunctionInput, CallViewFunctionOutput, ChainContext, ChainStatusOutput, EffectiveProbe,
    GasUnitPriceSource, GetAccountOverviewInput, GetAccountOverviewOutput, GetBlockInput,
    GetBlockOutput, GetEventsInput, GetEventsOutput, GetTransactionInput, GetTransactionOutput,
    ListBlocksInput, ListBlocksOutput, ListModulesInput, ListModulesOutput, ListResourcesInput,
    ListResourcesOutput, Mode, NextAction, NodeHealthOutput, PreparationResult,
    PrepareContractCallInput, PreparePublishPackageInput, PrepareTransferInput,
    PreparedExecutionFacts, ResolveFunctionAbiInput, ResolveModuleAbiInput, ResolveStructAbiInput,
    RuntimeConfig, SequenceNumberSource, SharedError, SharedErrorCode, SimulateRawTransactionInput,
    SimulateRawTransactionOutput, SimulationResult, SimulationStatus, SubmissionNextAction,
    SubmissionState, SubmitSignedTransactionInput, SubmitSignedTransactionOutput, TransactionKind,
    TransactionStatusSummary, WatchTransactionInput, WatchTransactionOutput,
};
use starcoin_vm2_types::view::{
    FunctionIdView, TransactionArgumentView, TransactionRequest, TypeTagView,
};
use starcoin_vm2_vm_types::{
    account_address::AccountAddress,
    account_config::core_code_address,
    identifier::Identifier,
    language_storage::{ModuleId, TypeTag},
    on_chain_resource::ChainId,
    token::{stc::G_STC_TOKEN_CODE, token_code::TokenCode},
    transaction::{
        EntryFunction, Package, RawUserTransaction, SignedUserTransaction, TransactionPayload,
    },
    transaction_argument::{TransactionArgument, convert_txn_args},
};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use tokio::{
    sync::{OwnedSemaphorePermit, RwLock, Semaphore},
    time::{sleep, timeout},
};
use tracing::warn;

#[cfg(test)]
pub(crate) use bootstrap::{enforce_transaction_head_lag, validate_transaction_probe};
pub(crate) use helpers::{
    canonical_hex_payload, canonicalize_network_name, decode_hex_bytes, encode_hex_bcs,
    ensure_capability, extract_accepted_tokens, extract_balance_resources, extract_chain_context,
    extract_network, extract_optional_string, extract_optional_u64, extract_string, extract_u8,
    extract_u64, is_degradable_sequence_lookup_error, is_transport_error, map_named_entries,
    named_resource_entry, replace_stc_balance_with_primary_store, rfc3339_now,
    status_summary_from_parts, validate_chain_identity,
};
pub(crate) use submission::ensure_transaction_mode;
#[cfg(test)]
pub(crate) use submission::{
    accepted_submission_output, effective_min_confirmed_blocks, effective_submit_timeout_seconds,
    submission_unknown_output, validate_signed_transaction_submission,
};
#[cfg(test)]
pub(crate) use transaction::build_raw_transaction;

#[derive(Debug)]
pub struct AppContext {
    config: RuntimeConfig,
    rpc: NodeRpcClient,
    watch_permits: Arc<Semaphore>,
    expensive_permits: Arc<Semaphore>,
    startup_probe: EffectiveProbe,
    transaction_probe: Arc<RwLock<Option<CachedProbe>>>,
    prepared_transactions: Arc<RwLock<HashMap<String, PreparedTransactionRecord>>>,
    unresolved_submissions: Arc<RwLock<HashMap<String, UnresolvedSubmission>>>,
}

#[derive(Debug, Clone)]
struct CachedProbe {
    probe: EffectiveProbe,
    observed_at: Instant,
}

#[derive(Debug, Clone)]
struct PreparedTransactionRecord {
    simulation_status: SimulationStatus,
    chain_context: ChainContext,
    recorded_at: Instant,
}

#[derive(Debug, Clone)]
struct UnresolvedSubmission {
    recorded_at: Instant,
}

#[derive(Debug)]
struct TransactionEndpointSnapshot {
    chain_context: ChainContext,
    now_seconds: u64,
}
