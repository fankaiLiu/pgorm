//! Example for productized known limitations:
//! - `input_as` on `Option<Option<T>>`
//! - multiple filter operations on one `QueryParams` field
//! - bulk optimistic locking check in `update_by_ids`
//!
//! Run with:
//!   cargo run --example productized_limits -p pgorm
//!
//! Optional DB section:
//!   DATABASE_URL=postgres://postgres:postgres@localhost/pgorm_example

#![allow(dead_code)]

use pgorm::changeset::ValidationErrors;
use pgorm::{FromRow, InsertModel, Model, OrmError, OrmResult, QueryParams, UpdateModel, query};
use std::env;

#[derive(Debug, Clone, FromRow, Model)]
#[orm(table = "devices")]
struct Device {
    #[orm(id)]
    id: i64,
    label: String,
    external_id: Option<uuid::Uuid>,
    version: i32,
}

#[derive(Debug, InsertModel)]
#[orm(table = "devices")]
#[orm(input)]
struct NewDevice {
    label: String,
    #[orm(uuid, input_as = "String")]
    external_id: Option<Option<uuid::Uuid>>,
}

#[derive(Debug, UpdateModel)]
#[orm(table = "devices", id_column = "id")]
#[orm(input)]
struct DevicePatch {
    label: Option<String>,
    #[orm(uuid, input_as = "String")]
    external_id: Option<Option<uuid::Uuid>>,
    #[orm(version)]
    version: i32,
}

#[derive(Debug, QueryParams)]
#[orm(model = "Device")]
struct DeviceSearchParams<'a> {
    // Multiple ops on the same field across multiple attrs.
    #[orm(eq(DeviceQuery::COL_LABEL))]
    #[orm(ilike(DeviceQuery::COL_LABEL))]
    label: Option<&'a str>,

    // Multiple ops on the same field in one attr list.
    #[orm(gte(DeviceQuery::COL_VERSION), lte(DeviceQuery::COL_VERSION))]
    version: Option<i32>,

    #[orm(order_by_desc)]
    order_by_desc: Option<&'a str>,
}

fn print_validation_errors(errs: &ValidationErrors) {
    println!("{}", serde_json::to_string_pretty(errs).unwrap());
}

fn demo_input_as_option_option() {
    println!("=== input_as on Option<Option<T>> ===");

    let create_json = r#"
        {
          "label": "edge-router",
          "external_id": "550e8400-e29b-41d4-a716-446655440000"
        }
    "#;
    let create_input: NewDeviceInput = serde_json::from_str(create_json).unwrap();
    let create_model = create_input.try_into_model().unwrap();
    println!("create external_id parsed: {:?}", create_model.external_id);

    // For explicit "set NULL" intent in Rust value form, use `Some(None)`.
    let clear_input = DevicePatchInput {
        label: None,
        external_id: Some(None),
        version: Some(1),
    };
    let clear_patch = clear_input.try_into_patch().unwrap();
    println!(
        "patch external_id parsed: {:?} (Some(None) => set NULL)",
        clear_patch.external_id
    );

    let bad_json = r#"
        {
          "label": "bad-device",
          "external_id": "not-a-uuid"
        }
    "#;
    let bad_input: NewDeviceInput = serde_json::from_str(bad_json).unwrap();
    if let Err(errs) = bad_input.try_into_model() {
        println!("invalid uuid gives ValidationErrors:");
        print_validation_errors(&errs);
    }
}

fn demo_query_params_multi_ops() -> OrmResult<()> {
    println!("\n=== QueryParams multiple ops per field ===");

    let params = DeviceSearchParams {
        label: Some("edge-router"),
        version: Some(1),
        order_by_desc: Some("id"),
    };
    let q = params.into_query()?;
    println!("built query builder = {q:?}");

    Ok(())
}

async fn demo_bulk_version_check(database_url: &str) -> OrmResult<()> {
    println!("\n=== Bulk optimistic locking with version check ===");

    let (client, connection) = tokio_postgres::connect(database_url, tokio_postgres::NoTls)
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
            external_id UUID NULL,
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

    let ok_patch = DevicePatch {
        label: Some("patched".into()),
        external_id: None,
        version: 0,
    };
    let affected = ok_patch.update_by_ids(&client, ids.clone()).await?;
    println!("bulk update with version=0 affected rows: {affected}");

    // Simulate concurrent update on one row: one row version becomes 2, the other remains 1.
    query("UPDATE devices SET version = version + 1 WHERE id = $1")
        .bind(ids[0])
        .execute(&client)
        .await?;

    let stale_patch = DevicePatch {
        label: Some("stale-attempt".into()),
        external_id: None,
        version: 1,
    };
    match stale_patch.update_by_ids(&client, ids.clone()).await {
        Err(OrmError::StaleRecord {
            table,
            expected_version,
            ..
        }) => {
            println!("stale detected for table={table}, expected_version={expected_version}");
        }
        Ok(n) => {
            println!("unexpected success: updated {n} rows");
        }
        Err(e) => return Err(e),
    }

    Ok(())
}

#[tokio::main]
async fn main() -> OrmResult<()> {
    dotenvy::dotenv().ok();

    demo_input_as_option_option();
    demo_query_params_multi_ops()?;

    match env::var("DATABASE_URL") {
        Ok(url) => demo_bulk_version_check(&url).await?,
        Err(_) => {
            println!("\nDATABASE_URL not set; skipping bulk optimistic-locking demo.");
        }
    }

    Ok(())
}
