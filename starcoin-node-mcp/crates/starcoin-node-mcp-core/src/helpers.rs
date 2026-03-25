use super::*;

pub(crate) fn ensure_capability(supported: bool, message: &'static str) -> Result<(), SharedError> {
    if supported {
        Ok(())
    } else {
        Err(SharedError::new(
            SharedErrorCode::UnsupportedOperation,
            message,
        ))
    }
}

pub(crate) fn validate_chain_identity(
    config: &RuntimeConfig,
    node_info: &Value,
    chain_info: &Value,
) -> Result<(), SharedError> {
    if let Some(expected_chain_id) = config.expected_chain_id {
        let actual_chain_id = extract_u8(chain_info, &["chain_id"])?;
        if expected_chain_id != actual_chain_id {
            return Err(SharedError::new(
                SharedErrorCode::InvalidChainContext,
                format!(
                    "configured chain_id {expected_chain_id} does not match endpoint chain_id {actual_chain_id}"
                ),
            ));
        }
    }
    if let Some(expected_network) = &config.expected_network {
        let actual_network = extract_network(node_info)?;
        if !canonicalize_network_name(expected_network)
            .eq_ignore_ascii_case(&canonicalize_network_name(&actual_network))
        {
            return Err(SharedError::new(
                SharedErrorCode::InvalidChainContext,
                format!(
                    "configured network {expected_network} does not match endpoint network {actual_network}"
                ),
            ));
        }
    }
    if config.require_genesis_hash_match {
        if let Some(expected_genesis_hash) = &config.expected_genesis_hash {
            let actual_genesis_hash = extract_string(chain_info, &["genesis_hash"])?;
            if expected_genesis_hash != &actual_genesis_hash {
                return Err(SharedError::new(
                    SharedErrorCode::InvalidChainContext,
                    format!(
                        "configured genesis_hash {expected_genesis_hash} does not match endpoint genesis_hash {actual_genesis_hash}"
                    ),
                ));
            }
        }
    }
    Ok(())
}

pub(crate) fn extract_chain_context(
    node_info: &Value,
    chain_info: &Value,
) -> Result<ChainContext, SharedError> {
    Ok(ChainContext {
        chain_id: extract_u8(chain_info, &["chain_id"])?,
        network: extract_network(node_info)?,
        genesis_hash: extract_string(chain_info, &["genesis_hash"])?,
        head_block_hash: extract_string(chain_info, &["head", "block_hash"])?,
        head_block_number: extract_u64(chain_info, &["head", "number"])?,
        head_state_root: extract_optional_string(chain_info, &["head", "state_root"]),
        observed_at: rfc3339_now()?,
    })
}

pub(crate) fn extract_network(node_info: &Value) -> Result<String, SharedError> {
    parse_network_name(lookup(node_info, &["net"])).ok_or_else(|| {
        SharedError::new(
            SharedErrorCode::RpcUnavailable,
            "missing or invalid network field at net",
        )
    })
}

pub(crate) fn extract_string(value: &Value, path: &[&str]) -> Result<String, SharedError> {
    extract_optional_string(value, path).ok_or_else(|| {
        SharedError::new(
            SharedErrorCode::RpcUnavailable,
            format!("missing string field at {}", path.join(".")),
        )
    })
}

pub(crate) fn extract_optional_string(value: &Value, path: &[&str]) -> Option<String> {
    lookup(value, path).and_then(|value| match value {
        Value::String(string) => Some(string.clone()),
        Value::Number(number) => Some(number.to_string()),
        other => Some(other.to_string()),
    })
}

pub(crate) fn extract_u64(value: &Value, path: &[&str]) -> Result<u64, SharedError> {
    extract_optional_u64(value, path).ok_or_else(|| {
        SharedError::new(
            SharedErrorCode::RpcUnavailable,
            format!("missing numeric field at {}", path.join(".")),
        )
    })
}

pub(crate) fn extract_optional_u64(value: &Value, path: &[&str]) -> Option<u64> {
    lookup(value, path).and_then(|value| match value {
        Value::Number(number) => number.as_u64(),
        Value::String(string) => string.parse().ok(),
        _ => None,
    })
}

pub(crate) fn extract_u8(value: &Value, path: &[&str]) -> Result<u8, SharedError> {
    extract_u64(value, path).and_then(|value| {
        u8::try_from(value).map_err(|_| {
            SharedError::new(
                SharedErrorCode::RpcUnavailable,
                format!("numeric field at {} does not fit into u8", path.join(".")),
            )
        })
    })
}

pub(crate) fn lookup<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut cursor = value;
    for segment in path {
        cursor = cursor.get(*segment)?;
    }
    Some(cursor)
}

pub(crate) fn parse_network_name(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(name) => Some(canonicalize_network_name(name)),
        Value::Object(object) => {
            if let Some(name) = object.get("Builtin").and_then(Value::as_str) {
                return Some(canonicalize_network_name(name));
            }
            if let Some(custom) = object.get("Custom") {
                if let Some(name) = custom.get("chain_name").and_then(Value::as_str) {
                    return Some(canonicalize_network_name(name));
                }
            }
            None
        }
        _ => None,
    }
}

pub(crate) fn canonicalize_network_name(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

pub(crate) fn status_summary_from_parts(
    transaction: Option<&Value>,
    transaction_info: Option<&Value>,
) -> TransactionStatusSummary {
    let found = transaction.is_some() || transaction_info.is_some();
    let confirmed = transaction_info.is_some();
    let vm_status = transaction_info
        .and_then(|info| extract_optional_string(info, &["status"]))
        .or_else(|| transaction.and_then(|txn| extract_optional_string(txn, &["status"])));
    let gas_used = transaction_info.and_then(|info| extract_optional_u64(info, &["gas_used"]));
    TransactionStatusSummary {
        found,
        confirmed,
        vm_status,
        gas_used,
    }
}

pub(crate) fn is_terminal_watch_status(summary: &TransactionStatusSummary) -> bool {
    summary.confirmed
}

pub(crate) fn map_named_entries(container: &Value, field: &str) -> Vec<Value> {
    container
        .get(field)
        .and_then(Value::as_object)
        .map(|entries| {
            entries
                .iter()
                .map(|(name, value)| json!({ "name": name, "value": value }))
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn extract_balances_and_tokens(
    resources: &[Value],
    balances: &mut Vec<Value>,
    accepted_tokens: &mut Vec<String>,
) {
    for resource in resources {
        let Some(name) = resource.get("name").and_then(Value::as_str) else {
            continue;
        };
        if name.contains("Balance") || name.contains("balance") {
            balances.push(resource.clone());
        }
        if name.contains("Token") || name.contains("token") {
            accepted_tokens.push(name.to_owned());
        }
    }
}

pub(crate) fn is_transport_error(error: &SharedError) -> bool {
    matches!(
        error.code,
        SharedErrorCode::NodeUnavailable | SharedErrorCode::RpcUnavailable
    )
}

pub(crate) fn decode_hex_bytes(input: &str) -> Result<Vec<u8>, SharedError> {
    hex::decode(input.trim_start_matches("0x")).map_err(|error| {
        SharedError::new(
            SharedErrorCode::InvalidPackagePayload,
            format!("invalid hex payload: {error}"),
        )
    })
}

pub(crate) fn canonical_hex_payload(input: &str) -> Result<String, SharedError> {
    decode_hex_bytes(input).map(hex::encode)
}

pub(crate) fn encode_hex_bcs<T: serde::Serialize>(value: &T) -> Result<String, SharedError> {
    let bytes = bcs_ext::to_bytes(value).map_err(|error| {
        SharedError::new(
            SharedErrorCode::RpcUnavailable,
            format!("failed to bcs-encode transaction: {error}"),
        )
    })?;
    Ok(hex::encode(bytes))
}

pub(crate) fn rfc3339_now() -> Result<String, SharedError> {
    OffsetDateTime::now_utc().format(&Rfc3339).map_err(|error| {
        SharedError::new(
            SharedErrorCode::RpcUnavailable,
            format!("failed to format current timestamp: {error}"),
        )
    })
}
