//! MongoDB collection sync (table_name = collection name)

use crate::models::{validate_table_name, FileRecord};
use crate::remote::SchemaMigrateReport;
use anyhow::{Context, Result};
use futures_util::StreamExt;
use mongodb::bson::{doc, Document};
use mongodb::options::{ClientOptions, ReplaceOptions};
use mongodb::{Client, Collection};

fn collection_name(table: &str) -> Result<String> {
    // reuse identifier rules (letters, digits, underscore)
    validate_table_name(table)
}

async fn collection(url: &str, password: &str, coll: &str) -> Result<Collection<Document>> {
    let mut opts = ClientOptions::parse(url)
        .await
        .with_context(|| format!("parse mongodb url"))?;
    if !password.is_empty() {
        // If password not in URI, set credential password
        if let Some(ref mut cred) = opts.credential {
            if cred.password.is_none() {
                cred.password = Some(password.to_string());
            }
        }
    }
    let client = Client::with_options(opts).context("mongodb client")?;
    // Database from URI path, or default "content_sync"
    let db_name = client
        .default_database()
        .map(|d| d.name().to_string())
        .unwrap_or_else(|| "content_sync".to_string());
    let db = client.database(&db_name);
    Ok(db.collection::<Document>(coll))
}

pub async fn ensure_schema(url: &str, password: &str, table: &str) -> Result<SchemaMigrateReport> {
    let coll_name = collection_name(table)?;
    let coll = collection(url, password, &coll_name).await?;
    // Ensure unique index on file_name
    let indexes = coll.list_index_names().await.unwrap_or_default();
    let mut added = Vec::new();
    if !indexes.iter().any(|n| n.contains("file_name")) {
        use mongodb::options::IndexOptions;
        use mongodb::IndexModel;
        let model = IndexModel::builder()
            .keys(doc! { "file_name": 1 })
            .options(
                IndexOptions::builder()
                    .unique(true)
                    .name("uk_file_name".to_string())
                    .build(),
            )
            .build();
        coll.create_index(model)
            .await
            .context("create unique index on file_name")?;
        added.push("index:file_name".to_string());
    }
    // Ping via estimated count
    let _ = coll.estimated_document_count().await;
    Ok(SchemaMigrateReport {
        table: coll_name,
        columns: vec![
            "id".into(),
            "file_name".into(),
            "content".into(),
            "content_hash".into(),
            "updated_at".into(),
        ],
        added_columns: added,
    })
}

pub async fn list_files(url: &str, password: &str, table: &str) -> Result<Vec<FileRecord>> {
    let coll_name = collection_name(table)?;
    let coll = collection(url, password, &coll_name).await?;
    let mut cursor = coll.find(doc! {}).await.context("mongodb find")?;
    let mut out = Vec::new();
    while let Some(res) = cursor.next().await {
        let doc = res.context("mongodb cursor")?;
        let file_name = doc.get_str("file_name").unwrap_or("").to_string();
        if file_name.is_empty() {
            continue;
        }
        out.push(FileRecord {
            id: doc.get_str("id").unwrap_or("").to_string(),
            file_name,
            file_path: String::new(),
            content: doc.get_str("content").unwrap_or("").to_string(),
            content_hash: doc.get_str("content_hash").unwrap_or("").to_string(),
            updated_at: doc.get_str("updated_at").unwrap_or("").to_string(),
            connection_id: None,
        });
    }
    out.sort_by(|a, b| a.file_name.cmp(&b.file_name));
    Ok(out)
}

pub async fn upsert_file(url: &str, password: &str, table: &str, rec: &FileRecord) -> Result<()> {
    let coll_name = collection_name(table)?;
    let coll = collection(url, password, &coll_name).await?;
    let doc = doc! {
        "id": &rec.id,
        "file_name": &rec.file_name,
        "content": &rec.content,
        "content_hash": &rec.content_hash,
        "updated_at": &rec.updated_at,
    };
    let opts = ReplaceOptions::builder().upsert(true).build();
    coll.replace_one(doc! { "file_name": &rec.file_name }, doc)
        .with_options(opts)
        .await
        .context("mongodb upsert")?;
    Ok(())
}

pub async fn delete_file(url: &str, password: &str, table: &str, file_name: &str) -> Result<()> {
    let coll_name = collection_name(table)?;
    let coll = collection(url, password, &coll_name).await?;
    coll.delete_one(doc! { "file_name": file_name })
        .await
        .context("mongodb delete")?;
    Ok(())
}

pub async fn test_connection(
    url: &str,
    password: &str,
    table: &str,
) -> Result<SchemaMigrateReport> {
    ensure_schema(url, password, table).await
}
