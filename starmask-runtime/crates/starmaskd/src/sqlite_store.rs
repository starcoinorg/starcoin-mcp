use std::{fs, path::Path};

use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::{Serialize, de::DeserializeOwned};

use crate::{
    account_labels::{
        current_timestamp_ms, ensure_local_account_labels, existing_account_order,
        is_local_account_wallet, next_account_order, normalize_account_label,
        upsert_wallet_account_label,
    },
    schema::ensure_current_schema,
};
use starmask_core::{RepositoryError, RequestRepository, WalletRepository};
use starmask_types::{
    ClientRequestId, DeliveryLease, DeliveryLeaseId, PresentationId, RequestId, RequestRecord,
    TimestampMs, WalletAccountRecord, WalletInstanceId, WalletInstanceRecord,
};

pub use crate::schema::SCHEMA_VERSION;

pub struct SqliteStore {
    connection: Connection,
}

impl SqliteStore {
    pub fn open(path: &Path) -> Result<Self, RepositoryError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(io_error)?;
        }
        let mut connection = Connection::open(path).map_err(sql_error)?;
        connection
            .execute_batch(
                "PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 5000; PRAGMA synchronous = NORMAL;",
            )
            .map_err(sql_error)?;
        ensure_current_schema(&mut connection)?;
        Ok(Self { connection })
    }

    pub fn schema_version(&self) -> Result<u32, RepositoryError> {
        let version: i64 = self
            .connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(sql_error)?;
        u32::try_from(version).map_err(|error| RepositoryError::Storage(error.to_string()))
    }
}

impl RequestRepository for SqliteStore {
    fn get_request(
        &mut self,
        request_id: &RequestId,
    ) -> Result<Option<RequestRecord>, RepositoryError> {
        let mut statement = self
            .connection
            .prepare("SELECT * FROM requests WHERE request_id = ?1")
            .map_err(sql_error)?;
        statement
            .query_row(params![request_id.as_str()], read_request)
            .optional()
            .map_err(sql_error)
    }

    fn get_request_by_client_request_id(
        &mut self,
        client_request_id: &ClientRequestId,
    ) -> Result<Option<RequestRecord>, RepositoryError> {
        let mut statement = self
            .connection
            .prepare("SELECT * FROM requests WHERE client_request_id = ?1")
            .map_err(sql_error)?;
        statement
            .query_row(params![client_request_id.as_str()], read_request)
            .optional()
            .map_err(sql_error)
    }

    fn insert_request(&mut self, request: RequestRecord) -> Result<RequestRecord, RepositoryError> {
        write_request(&self.connection, &request, true)?;
        Ok(request)
    }

    fn update_request(&mut self, request: RequestRecord) -> Result<RequestRecord, RepositoryError> {
        write_request(&self.connection, &request, false)?;
        Ok(request)
    }

    fn claim_next_request_for_wallet(
        &mut self,
        wallet_instance_id: &WalletInstanceId,
        delivery_lease: DeliveryLease,
        now: TimestampMs,
    ) -> Result<Option<RequestRecord>, RepositoryError> {
        let transaction = self.connection.transaction().map_err(sql_error)?;
        let request_id: Option<String> = transaction
            .query_row(
                "SELECT request_id FROM requests WHERE wallet_instance_id = ?1 AND status = 'created' ORDER BY created_at ASC LIMIT 1",
                params![wallet_instance_id.as_str()],
                |row| row.get(0),
            )
            .optional()
            .map_err(sql_error)?;

        let Some(request_id) = request_id else {
            transaction.commit().map_err(sql_error)?;
            return Ok(None);
        };

        transaction
            .execute(
                "UPDATE requests
                 SET status = 'dispatched',
                     updated_at = ?2,
                     delivery_lease_id = ?3,
                     delivery_lease_expires_at = ?4
                 WHERE request_id = ?1",
                params![
                    request_id,
                    now.as_millis(),
                    delivery_lease.delivery_lease_id.as_str(),
                    delivery_lease.delivery_lease_expires_at.as_millis()
                ],
            )
            .map_err(sql_error)?;

        let request = transaction
            .query_row(
                "SELECT * FROM requests WHERE request_id = ?1",
                params![request_id],
                read_request,
            )
            .map_err(sql_error)?;
        transaction.commit().map_err(sql_error)?;
        Ok(Some(request))
    }

    fn list_non_terminal_requests(&mut self) -> Result<Vec<RequestRecord>, RepositoryError> {
        query_requests(
            &self.connection,
            "SELECT * FROM requests WHERE status IN ('created', 'dispatched', 'pending_user_approval') ORDER BY created_at ASC",
            params![],
        )
    }

    fn list_terminal_requests_with_expired_results(
        &mut self,
        now: TimestampMs,
    ) -> Result<Vec<RequestRecord>, RepositoryError> {
        query_requests(
            &self.connection,
            "SELECT * FROM requests WHERE result_expires_at IS NOT NULL AND result_expires_at <= ?1",
            params![now.as_millis()],
        )
    }
}

impl WalletRepository for SqliteStore {
    fn get_wallet_instance(
        &mut self,
        wallet_instance_id: &WalletInstanceId,
    ) -> Result<Option<WalletInstanceRecord>, RepositoryError> {
        let mut statement = self
            .connection
            .prepare("SELECT * FROM wallet_instances WHERE wallet_instance_id = ?1")
            .map_err(sql_error)?;
        statement
            .query_row(params![wallet_instance_id.as_str()], read_wallet_instance)
            .optional()
            .map_err(sql_error)
    }

    fn upsert_wallet_instance(
        &mut self,
        wallet_instance: WalletInstanceRecord,
    ) -> Result<(), RepositoryError> {
        self.connection
            .execute(
                "INSERT INTO wallet_instances (
                    wallet_instance_id, backend_kind, transport_kind, approval_surface,
                    instance_label, extension_id, extension_version, protocol_version,
                    capabilities_json, backend_metadata_json, profile_hint, lock_state,
                    connected, last_seen_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
                 ON CONFLICT(wallet_instance_id) DO UPDATE SET
                    backend_kind = excluded.backend_kind,
                    transport_kind = excluded.transport_kind,
                    approval_surface = excluded.approval_surface,
                    instance_label = excluded.instance_label,
                    extension_id = excluded.extension_id,
                    extension_version = excluded.extension_version,
                    protocol_version = excluded.protocol_version,
                    capabilities_json = excluded.capabilities_json,
                    backend_metadata_json = excluded.backend_metadata_json,
                    profile_hint = excluded.profile_hint,
                    lock_state = excluded.lock_state,
                    connected = excluded.connected,
                    last_seen_at = excluded.last_seen_at",
                params![
                    wallet_instance.wallet_instance_id.as_str(),
                    encode_string_enum(wallet_instance.backend_kind)?,
                    encode_string_enum(wallet_instance.transport_kind)?,
                    encode_string_enum(wallet_instance.approval_surface)?,
                    wallet_instance.instance_label,
                    wallet_instance.extension_id,
                    wallet_instance.extension_version,
                    wallet_instance.protocol_version,
                    encode_json(&wallet_instance.capabilities)?,
                    encode_json(&wallet_instance.backend_metadata)?,
                    wallet_instance.profile_hint,
                    encode_string_enum(wallet_instance.lock_state)?,
                    bool_to_int(wallet_instance.connected),
                    wallet_instance.last_seen_at.as_millis(),
                ],
            )
            .map_err(sql_error)?;
        Ok(())
    }

    fn list_wallet_instances(
        &mut self,
        connected_only: bool,
    ) -> Result<Vec<WalletInstanceRecord>, RepositoryError> {
        let sql = if connected_only {
            "SELECT * FROM wallet_instances WHERE connected = 1 ORDER BY last_seen_at DESC"
        } else {
            "SELECT * FROM wallet_instances ORDER BY last_seen_at DESC"
        };
        let mut statement = self.connection.prepare(sql).map_err(sql_error)?;
        let rows = statement
            .query_map(params![], read_wallet_instance)
            .map_err(sql_error)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(sql_error)
    }

    fn replace_wallet_accounts(
        &mut self,
        wallet_instance_id: &WalletInstanceId,
        accounts: Vec<WalletAccountRecord>,
    ) -> Result<(), RepositoryError> {
        let transaction = self.connection.transaction().map_err(sql_error)?;
        if is_local_account_wallet(&transaction, wallet_instance_id)? {
            ensure_local_account_labels(&transaction, wallet_instance_id, &accounts)?;
        }
        transaction
            .execute(
                "DELETE FROM wallet_accounts WHERE wallet_instance_id = ?1",
                params![wallet_instance_id.as_str()],
            )
            .map_err(sql_error)?;
        for account in accounts {
            transaction
                .execute(
                    "INSERT INTO wallet_accounts (
                        wallet_instance_id, address, label, public_key, is_default, is_read_only, is_locked, last_seen_at
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![
                        account.wallet_instance_id.as_str(),
                        account.address,
                        account.label,
                        account.public_key,
                        bool_to_int(account.is_default),
                        bool_to_int(account.is_read_only),
                        bool_to_int(account.is_locked),
                        account.last_seen_at.as_millis(),
                    ],
                )
                .map_err(sql_error)?;
        }
        transaction.commit().map_err(sql_error)?;
        Ok(())
    }

    fn list_wallet_accounts(
        &mut self,
        wallet_instance_id: Option<&WalletInstanceId>,
    ) -> Result<Vec<WalletAccountRecord>, RepositoryError> {
        if let Some(wallet_instance_id) = wallet_instance_id {
            query_wallet_accounts(
                &self.connection,
                " WHERE wallet_accounts.wallet_instance_id = ?1
                  ORDER BY COALESCE(wallet_account_labels.account_order, 9223372036854775807) ASC,
                           wallet_accounts.address ASC",
                params![wallet_instance_id.as_str()],
            )
        } else {
            query_wallet_accounts(
                &self.connection,
                " ORDER BY wallet_accounts.wallet_instance_id ASC,
                           COALESCE(wallet_account_labels.account_order, 9223372036854775807) ASC,
                           wallet_accounts.address ASC",
                params![],
            )
        }
    }

    fn get_wallet_account(
        &mut self,
        wallet_instance_id: &WalletInstanceId,
        address: &str,
    ) -> Result<Option<WalletAccountRecord>, RepositoryError> {
        query_wallet_account(
            &self.connection,
            " WHERE wallet_accounts.wallet_instance_id = ?1 AND wallet_accounts.address = ?2",
            params![wallet_instance_id.as_str(), address],
        )
    }

    fn set_wallet_account_label(
        &mut self,
        wallet_instance_id: &WalletInstanceId,
        address: &str,
        label: &str,
    ) -> Result<WalletAccountRecord, RepositoryError> {
        let transaction = self.connection.transaction().map_err(sql_error)?;
        if !is_local_account_wallet(&transaction, wallet_instance_id)? {
            return Err(RepositoryError::Storage(format!(
                "wallet instance {} does not support daemon-managed account labels",
                wallet_instance_id
            )));
        }

        let mut account = query_wallet_account(
            &transaction,
            " WHERE wallet_accounts.wallet_instance_id = ?1 AND wallet_accounts.address = ?2",
            params![wallet_instance_id.as_str(), address],
        )?
        .ok_or_else(|| {
            RepositoryError::Storage(format!(
                "wallet account {}:{} was not found",
                wallet_instance_id, address
            ))
        })?;
        let normalized_label = normalize_account_label(label)?;
        let account_order = match existing_account_order(&transaction, wallet_instance_id, address)?
        {
            Some(account_order) => account_order,
            None => next_account_order(&transaction, wallet_instance_id)?,
        };
        upsert_wallet_account_label(
            &transaction,
            wallet_instance_id,
            address,
            &normalized_label,
            account_order,
            current_timestamp_ms(),
        )?;
        account.label = Some(normalized_label);
        transaction.commit().map_err(sql_error)?;
        Ok(account)
    }
}

fn query_requests<P>(
    connection: &Connection,
    sql: &str,
    params: P,
) -> Result<Vec<RequestRecord>, RepositoryError>
where
    P: rusqlite::Params,
{
    let mut statement = connection.prepare(sql).map_err(sql_error)?;
    let rows = statement
        .query_map(params, read_request)
        .map_err(sql_error)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(sql_error)
}

fn query_wallet_accounts<P>(
    connection: &Connection,
    sql_tail: &str,
    params: P,
) -> Result<Vec<WalletAccountRecord>, RepositoryError>
where
    P: rusqlite::Params,
{
    let sql = wallet_account_query(sql_tail);
    let mut statement = connection.prepare(&sql).map_err(sql_error)?;
    let rows = statement
        .query_map(params, read_wallet_account)
        .map_err(sql_error)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(sql_error)
}

fn query_wallet_account<P>(
    connection: &Connection,
    sql_tail: &str,
    params: P,
) -> Result<Option<WalletAccountRecord>, RepositoryError>
where
    P: rusqlite::Params,
{
    let sql = wallet_account_query(sql_tail);
    connection
        .query_row(&sql, params, read_wallet_account)
        .optional()
        .map_err(sql_error)
}

fn wallet_account_query(sql_tail: &str) -> String {
    format!(
        "SELECT
             wallet_accounts.wallet_instance_id,
             wallet_accounts.address,
             COALESCE(wallet_account_labels.label, wallet_accounts.label) AS label,
             wallet_accounts.public_key,
             wallet_accounts.is_default,
             wallet_accounts.is_read_only,
             wallet_accounts.is_locked,
             wallet_accounts.last_seen_at
           FROM wallet_accounts
      LEFT JOIN wallet_account_labels
             ON wallet_account_labels.wallet_instance_id = wallet_accounts.wallet_instance_id
            AND wallet_account_labels.address = wallet_accounts.address{sql_tail}"
    )
}

fn write_request(
    connection: &Connection,
    request: &RequestRecord,
    insert: bool,
) -> Result<(), RepositoryError> {
    let sql = if insert {
        "INSERT INTO requests (
            request_id, client_request_id, kind, status, wallet_instance_id, account_address,
            payload_hash, payload_json, result_json, created_at, expires_at, updated_at,
            approved_at, rejected_at, cancelled_at, failed_at, result_expires_at,
            last_error_code, last_error_message, reject_reason_code,
            delivery_lease_id, delivery_lease_expires_at, presentation_id, presentation_expires_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24)"
    } else {
        "UPDATE requests SET
            client_request_id = ?2,
            kind = ?3,
            status = ?4,
            wallet_instance_id = ?5,
            account_address = ?6,
            payload_hash = ?7,
            payload_json = ?8,
            result_json = ?9,
            created_at = ?10,
            expires_at = ?11,
            updated_at = ?12,
            approved_at = ?13,
            rejected_at = ?14,
            cancelled_at = ?15,
            failed_at = ?16,
            result_expires_at = ?17,
            last_error_code = ?18,
            last_error_message = ?19,
            reject_reason_code = ?20,
            delivery_lease_id = ?21,
            delivery_lease_expires_at = ?22,
            presentation_id = ?23,
            presentation_expires_at = ?24
         WHERE request_id = ?1"
    };

    connection
        .execute(
            sql,
            params![
                request.request_id.as_str(),
                request.client_request_id.as_str(),
                encode_string_enum(request.kind)?,
                encode_string_enum(request.status)?,
                request.wallet_instance_id.as_str(),
                request.account_address,
                request.payload_hash.as_str(),
                encode_json(&request.payload)?,
                encode_optional_json(request.result.as_ref())?,
                request.created_at.as_millis(),
                request.expires_at.as_millis(),
                request.updated_at.as_millis(),
                option_timestamp(request.approved_at),
                option_timestamp(request.rejected_at),
                option_timestamp(request.cancelled_at),
                option_timestamp(request.failed_at),
                option_timestamp(request.result_expires_at),
                encode_optional_string_enum(request.last_error_code)?,
                request.last_error_message,
                encode_optional_string_enum(request.reject_reason_code)?,
                request
                    .delivery_lease
                    .as_ref()
                    .map(|lease| lease.delivery_lease_id.as_str().to_owned()),
                request
                    .delivery_lease
                    .as_ref()
                    .map(|lease| lease.delivery_lease_expires_at.as_millis()),
                request
                    .presentation
                    .as_ref()
                    .map(|presentation| presentation.presentation_id.as_str().to_owned()),
                request
                    .presentation
                    .as_ref()
                    .map(|presentation| presentation.presentation_expires_at.as_millis()),
            ],
        )
        .map_err(sql_error)?;
    Ok(())
}

fn read_request(row: &Row<'_>) -> rusqlite::Result<RequestRecord> {
    let delivery_lease_id: Option<String> = row.get("delivery_lease_id")?;
    let delivery_lease_expires_at: Option<i64> = row.get("delivery_lease_expires_at")?;
    let presentation_id: Option<String> = row.get("presentation_id")?;
    let presentation_expires_at: Option<i64> = row.get("presentation_expires_at")?;

    Ok(RequestRecord {
        request_id: read_id(row, "request_id")?,
        client_request_id: read_id(row, "client_request_id")?,
        kind: decode_string_enum(&row.get::<_, String>("kind")?)?,
        status: decode_string_enum(&row.get::<_, String>("status")?)?,
        wallet_instance_id: read_id(row, "wallet_instance_id")?,
        account_address: row.get("account_address")?,
        payload_hash: read_id(row, "payload_hash")?,
        payload: decode_json(&row.get::<_, String>("payload_json")?)?,
        result: row
            .get::<_, Option<String>>("result_json")?
            .map(|value| decode_json(&value))
            .transpose()?,
        created_at: TimestampMs::from_millis(row.get("created_at")?),
        expires_at: TimestampMs::from_millis(row.get("expires_at")?),
        updated_at: TimestampMs::from_millis(row.get("updated_at")?),
        approved_at: row
            .get::<_, Option<i64>>("approved_at")?
            .map(TimestampMs::from_millis),
        rejected_at: row
            .get::<_, Option<i64>>("rejected_at")?
            .map(TimestampMs::from_millis),
        cancelled_at: row
            .get::<_, Option<i64>>("cancelled_at")?
            .map(TimestampMs::from_millis),
        failed_at: row
            .get::<_, Option<i64>>("failed_at")?
            .map(TimestampMs::from_millis),
        result_expires_at: row
            .get::<_, Option<i64>>("result_expires_at")?
            .map(TimestampMs::from_millis),
        last_error_code: row
            .get::<_, Option<String>>("last_error_code")?
            .map(|value| decode_string_enum(&value))
            .transpose()?,
        last_error_message: row.get("last_error_message")?,
        reject_reason_code: row
            .get::<_, Option<String>>("reject_reason_code")?
            .map(|value| decode_string_enum(&value))
            .transpose()?,
        delivery_lease: match (delivery_lease_id, delivery_lease_expires_at) {
            (Some(delivery_lease_id), Some(expires_at)) => Some(DeliveryLease {
                delivery_lease_id: DeliveryLeaseId::new(delivery_lease_id).map_err(from_other)?,
                delivery_lease_expires_at: TimestampMs::from_millis(expires_at),
            }),
            _ => None,
        },
        presentation: match (presentation_id, presentation_expires_at) {
            (Some(presentation_id), Some(expires_at)) => Some(starmask_types::PresentationLease {
                presentation_id: PresentationId::new(presentation_id).map_err(from_other)?,
                presentation_expires_at: TimestampMs::from_millis(expires_at),
            }),
            _ => None,
        },
    })
}

fn read_wallet_instance(row: &Row<'_>) -> rusqlite::Result<WalletInstanceRecord> {
    Ok(WalletInstanceRecord {
        wallet_instance_id: read_id(row, "wallet_instance_id")?,
        backend_kind: decode_string_enum(&row.get::<_, String>("backend_kind")?)?,
        transport_kind: decode_string_enum(&row.get::<_, String>("transport_kind")?)?,
        approval_surface: decode_string_enum(&row.get::<_, String>("approval_surface")?)?,
        instance_label: row.get("instance_label")?,
        extension_id: row.get("extension_id")?,
        extension_version: row.get("extension_version")?,
        protocol_version: row.get("protocol_version")?,
        capabilities: decode_json(&row.get::<_, String>("capabilities_json")?)?,
        backend_metadata: decode_json(&row.get::<_, String>("backend_metadata_json")?)?,
        profile_hint: row.get("profile_hint")?,
        lock_state: decode_string_enum(&row.get::<_, String>("lock_state")?)?,
        connected: row.get::<_, i64>("connected")? != 0,
        last_seen_at: TimestampMs::from_millis(row.get("last_seen_at")?),
    })
}

fn read_wallet_account(row: &Row<'_>) -> rusqlite::Result<WalletAccountRecord> {
    Ok(WalletAccountRecord {
        wallet_instance_id: read_id(row, "wallet_instance_id")?,
        address: row.get("address")?,
        label: row.get("label")?,
        public_key: row.get("public_key")?,
        is_default: row.get::<_, i64>("is_default")? != 0,
        is_read_only: row.get::<_, i64>("is_read_only")? != 0,
        is_locked: row.get::<_, i64>("is_locked")? != 0,
        last_seen_at: TimestampMs::from_millis(row.get("last_seen_at")?),
    })
}

fn read_id<T>(row: &Row<'_>, column: &str) -> rusqlite::Result<T>
where
    T: TryFrom<String>,
    T::Error: std::fmt::Display,
{
    let value: String = row.get(column)?;
    T::try_from(value).map_err(from_other)
}

fn encode_json<T: Serialize>(value: &T) -> Result<String, RepositoryError> {
    serde_json::to_string(value).map_err(json_error)
}

fn encode_optional_json<T: Serialize>(
    value: Option<&T>,
) -> Result<Option<String>, RepositoryError> {
    value.map(encode_json).transpose()
}

fn decode_json<T: DeserializeOwned>(value: &str) -> rusqlite::Result<T> {
    serde_json::from_str(value).map_err(from_other)
}

fn encode_string_enum<T: Serialize>(value: T) -> Result<String, RepositoryError> {
    match serde_json::to_value(value).map_err(json_error)? {
        serde_json::Value::String(text) => Ok(text),
        _ => Err(RepositoryError::Storage(
            "enum did not serialize to string".to_owned(),
        )),
    }
}

fn encode_optional_string_enum<T: Serialize>(
    value: Option<T>,
) -> Result<Option<String>, RepositoryError> {
    value.map(encode_string_enum).transpose()
}

fn decode_string_enum<T: DeserializeOwned>(value: &str) -> rusqlite::Result<T> {
    serde_json::from_value(serde_json::Value::String(value.to_owned())).map_err(from_other)
}

fn option_timestamp(value: Option<TimestampMs>) -> Option<i64> {
    value.map(TimestampMs::as_millis)
}

fn bool_to_int(value: bool) -> i64 {
    if value { 1 } else { 0 }
}

fn sql_error(error: rusqlite::Error) -> RepositoryError {
    RepositoryError::Storage(error.to_string())
}

fn json_error(error: serde_json::Error) -> RepositoryError {
    RepositoryError::Storage(error.to_string())
}

fn io_error(error: std::io::Error) -> RepositoryError {
    RepositoryError::Storage(error.to_string())
}

fn from_other<E: std::fmt::Display>(error: E) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        0,
        rusqlite::types::Type::Text,
        Box::<dyn std::error::Error + Send + Sync>::from(error.to_string()),
    )
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use rusqlite::{Connection, params};
    use tempfile::tempdir;

    use starmask_core::WalletRepository;
    use starmask_types::{
        ApprovalSurface, BackendKind, LockState, TransportKind, WalletCapability, WalletInstanceId,
        WalletInstanceRecord,
    };

    use super::{SCHEMA_VERSION, SqliteStore};

    fn raw_wallet_account_label(
        database_path: &std::path::Path,
        wallet_instance_id: &WalletInstanceId,
        address: &str,
    ) -> Option<String> {
        let connection = Connection::open(database_path).unwrap();
        connection
            .query_row(
                "SELECT label FROM wallet_accounts WHERE wallet_instance_id = ?1 AND address = ?2",
                params![wallet_instance_id.as_str(), address],
                |row| row.get(0),
            )
            .unwrap()
    }

    fn label_row_updated_at(
        database_path: &std::path::Path,
        wallet_instance_id: &WalletInstanceId,
        address: &str,
    ) -> i64 {
        let connection = Connection::open(database_path).unwrap();
        connection
            .query_row(
                "SELECT updated_at FROM wallet_account_labels WHERE wallet_instance_id = ?1 AND address = ?2",
                params![wallet_instance_id.as_str(), address],
                |row| row.get(0),
            )
            .unwrap()
    }

    #[test]
    fn rejects_old_schema_versions_instead_of_migrating() {
        let tempdir = tempdir().unwrap();
        let database_path = tempdir.path().join("starmaskd.sqlite");
        let connection = Connection::open(&database_path).unwrap();
        connection.pragma_update(None, "user_version", 1).unwrap();
        drop(connection);

        let error = SqliteStore::open(&database_path)
            .err()
            .expect("old schema should be rejected");
        assert!(error.to_string().contains("unsupported schema version: 1"));
    }

    #[test]
    fn initializes_empty_database_to_current_schema_version() {
        let tempdir = tempdir().unwrap();
        let database_path = tempdir.path().join("starmaskd.sqlite");
        let store = SqliteStore::open(&database_path).unwrap();

        assert_eq!(store.schema_version().unwrap(), SCHEMA_VERSION);
    }

    #[test]
    fn rejects_current_version_missing_required_tables() {
        let tempdir = tempdir().unwrap();
        let database_path = tempdir.path().join("starmaskd.sqlite");
        let connection = Connection::open(&database_path).unwrap();
        connection
            .pragma_update(None, "user_version", i64::from(SCHEMA_VERSION))
            .unwrap();
        drop(connection);

        let error = SqliteStore::open(&database_path)
            .err()
            .expect("current version missing tables should be rejected");
        assert!(
            error
                .to_string()
                .contains("missing required table requests")
        );
    }

    #[test]
    fn rejects_unversioned_non_empty_databases() {
        let tempdir = tempdir().unwrap();
        let database_path = tempdir.path().join("starmaskd.sqlite");
        let connection = Connection::open(&database_path).unwrap();
        connection
            .execute_batch("CREATE TABLE legacy_wallets (id TEXT PRIMARY KEY);")
            .unwrap();
        drop(connection);

        let error = SqliteStore::open(&database_path)
            .err()
            .expect("unversioned non-empty database should be rejected");
        assert!(
            error
                .to_string()
                .contains("database has no schema version but is not empty")
        );
    }

    #[test]
    fn local_account_labels_default_to_account_sequence_and_can_be_customized() {
        let tempdir = tempdir().unwrap();
        let database_path = tempdir.path().join("starmaskd.sqlite");
        let mut store = SqliteStore::open(&database_path).unwrap();
        let wallet_instance_id = WalletInstanceId::new("local-default").unwrap();
        store
            .upsert_wallet_instance(WalletInstanceRecord {
                wallet_instance_id: wallet_instance_id.clone(),
                backend_kind: BackendKind::LocalAccountDir,
                transport_kind: TransportKind::LocalSocket,
                approval_surface: ApprovalSurface::TtyPrompt,
                instance_label: "Local Default Wallet".to_owned(),
                extension_id: "local-default".to_owned(),
                extension_version: env!("CARGO_PKG_VERSION").to_owned(),
                protocol_version: 2,
                capabilities: vec![WalletCapability::CreateAccount],
                backend_metadata: serde_json::json!({
                    "account_provider_kind": "local",
                    "prompt_mode": "tty_prompt",
                }),
                profile_hint: Some("local_account_dir".to_owned()),
                lock_state: LockState::Unlocked,
                connected: true,
                last_seen_at: starmask_types::TimestampMs::from_millis(100),
            })
            .unwrap();
        store
            .replace_wallet_accounts(
                &wallet_instance_id,
                vec![
                    starmask_types::WalletAccountRecord {
                        wallet_instance_id: wallet_instance_id.clone(),
                        address: "0x2".to_owned(),
                        label: Some("   ".to_owned()),
                        public_key: Some("0xdef".to_owned()),
                        is_default: false,
                        is_read_only: false,
                        is_locked: false,
                        last_seen_at: starmask_types::TimestampMs::from_millis(101),
                    },
                    starmask_types::WalletAccountRecord {
                        wallet_instance_id: wallet_instance_id.clone(),
                        address: "0x3".to_owned(),
                        label: None,
                        public_key: Some("0xghi".to_owned()),
                        is_default: true,
                        is_read_only: false,
                        is_locked: false,
                        last_seen_at: starmask_types::TimestampMs::from_millis(102),
                    },
                ],
            )
            .unwrap();

        let listed = store
            .list_wallet_accounts(Some(&wallet_instance_id))
            .unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].label.as_deref(), Some("account-1"));
        assert_eq!(listed[1].label.as_deref(), Some("account-2"));

        let renamed = store
            .set_wallet_account_label(&wallet_instance_id, "0x3", "savings")
            .unwrap();
        assert_eq!(renamed.label.as_deref(), Some("savings"));
        assert!(label_row_updated_at(&database_path, &wallet_instance_id, "0x3") > 102);
        let fetched = store
            .get_wallet_account(&wallet_instance_id, "0x3")
            .unwrap()
            .unwrap();
        assert_eq!(fetched.label.as_deref(), Some("savings"));
        assert_eq!(
            raw_wallet_account_label(&database_path, &wallet_instance_id, "0x3"),
            None
        );

        store
            .replace_wallet_accounts(
                &wallet_instance_id,
                vec![
                    starmask_types::WalletAccountRecord {
                        wallet_instance_id: wallet_instance_id.clone(),
                        address: "0x2".to_owned(),
                        label: None,
                        public_key: Some("0xdef".to_owned()),
                        is_default: false,
                        is_read_only: false,
                        is_locked: false,
                        last_seen_at: starmask_types::TimestampMs::from_millis(103),
                    },
                    starmask_types::WalletAccountRecord {
                        wallet_instance_id: wallet_instance_id.clone(),
                        address: "0x3".to_owned(),
                        label: None,
                        public_key: Some("0xghi".to_owned()),
                        is_default: true,
                        is_read_only: false,
                        is_locked: false,
                        last_seen_at: starmask_types::TimestampMs::from_millis(104),
                    },
                    starmask_types::WalletAccountRecord {
                        wallet_instance_id: wallet_instance_id.clone(),
                        address: "0x4".to_owned(),
                        label: None,
                        public_key: Some("0xjkl".to_owned()),
                        is_default: false,
                        is_read_only: false,
                        is_locked: false,
                        last_seen_at: starmask_types::TimestampMs::from_millis(105),
                    },
                ],
            )
            .unwrap();

        let listed = store
            .list_wallet_accounts(Some(&wallet_instance_id))
            .unwrap();
        assert_eq!(
            listed
                .iter()
                .map(|account| (account.address.as_str(), account.label.as_deref()))
                .collect::<Vec<_>>(),
            vec![
                ("0x2", Some("account-1")),
                ("0x3", Some("savings")),
                ("0x4", Some("account-3")),
            ]
        );
        assert_eq!(
            raw_wallet_account_label(&database_path, &wallet_instance_id, "0x4"),
            None
        );
        assert!(label_row_updated_at(&database_path, &wallet_instance_id, "0x4") > 105);
    }
}
