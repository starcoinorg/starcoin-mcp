use rusqlite::Connection;

use starmask_core::RepositoryError;

pub const SCHEMA_VERSION: u32 = 1;

const CURRENT_SCHEMA_SQL: &str = include_str!("../schema.sql");

pub(crate) fn ensure_current_schema(connection: &mut Connection) -> Result<(), RepositoryError> {
    let current: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(sql_error)?;
    match current {
        0 if database_has_user_tables(connection)? => Err(RepositoryError::Storage(format!(
            "database has no schema version but is not empty; recreate it with schema version {SCHEMA_VERSION}"
        ))),
        0 => create_current_schema(connection),
        value if value == i64::from(SCHEMA_VERSION) => Ok(()),
        other => Err(RepositoryError::Storage(format!(
            "unsupported schema version: {other}; recreate the database with schema version {SCHEMA_VERSION}"
        ))),
    }
}

fn database_has_user_tables(connection: &Connection) -> Result<bool, RepositoryError> {
    let table_count: i64 = connection
        .query_row(
            "SELECT COUNT(*)
             FROM sqlite_schema
             WHERE type = 'table'
               AND name NOT LIKE 'sqlite_%'",
            [],
            |row| row.get(0),
        )
        .map_err(sql_error)?;
    Ok(table_count > 0)
}

fn create_current_schema(connection: &mut Connection) -> Result<(), RepositoryError> {
    let transaction = connection.transaction().map_err(sql_error)?;
    transaction
        .execute_batch(CURRENT_SCHEMA_SQL)
        .map_err(sql_error)?;
    transaction
        .pragma_update(None, "user_version", i64::from(SCHEMA_VERSION))
        .map_err(sql_error)?;
    transaction.commit().map_err(sql_error)?;
    Ok(())
}

fn sql_error(error: rusqlite::Error) -> RepositoryError {
    RepositoryError::Storage(error.to_string())
}
