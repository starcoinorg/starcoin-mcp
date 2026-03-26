use pretty_assertions::assert_eq;
use rmcp::{ErrorData, model::ErrorCode};
use serde_json::json;
use starmask_mcp::AdapterError;
use starmask_types::{SharedError, SharedErrorCode, WalletInstanceId};

#[test]
fn protocol_version_mismatch_maps_to_invalid_params_with_shared_code() {
    let error_data: ErrorData = AdapterError::Shared(SharedError::new(
        SharedErrorCode::ProtocolVersionMismatch,
        "unsupported protocol version",
    ))
    .into();

    assert_eq!(error_data.code, ErrorCode::INVALID_PARAMS);
    assert_eq!(error_data.message.as_ref(), "unsupported protocol version");
    assert_eq!(
        error_data.data,
        Some(json!({
            "shared_code": "protocol_version_mismatch",
            "retryable": false,
            "details": null,
        }))
    );
}

#[test]
fn wallet_selection_required_preserves_shared_code_in_internal_error() {
    let error_data: ErrorData = AdapterError::Shared(SharedError::new(
        SharedErrorCode::WalletSelectionRequired,
        "multiple wallet instances match the account",
    ))
    .into();

    assert_eq!(error_data.code, ErrorCode::INTERNAL_ERROR);
    assert_eq!(
        error_data.message.as_ref(),
        "multiple wallet instances match the account"
    );
    assert_eq!(
        error_data.data,
        Some(json!({
            "shared_code": "wallet_selection_required",
            "retryable": true,
            "details": null,
        }))
    );
}

#[test]
fn id_validation_error_maps_to_invalid_params() {
    let id_error = WalletInstanceId::new("   ").expect_err("empty id should fail validation");
    let error_data: ErrorData = AdapterError::from(id_error).into();

    assert_eq!(error_data.code, ErrorCode::INVALID_PARAMS);
    assert!(error_data.message.contains("WalletInstanceId"));
    assert!(error_data.message.contains("cannot be empty"));
    assert_eq!(error_data.data, None);
}
