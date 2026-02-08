//! Example demonstrating bulk optimistic locking with `update_by_ids`.
//!
//! Run with:
//!   cargo run --example bulk_optimistic_locking -p pgorm
//!
//! Optional:
//!   DATABASE_URL=postgres://postgres:postgres@localhost/pgorm_example

#![allow(dead_code)]

use pgorm::{FromRow, Model, OrmError, OrmResult, UpdateModel, query};
use std::env;

#[derive(Debug, Clone, FromRow, Model)]
#[orm(table = "devices")]
struct Device {
    #[orm(id)]
    id: i64,
    label: String,
    version: i32,
}

#[derive(Debug, UpdateModel)]
#[orm(table = "devices", id_column = "id")]
struct DevicePatch {
    label: Option<String>,
    #[orm(version)]
    version: i32,
}

#[tokio::main]
async fn main() -> OrmResult<()> {
    dotenvy::dotenv().ok();

    let database_url = match env::var("DATABASE_URL") {
        Ok(v) => v,
        Err(_) => {
            println!("DATABASE_URL not set; skipping DB demo.");
            return Ok(());
        }
    };

    let (client, connection) = tokio_postgres::connect(&database_url, tokio_postgres::NoTls)
        .await
        .map_err(OrmError::from_db_error)?;
    tokio::spawn(async move {
        let _ = connection.await;
    });

    query("DROP TABLE IF EXISTS devices")
        .execute(&client)
        .await?;
    query(
        "CREATE TABLE devices (
            id BIGSERIAL PRIMARY KEY,
            label TEXT NOT NULL,
            version INT NOT NULL DEFAULT 0
        )",
    )
    .execute(&client)
    .await?;

    query("INSERT INTO devices (label, version) VALUES ($1, 0), ($2, 0)")
        .bind("edge-router")
        .bind("core-router")
        .execute(&client)
        .await?;

    let ids: Vec<i64> = query("SELECT id FROM devices ORDER BY id")
        .fetch_scalar_all::<i64>(&client)
        .await?;
    println!("target ids: {ids:?}");

    let ok_patch = DevicePatch {
        label: Some("patched".into()),
        version: 0,
    };
    let affected = ok_patch.update_by_ids(&client, ids.clone()).await?;
    println!("bulk update with version=0 affected rows: {affected}");

    // Simulate stale request: all rows are now at version=1, but request still uses version=0.
    let stale_patch = DevicePatch {
        label: Some("stale-attempt".into()),
        version: 0,
    };
    match stale_patch.update_by_ids(&client, ids).await {
        Err(OrmError::StaleRecord {
            table,
            expected_version,
            ..
        }) => {
            println!("stale detected for table={table}, expected_version={expected_version}");
        }
        Ok(n) => println!("unexpected success: updated {n} rows"),
        Err(e) => return Err(e),
    }

    let rows: Vec<Device> = query("SELECT id, label, version FROM devices ORDER BY id")
        .fetch_all_as::<Device>(&client)
        .await?;
    for row in rows {
        println!("row = {row:?}");
    }

    Ok(())
}
