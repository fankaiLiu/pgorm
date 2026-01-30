//! Example demonstrating jsonb support.
//!
//! Run with:
//!   cargo run --example jsonb -p pgorm
//!
//! Requires:
//!   DATABASE_URL=postgres://postgres:postgres@localhost/pgorm_example

use pgorm::{Json, OrmError, OrmResult, RowExt, query};
use serde::{Deserialize, Serialize};
use std::env;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Meta {
    tags: Vec<String>,
    active: bool,
}

#[tokio::main]
async fn main() -> OrmResult<()> {
    dotenvy::dotenv().ok();
    let database_url = env::var("DATABASE_URL")
        .map_err(|_| OrmError::Connection("DATABASE_URL is not set".into()))?;

    let (client, connection) = tokio_postgres::connect(&database_url, tokio_postgres::NoTls)
        .await
        .map_err(OrmError::from_db_error)?;
    tokio::spawn(async move {
        let _ = connection.await;
    });

    query("DROP TABLE IF EXISTS users CASCADE")
        .execute(&client)
        .await?;
    query(
        "CREATE TABLE users (
            id BIGSERIAL PRIMARY KEY,
            meta JSONB NOT NULL
        )",
    )
    .execute(&client)
    .await?;

    // Strongly-typed jsonb: Json<T>
    let typed_meta = Json(Meta {
        tags: vec!["admin".into(), "staff".into()],
        active: true,
    });
    let row = query("INSERT INTO users (meta) VALUES ($1) RETURNING id, meta")
        .bind(typed_meta)
        .fetch_one(&client)
        .await?;
    let typed_id: i64 = row.try_get_column("id")?;
    let typed_meta: Json<Meta> = row.try_get_column("meta")?;
    println!("typed jsonb row: id={typed_id} meta={:?}", typed_meta.0);

    // Dynamic jsonb: serde_json::Value
    let dynamic_meta = serde_json::json!({"theme":"dark","beta":true});
    let row = query("INSERT INTO users (meta) VALUES ($1) RETURNING id, meta")
        .bind(dynamic_meta)
        .fetch_one(&client)
        .await?;
    let dyn_id: i64 = row.try_get_column("id")?;
    let dyn_meta: serde_json::Value = row.try_get_column("meta")?;
    println!("dynamic jsonb row: id={dyn_id} meta={dyn_meta}");

    // Query inside jsonb.
    let theme: Option<String> = query("SELECT meta->>'theme' FROM users WHERE id = $1")
        .bind(dyn_id)
        .fetch_scalar_opt(&client)
        .await?;
    println!("theme for id={dyn_id}: {:?}", theme.as_deref());

    Ok(())
}
