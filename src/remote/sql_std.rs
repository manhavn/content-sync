//! SQLite / PostgreSQL / MySQL / MariaDB via sqlx

use crate::models::{validate_table_name, ConnectionDriver, FileRecord};
use crate::remote::SchemaMigrateReport;
use anyhow::{anyhow, Context, Result};
use sqlx::mysql::{MySqlConnectOptions, MySqlPoolOptions};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{AssertSqlSafe, MySql, Pool, Postgres, Row, Sqlite};
use std::str::FromStr;
use std::time::Duration;

/// table names are validated; dynamic SQL is intentional
fn q(sql: String) -> AssertSqlSafe<String> {
    AssertSqlSafe(sql)
}

fn qi_pg(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}
fn qi_my(name: &str) -> String {
    format!("`{}`", name.replace('`', "``"))
}

pub async fn ensure_schema(
    driver: ConnectionDriver,
    url: &str,
    password: &str,
    table: &str,
) -> Result<SchemaMigrateReport> {
    let table = validate_table_name(table)?;
    match driver {
        ConnectionDriver::Sqlite => ensure_sqlite(&connect_sqlite(url).await?, &table).await,
        ConnectionDriver::Postgres => {
            ensure_postgres(&connect_pg(url, password).await?, &table).await
        }
        ConnectionDriver::Mysql | ConnectionDriver::Mariadb => {
            ensure_mysql(&connect_mysql(url, password).await?, &table).await
        }
        _ => Err(anyhow!("not a sql_std driver")),
    }
}

pub async fn list_files(
    driver: ConnectionDriver,
    url: &str,
    password: &str,
    table: &str,
) -> Result<Vec<FileRecord>> {
    let table = validate_table_name(table)?;
    match driver {
        ConnectionDriver::Sqlite => {
            let pool = connect_sqlite(url).await?;
            ensure_sqlite(&pool, &table).await?;
            list_generic_sqlite(&pool, &table).await
        }
        ConnectionDriver::Postgres => {
            let pool = connect_pg(url, password).await?;
            ensure_postgres(&pool, &table).await?;
            list_generic_pg(&pool, &table).await
        }
        ConnectionDriver::Mysql | ConnectionDriver::Mariadb => {
            let pool = connect_mysql(url, password).await?;
            ensure_mysql(&pool, &table).await?;
            list_generic_mysql(&pool, &table).await
        }
        _ => Err(anyhow!("not a sql_std driver")),
    }
}

pub async fn upsert_file(
    driver: ConnectionDriver,
    url: &str,
    password: &str,
    table: &str,
    rec: &FileRecord,
) -> Result<()> {
    let table = validate_table_name(table)?;
    match driver {
        ConnectionDriver::Sqlite => {
            let pool = connect_sqlite(url).await?;
            ensure_sqlite(&pool, &table).await?;
            let sql = format!(
                "INSERT INTO {table}(id, file_name, content, content_hash, updated_at) \
                 VALUES(?1,?2,?3,?4,?5) \
                 ON CONFLICT(file_name) DO UPDATE SET content=excluded.content, \
                 content_hash=excluded.content_hash, updated_at=excluded.updated_at"
            );
            sqlx::query(q(sql))
                .bind(&rec.id)
                .bind(&rec.file_name)
                .bind(&rec.content)
                .bind(&rec.content_hash)
                .bind(&rec.updated_at)
                .execute(&pool)
                .await?;
            Ok(())
        }
        ConnectionDriver::Postgres => {
            let pool = connect_pg(url, password).await?;
            ensure_postgres(&pool, &table).await?;
            let t = qi_pg(&table);
            let sql = format!(
                "INSERT INTO {t}(id, file_name, content, content_hash, updated_at) \
                 VALUES($1,$2,$3,$4,$5) \
                 ON CONFLICT(file_name) DO UPDATE SET content=EXCLUDED.content, \
                 content_hash=EXCLUDED.content_hash, updated_at=EXCLUDED.updated_at"
            );
            sqlx::query(q(sql))
                .bind(&rec.id)
                .bind(&rec.file_name)
                .bind(&rec.content)
                .bind(&rec.content_hash)
                .bind(&rec.updated_at)
                .execute(&pool)
                .await?;
            Ok(())
        }
        ConnectionDriver::Mysql | ConnectionDriver::Mariadb => {
            let pool = connect_mysql(url, password).await?;
            ensure_mysql(&pool, &table).await?;
            let t = qi_my(&table);
            let sql = format!(
                "INSERT INTO {t}(id, file_name, content, content_hash, updated_at) \
                 VALUES(?,?,?,?,?) \
                 ON DUPLICATE KEY UPDATE content=VALUES(content), \
                 content_hash=VALUES(content_hash), updated_at=VALUES(updated_at)"
            );
            sqlx::query(q(sql))
                .bind(&rec.id)
                .bind(&rec.file_name)
                .bind(&rec.content)
                .bind(&rec.content_hash)
                .bind(&rec.updated_at)
                .execute(&pool)
                .await?;
            Ok(())
        }
        _ => Err(anyhow!("not a sql_std driver")),
    }
}

pub async fn delete_file(
    driver: ConnectionDriver,
    url: &str,
    password: &str,
    table: &str,
    file_name: &str,
) -> Result<()> {
    let table = validate_table_name(table)?;
    match driver {
        ConnectionDriver::Sqlite => {
            let pool = connect_sqlite(url).await?;
            sqlx::query(q(format!("DELETE FROM {table} WHERE file_name = ?1")))
                .bind(file_name)
                .execute(&pool)
                .await?;
            Ok(())
        }
        ConnectionDriver::Postgres => {
            let pool = connect_pg(url, password).await?;
            let t = qi_pg(&table);
            sqlx::query(q(format!("DELETE FROM {t} WHERE file_name = $1")))
                .bind(file_name)
                .execute(&pool)
                .await?;
            Ok(())
        }
        ConnectionDriver::Mysql | ConnectionDriver::Mariadb => {
            let pool = connect_mysql(url, password).await?;
            let t = qi_my(&table);
            sqlx::query(q(format!("DELETE FROM {t} WHERE file_name = ?")))
                .bind(file_name)
                .execute(&pool)
                .await?;
            Ok(())
        }
        _ => Err(anyhow!("not a sql_std driver")),
    }
}

pub async fn test_connection(
    driver: ConnectionDriver,
    url: &str,
    password: &str,
    table: &str,
) -> Result<SchemaMigrateReport> {
    match driver {
        ConnectionDriver::Sqlite => {
            let pool = connect_sqlite(url).await?;
            sqlx::query("SELECT 1").execute(&pool).await?;
            ensure_sqlite(&pool, &validate_table_name(table)?).await
        }
        ConnectionDriver::Postgres => {
            let pool = connect_pg(url, password).await?;
            sqlx::query("SELECT 1").execute(&pool).await?;
            ensure_postgres(&pool, &validate_table_name(table)?).await
        }
        ConnectionDriver::Mysql | ConnectionDriver::Mariadb => {
            let pool = connect_mysql(url, password).await?;
            sqlx::query("SELECT 1").execute(&pool).await?;
            ensure_mysql(&pool, &validate_table_name(table)?).await
        }
        _ => Err(anyhow!("not a sql_std driver")),
    }
}

async fn connect_sqlite(url: &str) -> Result<Pool<Sqlite>> {
    let opts = SqliteConnectOptions::from_str(url)
        .with_context(|| format!("parse sqlite url {url}"))?
        .create_if_missing(true);
    SqlitePoolOptions::new()
        .max_connections(4)
        .acquire_timeout(Duration::from_secs(15))
        .connect_with(opts)
        .await
        .context("connect sqlite")
}

async fn connect_pg(url: &str, password: &str) -> Result<Pool<Postgres>> {
    let mut opts =
        PgConnectOptions::from_str(url).with_context(|| format!("parse postgres url {url}"))?;
    if !password.is_empty() {
        opts = opts.password(password);
    }
    PgPoolOptions::new()
        .max_connections(5)
        .acquire_timeout(Duration::from_secs(15))
        .connect_with(opts)
        .await
        .context("connect postgres")
}

async fn connect_mysql(url: &str, password: &str) -> Result<Pool<MySql>> {
    let mut opts =
        MySqlConnectOptions::from_str(url).with_context(|| format!("parse mysql url {url}"))?;
    if !password.is_empty() {
        opts = opts.password(password);
    }
    MySqlPoolOptions::new()
        .max_connections(5)
        .acquire_timeout(Duration::from_secs(15))
        .connect_with(opts)
        .await
        .context("connect mysql/mariadb")
}

async fn ensure_sqlite(pool: &Pool<Sqlite>, table: &str) -> Result<SchemaMigrateReport> {
    let create = format!(
        "CREATE TABLE IF NOT EXISTS {table} (\
         id TEXT PRIMARY KEY, file_name TEXT NOT NULL UNIQUE, content TEXT, \
         content_hash TEXT, updated_at TEXT NOT NULL)"
    );
    sqlx::query(q(create)).execute(pool).await?;
    let mut added = Vec::new();
    for (col, ty) in [
        ("content", "TEXT"),
        ("content_hash", "TEXT"),
        ("updated_at", "TEXT"),
        ("file_name", "TEXT"),
        ("id", "TEXT"),
    ] {
        if !col_exists_sqlite(pool, table, col).await? {
            sqlx::query(q(format!("ALTER TABLE {table} ADD COLUMN {col} {ty}")))
                .execute(pool)
                .await?;
            added.push(col.to_string());
        }
    }
    let _ = sqlx::query(q(format!(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_{table}_file_name ON {table}(file_name)"
    )))
    .execute(pool)
    .await;
    if col_exists_sqlite(pool, table, "content_json").await? {
        let _ = sqlx::query(q(format!(
            "UPDATE {table} SET content = content_json \
             WHERE (content IS NULL OR content = '') AND content_json IS NOT NULL"
        )))
        .execute(pool)
        .await;
    }
    Ok(SchemaMigrateReport {
        table: table.to_string(),
        columns: cols_sqlite(pool, table).await?,
        added_columns: added,
    })
}

async fn ensure_postgres(pool: &Pool<Postgres>, table: &str) -> Result<SchemaMigrateReport> {
    let t = qi_pg(table);
    let create = format!(
        "CREATE TABLE IF NOT EXISTS {t} (\
         id TEXT PRIMARY KEY, file_name TEXT NOT NULL UNIQUE, content TEXT, \
         content_hash TEXT, updated_at TEXT NOT NULL)"
    );
    sqlx::query(q(create)).execute(pool).await?;
    let mut added = Vec::new();
    for (col, ty) in [
        ("content", "TEXT"),
        ("content_hash", "TEXT"),
        ("updated_at", "TEXT"),
        ("file_name", "TEXT"),
        ("id", "TEXT"),
    ] {
        if !col_exists_pg(pool, table, col).await? {
            sqlx::query(q(format!(
                "ALTER TABLE {t} ADD COLUMN IF NOT EXISTS {col} {ty}"
            )))
            .execute(pool)
            .await?;
            added.push(col.to_string());
        }
    }
    let _ = sqlx::query(q(format!(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_{table}_file_name ON {t}(file_name)"
    )))
    .execute(pool)
    .await;
    Ok(SchemaMigrateReport {
        table: table.to_string(),
        columns: cols_pg(pool, table).await?,
        added_columns: added,
    })
}

async fn ensure_mysql(pool: &Pool<MySql>, table: &str) -> Result<SchemaMigrateReport> {
    let t = qi_my(table);
    let create = format!(
        "CREATE TABLE IF NOT EXISTS {t} (\
         id VARCHAR(64) PRIMARY KEY, file_name VARCHAR(255) NOT NULL, \
         content LONGTEXT, content_hash VARCHAR(128), updated_at VARCHAR(64) NOT NULL, \
         UNIQUE KEY uk_file_name (file_name)) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4"
    );
    sqlx::query(q(create)).execute(pool).await?;
    let mut added = Vec::new();
    for (col, ty) in [
        ("content", "LONGTEXT"),
        ("content_hash", "VARCHAR(128)"),
        ("updated_at", "VARCHAR(64)"),
        ("file_name", "VARCHAR(255)"),
        ("id", "VARCHAR(64)"),
    ] {
        if !col_exists_mysql(pool, table, col).await? {
            match sqlx::query(q(format!("ALTER TABLE {t} ADD COLUMN {col} {ty}")))
                .execute(pool)
                .await
            {
                Ok(_) => added.push(col.to_string()),
                Err(e) if e.to_string().to_lowercase().contains("duplicate") => {}
                Err(e) => return Err(e.into()),
            }
        }
    }
    Ok(SchemaMigrateReport {
        table: table.to_string(),
        columns: cols_mysql(pool, table).await?,
        added_columns: added,
    })
}

async fn list_generic_sqlite(pool: &Pool<Sqlite>, table: &str) -> Result<Vec<FileRecord>> {
    let rows = sqlx::query(q(format!(
        "SELECT id, file_name, content, content_hash, updated_at FROM {table} ORDER BY file_name"
    )))
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|r| {
            let file_name: String = r.try_get("file_name").ok()?;
            if file_name.is_empty() {
                return None;
            }
            Some(FileRecord {
                id: r.try_get("id").unwrap_or_default(),
                file_name,
                file_path: String::new(),
                content: r.try_get("content").unwrap_or_default(),
                content_hash: r.try_get("content_hash").unwrap_or_default(),
                updated_at: r.try_get("updated_at").unwrap_or_default(),
                connection_id: None,
            })
        })
        .collect())
}

async fn list_generic_pg(pool: &Pool<Postgres>, table: &str) -> Result<Vec<FileRecord>> {
    let t = qi_pg(table);
    let rows = sqlx::query(q(format!(
        "SELECT id, file_name, content, content_hash, updated_at FROM {t} ORDER BY file_name"
    )))
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|r| {
            let file_name: String = r.try_get("file_name").ok()?;
            if file_name.is_empty() {
                return None;
            }
            Some(FileRecord {
                id: r.try_get("id").unwrap_or_default(),
                file_name,
                file_path: String::new(),
                content: r.try_get("content").unwrap_or_default(),
                content_hash: r.try_get("content_hash").unwrap_or_default(),
                updated_at: r.try_get("updated_at").unwrap_or_default(),
                connection_id: None,
            })
        })
        .collect())
}

async fn list_generic_mysql(pool: &Pool<MySql>, table: &str) -> Result<Vec<FileRecord>> {
    let t = qi_my(table);
    let rows = sqlx::query(q(format!(
        "SELECT id, file_name, content, content_hash, updated_at FROM {t} ORDER BY file_name"
    )))
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|r| {
            let file_name: String = r.try_get("file_name").ok()?;
            if file_name.is_empty() {
                return None;
            }
            Some(FileRecord {
                id: r.try_get("id").unwrap_or_default(),
                file_name,
                file_path: String::new(),
                content: r.try_get("content").unwrap_or_default(),
                content_hash: r.try_get("content_hash").unwrap_or_default(),
                updated_at: r.try_get("updated_at").unwrap_or_default(),
                connection_id: None,
            })
        })
        .collect())
}

async fn cols_sqlite(pool: &Pool<Sqlite>, table: &str) -> Result<Vec<String>> {
    let rows = sqlx::query(q(format!("PRAGMA table_info({table})")))
        .fetch_all(pool)
        .await?;
    Ok(rows
        .into_iter()
        .filter_map(|r| r.try_get::<String, _>("name").ok())
        .collect())
}

async fn col_exists_sqlite(pool: &Pool<Sqlite>, table: &str, col: &str) -> Result<bool> {
    Ok(cols_sqlite(pool, table)
        .await?
        .iter()
        .any(|c| c.eq_ignore_ascii_case(col)))
}

async fn cols_pg(pool: &Pool<Postgres>, table: &str) -> Result<Vec<String>> {
    let rows = sqlx::query(
        "SELECT column_name FROM information_schema.columns \
         WHERE table_name = $1 AND table_schema = current_schema()",
    )
    .bind(table)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|r| r.try_get::<String, _>("column_name").ok())
        .collect())
}

async fn col_exists_pg(pool: &Pool<Postgres>, table: &str, col: &str) -> Result<bool> {
    Ok(cols_pg(pool, table)
        .await?
        .iter()
        .any(|c| c.eq_ignore_ascii_case(col)))
}

async fn cols_mysql(pool: &Pool<MySql>, table: &str) -> Result<Vec<String>> {
    let rows = sqlx::query(
        "SELECT COLUMN_NAME AS column_name FROM information_schema.COLUMNS \
         WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = ?",
    )
    .bind(table)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|r| r.try_get::<String, _>("column_name").ok())
        .collect())
}

async fn col_exists_mysql(pool: &Pool<MySql>, table: &str, col: &str) -> Result<bool> {
    Ok(cols_mysql(pool, table)
        .await?
        .iter()
        .any(|c| c.eq_ignore_ascii_case(col)))
}
