mod bunny;
mod libsql_sdk;
mod mongo;
mod sql_std;

pub use bunny::SchemaMigrateReport;

use crate::models::{Connection, ConnectionDriver, FileRecord};
use anyhow::Result;

/// Ensure remote table/collection exists and columns/indexes match.
pub async fn ensure_schema(conn: &Connection) -> Result<SchemaMigrateReport> {
    match conn.driver {
        ConnectionDriver::SqlApi => {
            let client =
                bunny::BunnyClient::with_table(&conn.url, &conn.access_token, &conn.table_name)?;
            client.ensure_schema().await
        }
        ConnectionDriver::Libsql => {
            let client =
                libsql_sdk::LibsqlClient::connect(&conn.url, &conn.access_token, &conn.table_name)
                    .await?;
            client.ensure_schema().await
        }
        ConnectionDriver::Sqlite
        | ConnectionDriver::Postgres
        | ConnectionDriver::Mysql
        | ConnectionDriver::Mariadb => {
            sql_std::ensure_schema(conn.driver, &conn.url, &conn.access_token, &conn.table_name)
                .await
        }
        ConnectionDriver::Mongodb => {
            mongo::ensure_schema(&conn.url, &conn.access_token, &conn.table_name).await
        }
    }
}

pub async fn list_files(conn: &Connection) -> Result<Vec<FileRecord>> {
    match conn.driver {
        ConnectionDriver::SqlApi => {
            let client =
                bunny::BunnyClient::with_table(&conn.url, &conn.access_token, &conn.table_name)?;
            client.list_files_no_schema().await
        }
        ConnectionDriver::Libsql => {
            let client =
                libsql_sdk::LibsqlClient::connect(&conn.url, &conn.access_token, &conn.table_name)
                    .await?;
            client.list_files().await
        }
        ConnectionDriver::Sqlite
        | ConnectionDriver::Postgres
        | ConnectionDriver::Mysql
        | ConnectionDriver::Mariadb => {
            sql_std::list_files(conn.driver, &conn.url, &conn.access_token, &conn.table_name).await
        }
        ConnectionDriver::Mongodb => {
            mongo::list_files(&conn.url, &conn.access_token, &conn.table_name).await
        }
    }
}

pub async fn upsert_file(conn: &Connection, rec: &FileRecord) -> Result<()> {
    match conn.driver {
        ConnectionDriver::SqlApi => {
            let client =
                bunny::BunnyClient::with_table(&conn.url, &conn.access_token, &conn.table_name)?;
            client.upsert_file_no_schema(rec).await
        }
        ConnectionDriver::Libsql => {
            let client =
                libsql_sdk::LibsqlClient::connect(&conn.url, &conn.access_token, &conn.table_name)
                    .await?;
            client.upsert_file(rec).await
        }
        ConnectionDriver::Sqlite
        | ConnectionDriver::Postgres
        | ConnectionDriver::Mysql
        | ConnectionDriver::Mariadb => {
            sql_std::upsert_file(
                conn.driver,
                &conn.url,
                &conn.access_token,
                &conn.table_name,
                rec,
            )
            .await
        }
        ConnectionDriver::Mongodb => {
            mongo::upsert_file(&conn.url, &conn.access_token, &conn.table_name, rec).await
        }
    }
}

pub async fn delete_file(conn: &Connection, file_name: &str) -> Result<()> {
    match conn.driver {
        ConnectionDriver::SqlApi => {
            let client =
                bunny::BunnyClient::with_table(&conn.url, &conn.access_token, &conn.table_name)?;
            client.delete_file_no_schema(file_name).await
        }
        ConnectionDriver::Libsql => {
            let client =
                libsql_sdk::LibsqlClient::connect(&conn.url, &conn.access_token, &conn.table_name)
                    .await?;
            client.delete_file(file_name).await
        }
        ConnectionDriver::Sqlite
        | ConnectionDriver::Postgres
        | ConnectionDriver::Mysql
        | ConnectionDriver::Mariadb => {
            sql_std::delete_file(
                conn.driver,
                &conn.url,
                &conn.access_token,
                &conn.table_name,
                file_name,
            )
            .await
        }
        ConnectionDriver::Mongodb => {
            mongo::delete_file(&conn.url, &conn.access_token, &conn.table_name, file_name).await
        }
    }
}

pub async fn test_connection(conn: &Connection) -> Result<SchemaMigrateReport> {
    match conn.driver {
        ConnectionDriver::SqlApi => {
            bunny::test_connection(&conn.url, &conn.access_token, &conn.table_name).await
        }
        ConnectionDriver::Libsql => {
            libsql_sdk::test_connection(&conn.url, &conn.access_token, &conn.table_name).await
        }
        ConnectionDriver::Sqlite
        | ConnectionDriver::Postgres
        | ConnectionDriver::Mysql
        | ConnectionDriver::Mariadb => {
            sql_std::test_connection(conn.driver, &conn.url, &conn.access_token, &conn.table_name)
                .await
        }
        ConnectionDriver::Mongodb => {
            mongo::test_connection(&conn.url, &conn.access_token, &conn.table_name).await
        }
    }
}
