use std::{fs, path::PathBuf};

use serde_json::json;
use starcoin_node_types::{
    ChainContext, GasUnitPriceSource, NextAction, PreparationResult, PreparedExecutionFacts,
    SequenceNumberSource, SimulationResult, SimulationStatus, TransactionKind,
};

#[test]
fn preparation_result_matches_shared_unsigned_envelope_contract() {
    let schema_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../shared/schemas/unsigned-transaction-envelope.schema.json");
    let schema: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(schema_path).expect("schema should exist"))
            .expect("schema json should parse");

    let envelope = PreparationResult {
        transaction_kind: TransactionKind::Transfer,
        raw_txn_bcs_hex: "deadbeef".to_owned(),
        raw_txn: json!({ "sender": "0x1" }),
        transaction_summary: json!({ "receiver": "0x2", "amount": "1" }),
        chain_context: ChainContext {
            chain_id: 254,
            network: "main".to_owned(),
            genesis_hash: "0x1".to_owned(),
            head_block_hash: "0x2".to_owned(),
            head_block_number: 42,
            head_state_root: Some("0x3".to_owned()),
            observed_at: "2026-03-25T00:00:00Z".to_owned(),
        },
        prepared_at: "2026-03-25T00:00:05Z".to_owned(),
        sequence_number_source: SequenceNumberSource::Onchain,
        gas_unit_price_source: GasUnitPriceSource::Txpool,
        simulation_status: SimulationStatus::Performed,
        simulation: Some(SimulationResult {
            executed: true,
            vm_status: "Executed".to_owned(),
            gas_used: 7,
            events: vec![json!({ "type_tag": "0x1::Account::TransferEvent" })],
            write_set_summary: vec![json!({ "address": "0x1" })],
            raw: json!({ "status": "Executed" }),
        }),
        execution_facts: PreparedExecutionFacts {
            sender: "0x1".to_owned(),
            sequence_number: 7,
            max_gas_amount: 10_000_000,
            gas_unit_price: 1,
            gas_token_code: "0x1::STC::STC".to_owned(),
            expiration_timestamp_secs: 123,
            chain_id: 254,
            estimated_max_network_fee: "10000000".to_owned(),
            estimated_network_fee: Some("7".to_owned()),
            transfer_receiver: Some("0x2".to_owned()),
            transfer_amount: Some("1".to_owned()),
            transfer_token_code: Some("0x1::STC::STC".to_owned()),
        },
        next_action: NextAction::SignTransaction,
    };
    let serialized = serde_json::to_value(&envelope).expect("envelope should serialize");

    for key in schema["required"]
        .as_array()
        .expect("required should be an array")
    {
        let field = key.as_str().expect("required field should be a string");
        assert!(
            serialized.get(field).is_some(),
            "serialized envelope is missing required field {field}"
        );
    }

    let simulation_enum = schema["properties"]["simulation_status"]["enum"]
        .as_array()
        .expect("simulation_status enum should be an array")
        .iter()
        .map(|value| value.as_str().expect("enum value should be a string"))
        .collect::<Vec<_>>();
    assert_eq!(
        simulation_enum,
        vec![
            "performed",
            "skipped_missing_public_key",
            "skipped_by_policy",
            "failed",
        ]
    );
    assert_eq!(serialized["simulation_status"], "performed");
    assert_eq!(serialized["next_action"], "sign_transaction");
    assert_eq!(serialized["chain_context"]["chain_id"], 254);
    assert_eq!(serialized["prepared_at"], "2026-03-25T00:00:05Z");
    assert_eq!(serialized["execution_facts"]["sequence_number"], 7);
    assert_eq!(serialized["execution_facts"]["estimated_network_fee"], "7");
}
