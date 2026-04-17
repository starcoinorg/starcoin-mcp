#![forbid(unsafe_code)]

use std::{
    collections::HashMap,
    time::{SystemTime, UNIX_EPOCH},
};

use rusqlite::{Connection, OptionalExtension, params};
use serde::de::DeserializeOwned;

use starmask_core::RepositoryError;
use starmask_types::{BackendKind, WalletAccountRecord, WalletInstanceId};

pub(crate) fn is_local_account_wallet(
    connection: &Connection,
    wallet_instance_id: &WalletInstanceId,
) -> Result<bool, RepositoryError> {
    let backend_kind = connection
        .query_row(
            "SELECT backend_kind FROM wallet_instances WHERE wallet_instance_id = ?1",
            params![wallet_instance_id.as_str()],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(sql_error)?
        .ok_or_else(|| {
            RepositoryError::Storage(format!(
                "wallet instance {} was not found",
                wallet_instance_id
            ))
        })?;
    Ok(
        decode_string_enum::<BackendKind>(&backend_kind).map_err(sql_error)?
            == BackendKind::LocalAccountDir,
    )
}

pub(crate) fn ensure_local_account_labels(
    connection: &Connection,
    wallet_instance_id: &WalletInstanceId,
    accounts: &[WalletAccountRecord],
) -> Result<(), RepositoryError> {
    let known_labels = list_wallet_account_labels(connection, wallet_instance_id)?;
    let mut next_order = next_account_order(connection, wallet_instance_id)?;
    let updated_at = current_timestamp_ms();

    for account in accounts {
        if known_labels.contains_key(&account.address) {
            continue;
        }
        let label = discovered_account_label(account.label.as_deref(), next_order)?;
        upsert_wallet_account_label(
            connection,
            wallet_instance_id,
            &account.address,
            &label,
            next_order,
            updated_at,
        )?;
        next_order += 1;
    }

    Ok(())
}

pub(crate) fn next_account_order(
    connection: &Connection,
    wallet_instance_id: &WalletInstanceId,
) -> Result<i64, RepositoryError> {
    let max_order = connection
        .query_row(
            "SELECT MAX(account_order) FROM wallet_account_labels WHERE wallet_instance_id = ?1",
            params![wallet_instance_id.as_str()],
            |row| row.get::<_, Option<i64>>(0),
        )
        .map_err(sql_error)?
        .unwrap_or(0);
    Ok(max_order + 1)
}

pub(crate) fn existing_account_order(
    connection: &Connection,
    wallet_instance_id: &WalletInstanceId,
    address: &str,
) -> Result<Option<i64>, RepositoryError> {
    connection
        .query_row(
            "SELECT account_order
               FROM wallet_account_labels
              WHERE wallet_instance_id = ?1 AND address = ?2",
            params![wallet_instance_id.as_str(), address],
            |row| row.get(0),
        )
        .optional()
        .map_err(sql_error)
}

pub(crate) fn upsert_wallet_account_label(
    connection: &Connection,
    wallet_instance_id: &WalletInstanceId,
    address: &str,
    label: &str,
    account_order: i64,
    updated_at: i64,
) -> Result<(), RepositoryError> {
    connection
        .execute(
            "INSERT INTO wallet_account_labels (
                wallet_instance_id, address, label, account_order, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(wallet_instance_id, address) DO UPDATE SET
                label = excluded.label,
                account_order = excluded.account_order,
                updated_at = excluded.updated_at",
            params![
                wallet_instance_id.as_str(),
                address,
                label,
                account_order,
                updated_at
            ],
        )
        .map_err(sql_error)?;
    Ok(())
}

pub(crate) fn normalize_account_label(label: &str) -> Result<String, RepositoryError> {
    let normalized = label.trim();
    if normalized.is_empty() {
        return Err(RepositoryError::Storage(
            "account label cannot be empty".to_owned(),
        ));
    }
    Ok(normalized.to_owned())
}

pub(crate) fn current_timestamp_ms() -> i64 {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    i64::try_from(millis).unwrap_or(i64::MAX)
}

fn list_wallet_account_labels(
    connection: &Connection,
    wallet_instance_id: &WalletInstanceId,
) -> Result<HashMap<String, String>, RepositoryError> {
    let mut statement = connection
        .prepare(
            "SELECT address, label
               FROM wallet_account_labels
              WHERE wallet_instance_id = ?1",
        )
        .map_err(sql_error)?;
    let rows = statement
        .query_map(params![wallet_instance_id.as_str()], |row| {
            Ok((
                row.get::<_, String>("address")?,
                row.get::<_, String>("label")?,
            ))
        })
        .map_err(sql_error)?;
    let labels = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(sql_error)?
        .into_iter()
        .collect();
    Ok(labels)
}

fn discovered_account_label(
    label: Option<&str>,
    next_order: i64,
) -> Result<String, RepositoryError> {
    match label.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) => normalize_account_label(value),
        None => Ok(format!("account-{next_order}")),
    }
}

fn decode_string_enum<T: DeserializeOwned>(value: &str) -> rusqlite::Result<T> {
    serde_json::from_value(serde_json::Value::String(value.to_owned())).map_err(from_other)
}

fn sql_error(error: rusqlite::Error) -> RepositoryError {
    RepositoryError::Storage(error.to_string())
}

fn from_other<E: std::fmt::Display>(error: E) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        0,
        rusqlite::types::Type::Text,
        Box::<dyn std::error::Error + Send + Sync>::from(error.to_string()),
    )
}
