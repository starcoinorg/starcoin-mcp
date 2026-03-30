use serde_json::{Map, Value, json};
use starcoin_node_mcp_types::{SharedError, SharedErrorCode};

pub(crate) const PRIMARY_FUNGIBLE_STORE_STRUCT_TAG: &str =
    "0x00000000000000000000000000000001::fungible_asset::FungibleStore";

pub(crate) fn vm2_list_resources_params(
    address: &str,
    decode: bool,
    start_index: u64,
    max_size: u64,
    state_root: Option<String>,
    resource_types: &[String],
) -> Value {
    json!([
        address,
        {
            "decode": decode,
            "state_root": state_root,
            "start_index": start_index,
            "max_size": max_size,
            "resource_types": if resource_types.is_empty() { Value::Null } else { json!(resource_types) },
            "primary_fungible_store": {}
        }
    ])
}

pub(crate) fn vm2_list_code_params(
    address: &str,
    resolve: bool,
    state_root: Option<String>,
) -> Value {
    json!([
        address,
        {
            "resolve": resolve,
            "state_root": state_root,
        }
    ])
}

pub(crate) fn vm2_get_resource_params(
    address: &str,
    resource_type: &str,
    decode: bool,
    state_root: Option<String>,
    include_primary_fungible_store: bool,
) -> Value {
    json!([
        address,
        resource_type,
        {
            "decode": decode,
            "state_root": state_root,
            "primary_fungible_store": if include_primary_fungible_store {
                json!({})
            } else {
                Value::Null
            }
        }
    ])
}

pub(crate) fn validate_list_resources_response(resources: &Value) -> Result<(), SharedError> {
    match resources.get("resources") {
        Some(Value::Object(_)) | Some(Value::Array(_)) => Ok(()),
        _ => Err(SharedError::new(
            SharedErrorCode::RpcUnavailable,
            "state2.list_resource returned a malformed resources envelope",
        )),
    }
}

pub(crate) fn synthesize_account_state_from_resources(resources: &Value) -> Option<Value> {
    let sequence_number = match resources.get("resources")? {
        Value::Object(entries) => {
            if entries.is_empty() {
                return None;
            }
            extract_sequence_number_from_object_entries(entries)
        }
        Value::Array(entries) => {
            if entries.is_empty() {
                return None;
            }
            extract_sequence_number_from_array_entries(entries)
        }
        _ => return None,
    };

    let mut summary = Map::new();
    if let Some(sequence_number) = sequence_number {
        summary.insert("sequence_number".to_owned(), json!(sequence_number));
    }

    (!summary.is_empty()).then_some(Value::Object(summary))
}

fn extract_sequence_number_from_object_entries(entries: &Map<String, Value>) -> Option<u64> {
    entries
        .iter()
        .find(|(name, _)| name.contains("::account::Account"))
        .and_then(|(_, resource)| resource.get("json"))
        .and_then(|resource| resource.get("sequence_number"))
        .and_then(parse_u64)
}

fn extract_sequence_number_from_array_entries(entries: &[Value]) -> Option<u64> {
    entries
        .iter()
        .find(|resource| {
            resource
                .get("name")
                .and_then(Value::as_str)
                .map(|name| name.contains("::account::Account"))
                .unwrap_or(false)
        })
        .and_then(|resource| Some(resource.get("value").unwrap_or(resource)))
        .and_then(|resource| resource.get("json"))
        .and_then(|resource| resource.get("sequence_number"))
        .and_then(parse_u64)
}

fn parse_u64(value: &Value) -> Option<u64> {
    match value {
        Value::Number(number) => number.as_u64(),
        Value::String(string) => string.parse().ok(),
        _ => None,
    }
}
