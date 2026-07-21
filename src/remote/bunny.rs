//! Bunny Database SQL API client (Hrana over HTTP / libSQL remote protocol).
//! See sql-api.md

use crate::models::{validate_table_name, FileRecord};
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Required columns only (legacy token fields are no longer used)
const REQUIRED_COLUMNS: &[(&str, &str)] = &[
    ("id", "TEXT"),
    ("file_name", "TEXT"),
    ("content", "TEXT"),
    ("content_hash", "TEXT"),
    ("updated_at", "TEXT"),
];

#[derive(Clone)]
pub struct BunnyClient {
    http: reqwest::Client,
    url: String,
    access_token: String,
    table: String,
}

impl BunnyClient {
    pub fn with_table(url: &str, access_token: &str, table_name: &str) -> Result<Self> {
        let table = validate_table_name(table_name)?;
        Ok(Self {
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("reqwest client"),
            url: url.trim().to_string(),
            access_token: access_token.to_string(),
            table,
        })
    }

    /// Execute one or more SQL statements via /v2/pipeline
    pub async fn execute(&self, statements: Vec<Stmt>) -> Result<PipelineResponse> {
        let mut requests: Vec<Value> = statements
            .into_iter()
            .map(|s| {
                let mut stmt = json!({ "sql": s.sql });
                if let Some(args) = s.args {
                    stmt["args"] = Value::Array(args);
                }
                if let Some(named) = s.named_args {
                    stmt["named_args"] = Value::Array(named);
                }
                json!({ "type": "execute", "stmt": stmt })
            })
            .collect();
        requests.push(json!({ "type": "close" }));

        let body = json!({ "requests": requests });

        let resp = self
            .http
            .post(&self.url)
            .header("Authorization", format!("Bearer {}", self.access_token))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .context("HTTP request to Bunny SQL API")?;

        let status = resp.status();
        let text = resp.text().await.context("read response body")?;
        if !status.is_success() {
            return Err(anyhow!("Bunny API HTTP {status}: {text}"));
        }

        let parsed: PipelineResponse =
            serde_json::from_str(&text).with_context(|| format!("parse response: {text}"))?;

        for (i, r) in parsed.results.iter().enumerate() {
            if r.r#type == "error" {
                let msg = r
                    .error
                    .as_ref()
                    .map(|e| e.message.clone())
                    .unwrap_or_else(|| "unknown error".into());
                return Err(anyhow!("statement[{i}] error: {msg}"));
            }
        }
        Ok(parsed)
    }

    /// Create table if missing, then ADD only required columns. Migrates legacy `content_json` → `content`.
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
        self.execute(vec![Stmt::sql(create)]).await?;

        let existing = self.list_columns().await?;
        let mut added = Vec::new();
        for &(col, col_type) in REQUIRED_COLUMNS {
            let present = existing.iter().any(|c| c.eq_ignore_ascii_case(col));
            if !present {
                let alter = format!("ALTER TABLE {t} ADD COLUMN {col} {col_type}");
                self.execute(vec![Stmt::sql(alter)])
                    .await
                    .with_context(|| format!("add column {col} to {t}"))?;
                added.push(col.to_string());
            }
        }

        let after = self.list_columns().await?;
        let has = |name: &str| after.iter().any(|c| c.eq_ignore_ascii_case(name));

        // Legacy: copy content_json → content when present
        if has("content_json") && has("content") {
            let _ = self
                .execute(vec![Stmt::sql(format!(
                    "UPDATE {t} SET content = content_json \
                     WHERE (content IS NULL OR content = '') \
                       AND content_json IS NOT NULL AND content_json != ''"
                ))])
                .await;
        }
        // Legacy: fill file_name from account_name
        if has("account_name") && has("file_name") {
            let _ = self
                .execute(vec![Stmt::sql(format!(
                    "UPDATE {t} SET file_name = account_name \
                     WHERE (file_name IS NULL OR file_name = '') \
                       AND account_name IS NOT NULL AND account_name != ''"
                ))])
                .await;
        }

        // Ensure UNIQUE(file_name) for ON CONFLICT (old tables may only have UNIQUE(account_name))
        let _ = self
            .execute(vec![Stmt::sql(format!(
                "CREATE UNIQUE INDEX IF NOT EXISTS idx_{t}_file_name ON {t}(file_name)"
            ))])
            .await;

        let mut missing = Vec::new();
        for &(col, _) in REQUIRED_COLUMNS {
            if !has(col) {
                missing.push(col.to_string());
            }
        }
        if !missing.is_empty() {
            return Err(anyhow!(
                "table `{t}` still missing columns after migration: {}",
                missing.join(", ")
            ));
        }

        let after = self.list_columns().await?;
        Ok(SchemaMigrateReport {
            table: t.clone(),
            columns: after,
            added_columns: added,
        })
    }

    async fn list_columns(&self) -> Result<Vec<String>> {
        // PRAGMA table_info returns cid, name, type, notnull, dflt_value, pk
        let sql = format!("PRAGMA table_info({})", self.table);
        let resp = self.execute(vec![Stmt::sql(sql)]).await?;
        let Some(result) = resp.first_execute_result() else {
            return Ok(vec![]);
        };
        let mut cols = Vec::new();
        for row in &result.rows {
            // name is index 1
            if let Some(name) = row.get(1).and_then(|c| c.as_text()) {
                if !name.is_empty() {
                    cols.push(name.to_string());
                }
            }
        }
        Ok(cols)
    }

    pub async fn ping(&self) -> Result<()> {
        self.execute(vec![Stmt::sql("SELECT 1")]).await?;
        Ok(())
    }

    /// List files without CREATE TABLE (caller already ensured schema).
    pub async fn list_files_no_schema(&self) -> Result<Vec<FileRecord>> {
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
        let resp = self.execute(vec![Stmt::sql(sql)]).await?;

        let Some(result) = resp.first_execute_result() else {
            return Ok(vec![]);
        };

        let mut out = Vec::new();
        for row in &result.rows {
            let get = |i: usize| -> String {
                row.get(i)
                    .and_then(|c| c.as_text())
                    .unwrap_or_default()
                    .to_string()
            };
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

    pub async fn upsert_file_no_schema(&self, rec: &FileRecord) -> Result<()> {
        let t = &self.table;
        let sql = format!(
            r#"
            INSERT INTO {t}(id, file_name, content, content_hash, updated_at)
            VALUES (?, ?, ?, ?, ?)
            ON CONFLICT(file_name) DO UPDATE SET
                content      = excluded.content,
                content_hash = excluded.content_hash,
                updated_at   = excluded.updated_at
        "#
        );
        self.execute(vec![Stmt {
            sql,
            args: Some(vec![
                text_arg(&rec.id),
                text_arg(&rec.file_name),
                text_arg(&rec.content),
                text_arg(&rec.content_hash),
                text_arg(&rec.updated_at),
            ]),
            named_args: None,
        }])
        .await?;
        Ok(())
    }

    pub async fn delete_file_no_schema(&self, file_name: &str) -> Result<()> {
        let t = &self.table;
        let sql = format!("DELETE FROM {t} WHERE file_name = ?");
        self.execute(vec![Stmt {
            sql,
            args: Some(vec![text_arg(file_name)]),
            named_args: None,
        }])
        .await?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SchemaMigrateReport {
    pub table: String,
    pub columns: Vec<String>,
    pub added_columns: Vec<String>,
}

fn text_arg(s: &str) -> Value {
    json!({ "type": "text", "value": s })
}

#[derive(Debug, Clone)]
pub struct Stmt {
    pub sql: String,
    pub args: Option<Vec<Value>>,
    pub named_args: Option<Vec<Value>>,
}

impl Stmt {
    pub fn sql(sql: impl Into<String>) -> Self {
        Self {
            sql: sql.into(),
            args: None,
            named_args: None,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct PipelineResponse {
    #[serde(default)]
    pub results: Vec<PipelineResult>,
}

impl PipelineResponse {
    pub fn first_execute_result(&self) -> Option<&ExecuteResult> {
        for r in &self.results {
            if let Some(ref resp) = r.response {
                if resp.r#type == "execute" {
                    return resp.result.as_ref();
                }
            }
        }
        None
    }
}

#[derive(Debug, Deserialize)]
pub struct PipelineResult {
    pub r#type: String,
    #[serde(default)]
    pub response: Option<ExecuteResponse>,
    #[serde(default)]
    pub error: Option<ApiError>,
}

#[derive(Debug, Deserialize)]
pub struct ExecuteResponse {
    pub r#type: String,
    #[serde(default)]
    pub result: Option<ExecuteResult>,
}

#[derive(Debug, Deserialize)]
pub struct ExecuteResult {
    #[serde(default)]
    #[allow(dead_code)]
    pub cols: Vec<Col>,
    #[serde(default)]
    pub rows: Vec<Vec<Cell>>,
    #[serde(default)]
    #[allow(dead_code)]
    pub affected_row_count: u64,
}

#[derive(Debug, Deserialize)]
pub struct Col {
    #[allow(dead_code)]
    pub name: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Cell {
    pub r#type: String,
    #[serde(default)]
    pub value: Option<Value>,
}

impl Cell {
    pub fn as_text(&self) -> Option<&str> {
        match self.r#type.as_str() {
            "null" => Some(""),
            _ => self.value.as_ref().and_then(|v| match v {
                Value::String(s) => Some(s.as_str()),
                Value::Null => Some(""),
                _ => None,
            }),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct ApiError {
    #[serde(default)]
    pub message: String,
}

pub async fn test_connection(
    url: &str,
    access_token: &str,
    table_name: &str,
) -> Result<SchemaMigrateReport> {
    let client = BunnyClient::with_table(url, access_token, table_name)?;
    client.ping().await?;
    client.ensure_schema().await
}
