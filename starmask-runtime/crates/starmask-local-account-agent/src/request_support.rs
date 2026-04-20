use std::{convert::TryFrom, fs, io::Write, path::Path, str::FromStr, time::Duration};

use anyhow::{Context, Result};
use starcoin_account::AccountManager;
use starcoin_account_api::{AccountInfo, AccountPrivateKey};
use starcoin_crypto::{ValidCryptoMaterial, ValidCryptoMaterialStringExt};
use starcoin_types::{
    account_address::AccountAddress, sign_message::SigningMessage, transaction::RawUserTransaction,
};
use starmask_types::{
    BackendAccount, Curve, MessageFormat, PulledRequest, RejectReasonCode, RequestKind,
    RequestResult, WalletCapability,
};
use tempfile::NamedTempFile;

#[derive(Clone, Debug)]
pub(crate) struct RequestRejection {
    pub(crate) reason_code: RejectReasonCode,
    pub(crate) message: Option<String>,
}

pub(crate) fn account_info_to_backend_account(account: AccountInfo) -> BackendAccount {
    BackendAccount {
        address: account.address.to_string(),
        label: None,
        public_key: Some(format!(
            "0x{}",
            hex::encode(account.public_key.public_key_bytes())
        )),
        is_default: account.is_default,
        is_read_only: account.is_readonly,
        is_locked: account.is_locked,
    }
}

pub(crate) fn ensure_local_unlock_capability(
    account_locked: bool,
    capabilities: &[WalletCapability],
) -> std::result::Result<(), RequestRejection> {
    if account_locked && !capabilities.contains(&WalletCapability::Unlock) {
        return Err(RequestRejection {
            reason_code: RejectReasonCode::WalletLocked,
            message: Some("Local account is locked".to_owned()),
        });
    }
    Ok(())
}

pub(crate) fn fulfill_request(
    manager: &AccountManager,
    unlock_cache_ttl: Duration,
    request: &PulledRequest,
    account_address: AccountAddress,
    account_info: &AccountInfo,
    capabilities: &[WalletCapability],
    password: Option<&str>,
) -> std::result::Result<RequestResult, RequestRejection> {
    if account_info.is_locked {
        ensure_local_unlock_capability(account_info.is_locked, capabilities)?;
        let Some(password) = password else {
            return Err(RequestRejection {
                reason_code: RejectReasonCode::WalletLocked,
                message: Some("Local account is locked".to_owned()),
            });
        };
        manager
            .unlock_account(account_address, password, unlock_cache_ttl)
            .map_err(|error| RequestRejection {
                reason_code: RejectReasonCode::WalletLocked,
                message: Some(format!("Failed to unlock local account: {error}")),
            })?;
    }

    match request.kind {
        RequestKind::SignTransaction => sign_transaction(manager, request, account_address),
        RequestKind::SignMessage => sign_message(manager, request, account_address),
        RequestKind::ExportAccount => export_account(manager, request, account_address, password),
        RequestKind::CreateAccount => Err(RequestRejection {
            reason_code: RejectReasonCode::UnsupportedOperation,
            message: Some(
                "CreateAccount requests must use the dedicated create-account flow".to_owned(),
            ),
        }),
        RequestKind::ImportAccount => Err(RequestRejection {
            reason_code: RejectReasonCode::UnsupportedOperation,
            message: Some(
                "ImportAccount requests must use the dedicated import-account flow".to_owned(),
            ),
        }),
    }
}

pub(crate) fn create_account(
    manager: &AccountManager,
    password: &str,
) -> std::result::Result<RequestResult, RequestRejection> {
    if password.is_empty() {
        return Err(RequestRejection {
            reason_code: RejectReasonCode::BackendPolicyBlocked,
            message: Some("Account password cannot be empty".to_owned()),
        });
    }
    let account = manager
        .create_account(password)
        .map_err(|error| RequestRejection {
            reason_code: RejectReasonCode::BackendUnavailable,
            message: Some(format!("Failed to create local account: {error}")),
        })?;
    let account_info = manager
        .account_info(*account.address())
        .map_err(|error| RequestRejection {
            reason_code: RejectReasonCode::BackendUnavailable,
            message: Some(format!("Failed to load created account: {error}")),
        })?
        .ok_or_else(|| RequestRejection {
            reason_code: RejectReasonCode::BackendUnavailable,
            message: Some("Created account was not visible after creation".to_owned()),
        })?;

    Ok(RequestResult::CreatedAccount {
        address: account_info.address.to_string(),
        public_key: format!(
            "0x{}",
            hex::encode(account_info.public_key.public_key_bytes())
        ),
        curve: Curve::Ed25519,
        is_default: account_info.is_default,
        is_locked: account_info.is_locked,
    })
}

pub(crate) fn import_account(
    manager: &AccountManager,
    request: &PulledRequest,
    password: &str,
) -> std::result::Result<RequestResult, RequestRejection> {
    if password.is_empty() {
        return Err(RequestRejection {
            reason_code: RejectReasonCode::BackendPolicyBlocked,
            message: Some("Account password cannot be empty".to_owned()),
        });
    }
    let private_key_file = request
        .private_key_file
        .as_deref()
        .ok_or_else(|| RequestRejection {
            reason_code: RejectReasonCode::BackendPolicyBlocked,
            message: Some("Missing private key file for account import".to_owned()),
        })?;
    let private_key_data =
        std::fs::read_to_string(private_key_file).map_err(|error| RequestRejection {
            reason_code: RejectReasonCode::BackendUnavailable,
            message: Some(format!("Failed to read private key file: {error}")),
        })?;
    let private_key =
        AccountPrivateKey::from_encoded_string(private_key_data.trim()).map_err(|error| {
            RequestRejection {
                reason_code: RejectReasonCode::BackendPolicyBlocked,
                message: Some(format!("Invalid private key file: {error}")),
            }
        })?;
    let derived_address = private_key.public_key().derived_address();
    if request.account_address.trim().is_empty() {
        return Err(RequestRejection {
            reason_code: RejectReasonCode::BackendPolicyBlocked,
            message: Some("Import request is missing the expected account address".to_owned()),
        });
    }
    let address =
        parse_account_address(&request.account_address).map_err(|error| RequestRejection {
            reason_code: RejectReasonCode::BackendPolicyBlocked,
            message: Some(error.to_string()),
        })?;
    if address != derived_address {
        return Err(RequestRejection {
            reason_code: RejectReasonCode::BackendPolicyBlocked,
            message: Some(
                "Requested import address does not match the private key's derived address"
                    .to_owned(),
            ),
        });
    }

    let account = manager
        .import_account(address, private_key.to_bytes().to_vec(), password)
        .map_err(|error| RequestRejection {
            reason_code: RejectReasonCode::BackendUnavailable,
            message: Some(format!("Failed to import local account: {error}")),
        })?;
    let account_info = manager
        .account_info(*account.address())
        .map_err(|error| RequestRejection {
            reason_code: RejectReasonCode::BackendUnavailable,
            message: Some(format!("Failed to load imported account: {error}")),
        })?
        .ok_or_else(|| RequestRejection {
            reason_code: RejectReasonCode::BackendUnavailable,
            message: Some("Imported account was not visible after import".to_owned()),
        })?;

    Ok(RequestResult::ImportedAccount {
        address: account_info.address.to_string(),
        public_key: format!(
            "0x{}",
            hex::encode(account_info.public_key.public_key_bytes())
        ),
        curve: Curve::Ed25519,
        is_default: account_info.is_default,
        is_locked: account_info.is_locked,
    })
}

fn export_account(
    manager: &AccountManager,
    request: &PulledRequest,
    address: AccountAddress,
    password: Option<&str>,
) -> std::result::Result<RequestResult, RequestRejection> {
    let Some(password) = password else {
        return Err(RequestRejection {
            reason_code: RejectReasonCode::WalletLocked,
            message: Some("Account password is required for export".to_owned()),
        });
    };
    let output_file = request
        .output_file
        .as_deref()
        .ok_or_else(|| RequestRejection {
            reason_code: RejectReasonCode::BackendPolicyBlocked,
            message: Some("Missing output file for account export".to_owned()),
        })?;
    let private_key_bytes =
        manager
            .export_account(address, password)
            .map_err(|error| RequestRejection {
                reason_code: RejectReasonCode::WalletLocked,
                message: Some(format!("Failed to export local account: {error}")),
            })?;
    if private_key_bytes.is_empty() {
        return Err(RequestRejection {
            reason_code: RejectReasonCode::UnsupportedOperation,
            message: Some("Account has no exportable private key; it may be read-only".to_owned()),
        });
    }

    let private_key =
        AccountPrivateKey::try_from(private_key_bytes.as_slice()).map_err(|error| {
            RequestRejection {
                reason_code: RejectReasonCode::BackendUnavailable,
                message: Some(format!("Exported account private key is invalid: {error}")),
            }
        })?;
    let encoded = private_key
        .to_encoded_string()
        .map_err(|error| RequestRejection {
            reason_code: RejectReasonCode::BackendUnavailable,
            message: Some(format!("Failed to encode account private key: {error}")),
        })?;
    write_private_key_file(Path::new(output_file), &encoded, request.force)?;

    Ok(RequestResult::ExportedAccount {
        address: address.to_string(),
        output_file: output_file.to_owned(),
    })
}

fn write_private_key_file(
    path: &Path,
    encoded: &str,
    force: bool,
) -> std::result::Result<(), RequestRejection> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| RequestRejection {
            reason_code: RejectReasonCode::BackendUnavailable,
            message: Some(format!(
                "Failed to create output directory {}: {error}",
                parent.display()
            )),
        })?;
    }
    if path.exists() && !force {
        return Err(RequestRejection {
            reason_code: RejectReasonCode::BackendPolicyBlocked,
            message: Some(format!(
                "Output file already exists: {}; pass --force to overwrite it",
                path.display()
            )),
        });
    }

    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let mut temp_file = NamedTempFile::new_in(parent).map_err(|error| RequestRejection {
        reason_code: RejectReasonCode::BackendUnavailable,
        message: Some(format!(
            "Failed to create temporary export file in {}: {error}",
            parent.display()
        )),
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        temp_file
            .as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|error| RequestRejection {
                reason_code: RejectReasonCode::BackendUnavailable,
                message: Some(format!(
                    "Failed to restrict permissions on temporary export file in {}: {error}",
                    parent.display()
                )),
            })?;
    }
    temp_file
        .write_all(encoded.as_bytes())
        .and_then(|_| temp_file.write_all(b"\n"))
        .and_then(|_| temp_file.as_file().sync_all())
        .map_err(|error| RequestRejection {
            reason_code: RejectReasonCode::BackendUnavailable,
            message: Some(format!(
                "Failed to write account private-key export: {error}"
            )),
        })?;

    if force {
        #[cfg(windows)]
        if path.exists() {
            fs::remove_file(path).map_err(|error| RequestRejection {
                reason_code: RejectReasonCode::BackendUnavailable,
                message: Some(format!(
                    "Failed to replace existing output file {}: {error}",
                    path.display()
                )),
            })?;
        }
        temp_file.persist(path).map_err(|error| RequestRejection {
            reason_code: RejectReasonCode::BackendUnavailable,
            message: Some(format!(
                "Failed to move account private-key export into {}: {}",
                path.display(),
                error.error
            )),
        })?;
        return Ok(());
    }

    temp_file.persist_noclobber(path).map_err(|error| {
        let reason_code = if error.error.kind() == std::io::ErrorKind::AlreadyExists {
            RejectReasonCode::BackendPolicyBlocked
        } else {
            RejectReasonCode::BackendUnavailable
        };
        let message = if error.error.kind() == std::io::ErrorKind::AlreadyExists {
            format!(
                "Output file already exists: {}; pass --force to overwrite it",
                path.display()
            )
        } else {
            format!(
                "Failed to move account private-key export into {}: {}",
                path.display(),
                error.error
            )
        };
        RequestRejection {
            reason_code,
            message: Some(message),
        }
    })?;
    Ok(())
}

fn sign_transaction(
    manager: &AccountManager,
    request: &PulledRequest,
    address: AccountAddress,
) -> std::result::Result<RequestResult, RequestRejection> {
    let raw_txn_hex = request
        .raw_txn_bcs_hex
        .as_deref()
        .ok_or_else(|| RequestRejection {
            reason_code: RejectReasonCode::InvalidTransactionPayload,
            message: Some("Missing raw transaction payload".to_owned()),
        })?;
    let raw_txn_bytes = decode_hex_bytes(raw_txn_hex).map_err(|error| RequestRejection {
        reason_code: RejectReasonCode::InvalidTransactionPayload,
        message: Some(error),
    })?;
    let raw_txn: RawUserTransaction =
        bcs_ext::from_bytes(&raw_txn_bytes).map_err(|error| RequestRejection {
            reason_code: RejectReasonCode::InvalidTransactionPayload,
            message: Some(format!("Invalid raw transaction payload: {error}")),
        })?;
    if raw_txn.sender() != address {
        return Err(RequestRejection {
            reason_code: RejectReasonCode::InvalidTransactionPayload,
            message: Some("Raw transaction sender does not match request account".to_owned()),
        });
    }

    let signed_txn = manager
        .sign_txn(address, raw_txn)
        .map_err(|error| RequestRejection {
            reason_code: RejectReasonCode::BackendUnavailable,
            message: Some(format!("Failed to sign transaction: {error}")),
        })?;
    let signed_txn_bytes = bcs_ext::to_bytes(&signed_txn).map_err(|error| RequestRejection {
        reason_code: RejectReasonCode::BackendUnavailable,
        message: Some(format!("Failed to serialize signed transaction: {error}")),
    })?;
    Ok(RequestResult::SignedTransaction {
        signed_txn_bcs_hex: format!("0x{}", hex::encode(signed_txn_bytes)),
    })
}

fn sign_message(
    manager: &AccountManager,
    request: &PulledRequest,
    address: AccountAddress,
) -> std::result::Result<RequestResult, RequestRejection> {
    let message = request.message.as_deref().ok_or_else(|| RequestRejection {
        reason_code: RejectReasonCode::InvalidMessagePayload,
        message: Some("Missing message payload".to_owned()),
    })?;
    let format = request.message_format.ok_or_else(|| RequestRejection {
        reason_code: RejectReasonCode::InvalidMessagePayload,
        message: Some("Missing message format".to_owned()),
    })?;
    let signing_message =
        decode_signing_message(message, format).map_err(|error| RequestRejection {
            reason_code: RejectReasonCode::InvalidMessagePayload,
            message: Some(error),
        })?;
    let signed_message = manager
        .sign_message(address, signing_message)
        .map_err(|error| RequestRejection {
            reason_code: RejectReasonCode::BackendUnavailable,
            message: Some(format!("Failed to sign message: {error}")),
        })?;
    Ok(RequestResult::SignedMessage {
        signature: signed_message.to_string(),
    })
}

fn decode_hex_bytes(input: &str) -> std::result::Result<Vec<u8>, String> {
    let trimmed = input.strip_prefix("0x").unwrap_or(input);
    hex::decode(trimmed).map_err(|error| format!("invalid hex payload: {error}"))
}

pub(crate) fn decode_signing_message(
    message: &str,
    format: MessageFormat,
) -> std::result::Result<SigningMessage, String> {
    match format {
        MessageFormat::Utf8 => Ok(SigningMessage::from(message.as_bytes().to_vec())),
        MessageFormat::Hex => decode_hex_bytes(message).map(SigningMessage::from),
    }
}

pub(crate) fn parse_account_address(account_address: &str) -> Result<AccountAddress> {
    AccountAddress::from_str(account_address)
        .with_context(|| format!("invalid account address {account_address}"))
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use starcoin_account::{AccountManager, account_storage::AccountStorage};
    use starcoin_config::RocksdbConfig;
    use starcoin_types::genesis_config::ChainId;
    use tempfile::tempdir;

    use super::{
        account_info_to_backend_account, decode_signing_message, ensure_local_unlock_capability,
        write_private_key_file,
    };
    use starmask_types::{MessageFormat, WalletCapability};

    #[test]
    fn decode_signing_message_accepts_utf8_and_hex() {
        let utf8 = decode_signing_message("hello", MessageFormat::Utf8).unwrap();
        assert_eq!(utf8.to_string(), "0x68656c6c6f");

        let hex = decode_signing_message("0x010203", MessageFormat::Hex).unwrap();
        assert_eq!(hex.to_string(), "0x010203");
    }

    #[test]
    fn backend_account_uses_prefixed_public_key_hex() {
        let tempdir = tempdir().unwrap();
        let storage =
            AccountStorage::create_from_path(tempdir.path(), RocksdbConfig::default()).unwrap();
        let manager = AccountManager::new(storage, ChainId::test()).unwrap();
        let account = manager.create_account("hello").unwrap();
        let info = manager.account_info(*account.address()).unwrap().unwrap();
        let backend = account_info_to_backend_account(info);

        assert!(backend.public_key.unwrap().starts_with("0x"));
    }

    #[test]
    fn locked_accounts_fail_closed_without_unlock_capability() {
        let error = ensure_local_unlock_capability(
            true,
            &[
                WalletCapability::GetPublicKey,
                WalletCapability::SignMessage,
                WalletCapability::SignTransaction,
            ],
        )
        .unwrap_err();

        assert_eq!(
            error.reason_code,
            starmask_types::RejectReasonCode::WalletLocked
        );
        assert_eq!(error.message.as_deref(), Some("Local account is locked"));
    }

    #[test]
    fn unlocked_accounts_do_not_require_unlock_capability() {
        ensure_local_unlock_capability(
            false,
            &[
                WalletCapability::GetPublicKey,
                WalletCapability::SignMessage,
                WalletCapability::SignTransaction,
            ],
        )
        .unwrap();
    }

    #[test]
    fn write_private_key_file_creates_private_output_with_expected_contents() {
        let tempdir = tempdir().unwrap();
        let path = tempdir.path().join("exports/account.key");

        write_private_key_file(&path, "private-key-data", false).unwrap();

        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "private-key-data\n"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(path.metadata().unwrap().permissions().mode() & 0o777, 0o600);
        }
    }
}
