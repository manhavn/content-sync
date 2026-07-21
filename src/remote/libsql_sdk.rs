//! libSQL remote client (see sdk-rust.md)
//!
//! ```ignore
//! let db = Builder::new_remote(url, token).build().await?;
//! let conn = db.connect()?;
//! ```

use crate::models::{validate_table_name, FileRecord, DEFAULT_CONTENT_TABLE};
use crate::remote::SchemaMigrateReport;
use anyhow::{anyhow, Context, Result};
use libsql::{params, Builder};

const REQUIRED_COLUMNS: &[(&str, &str)] = &[
    ("id", "TEXT"),
    ("file_name", "TEXT"),
    ("content", "TEXT"),
    ("content_hash", "TEXT"),
    ("updated_at", "TEXT"),
];

pub struct LibsqlClient {
    conn: libsql::Connection,
    table: String,
}

impl LibsqlClient {
    pub async fn connect(url: &str, access_token: &str, table_name: &str) -> Result<Self> {
        let table = validate_table_name(table_name)?;
        let url = url.trim().to_string();
        if url.is_empty() {
            anyhow::bail!("libsql url is empty");
        }
        let db = Builder::new_remote(url, access_token.to_string())
            .build()
            .await
            .context("libsql Builder::new_remote build")?;
        let conn = db.connect().context("libsql connect")?;
        Ok(Self { conn, table })
    }

    pub async fn ensure_schema(&self) -> Result<SchemaMigrateReport> {
        let t = &self.table;
        let create = format!(
            r#"CREATE TABLE IF NOT EXISTS {t} (
    id           TEXT PRIMARY KEY,
    file_name    TEXT NOT NULL UNIQUE,
    content      TEXT,
    content_hash TEXT,
    updated_at   TEXT NOT NULL
)"#
        );
        self.conn
            .execute(&create, ())
            .await
            .with_context(|| format!("create table {t}"))?;

        let existing = self.list_columns().await?;
        let mut added = Vec::new();
        for &(col, col_type) in REQUIRED_COLUMNS {
            if !existing.iter().any(|c| c.eq_ignore_ascii_case(col)) {
                let alter = format!("ALTER TABLE {t} ADD COLUMN {col} {col_type}");
                self.conn
                    .execute(&alter, ())
                    .await
                    .with_context(|| format!("add column {col} to {t}"))?;
                added.push(col.to_string());
            }
        }

        let after = self.list_columns().await?;
        let has = |name: &str| after.iter().any(|c| c.eq_ignore_ascii_case(name));

        if has("content_json") && has("content") {
            let _ = self
                .conn
                .execute(
                    &format!(
                        "UPDATE {t} SET content = content_json \
                         WHERE (content IS NULL OR content = '') \
                           AND content_json IS NOT NULL AND content_json != ''"
                    ),
                    (),
                )
                .await;
        }
        if has("account_name") && has("file_name") {
            let _ = self
                .conn
                .execute(
                    &format!(
                        "UPDATE {t} SET file_name = account_name \
                         WHERE (file_name IS NULL OR file_name = '') \
                           AND account_name IS NOT NULL AND account_name != ''"
                    ),
                    (),
                )
                .await;
        }
        let _ = self
            .conn
            .execute(
                &format!("CREATE UNIQUE INDEX IF NOT EXISTS idx_{t}_file_name ON {t}(file_name)"),
                (),
            )
            .await;

        let after = self.list_columns().await?;
        let mut missing = Vec::new();
        for &(col, _) in REQUIRED_COLUMNS {
            if !after.iter().any(|c| c.eq_ignore_ascii_case(col)) {
                missing.push(col.to_string());
            }
        }
        if !missing.is_empty() {
            return Err(anyhow!(
                "table `{t}` still missing columns after migration: {}",
                missing.join(", ")
            ));
        }

        Ok(SchemaMigrateReport {
            table: t.clone(),
            columns: after,
            added_columns: added,
        })
    }

    async fn list_columns(&self) -> Result<Vec<String>> {
        let sql = format!("PRAGMA table_info({})", self.table);
        let mut rows = self
            .conn
            .query(&sql, ())
            .await
            .context("PRAGMA table_info")?;
        let mut cols = Vec::new();
        while let Some(row) = rows.next().await? {
            // cid, name, type, notnull, dflt_value, pk — name is idx 1
            let name: String = row.get(1).unwrap_or_default();
            if !name.is_empty() {
                cols.push(name);
            }
        }
        Ok(cols)
    }

    pub async fn ping(&self) -> Result<()> {
        let mut rows = self.conn.query("SELECT 1", ()).await?;
        let _ = rows.next().await?;
        Ok(())
    }

    pub async fn list_files(&self) -> Result<Vec<FileRecord>> {
        let t = &self.table;
        let cols = self.list_columns().await.unwrap_or_default();
        let content_col = if cols.iter().any(|c| c.eq_ignore_ascii_case("content")) {
            "content"
        } else {
            "content_json"
        };
        let sql = format!(
            "SELECT id, file_name, {content_col}, content_hash, updated_at
             FROM {t} ORDER BY file_name"
        );
        let mut rows = self.conn.query(&sql, ()).await?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await? {
            let get = |i: i32| -> String { row.get::<String>(i).unwrap_or_default() };
            let file_name = get(1);
            if file_name.is_empty() {
                continue;
            }
            out.push(FileRecord {
                id: get(0),
                file_name,
                file_path: String::new(),
                content: get(2),
                content_hash: get(3),
                updated_at: get(4),
                connection_id: None,
            });
        }
        Ok(out)
    }

    pub async fn upsert_file(&self, rec: &FileRecord) -> Result<()> {
        let t = &self.table;
        let sql = format!(
            r#"
            INSERT INTO {t}(id, file_name, content, content_hash, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5)
            ON CONFLICT(file_name) DO UPDATE SET
                content      = excluded.content,
                content_hash = excluded.content_hash,
                updated_at   = excluded.updated_at
        "#
        );
        self.conn
            .execute(
                &sql,
                params![
                    rec.id.as_str(),
                    rec.file_name.as_str(),
                    rec.content.as_str(),
                    rec.content_hash.as_str(),
                    rec.updated_at.as_str(),
                ],
            )
            .await
            .context("libsql upsert")?;
        Ok(())
    }

    pub async fn delete_file(&self, file_name: &str) -> Result<()> {
        let t = &self.table;
        let sql = format!("DELETE FROM {t} WHERE file_name = ?1");
        self.conn
            .execute(&sql, params![file_name])
            .await
            .context("libsql delete")?;
        Ok(())
    }
}

pub async fn test_connection(
    url: &str,
    access_token: &str,
    table_name: &str,
) -> Result<SchemaMigrateReport> {
    let table = if table_name.trim().is_empty() {
        DEFAULT_CONTENT_TABLE
    } else {
        table_name
    };
    let client = LibsqlClient::connect(url, access_token, table).await?;
    client.ping().await?;
    client.ensure_schema().await
}
