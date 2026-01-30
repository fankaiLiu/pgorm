//! Example demonstrating InsertModel::insert_many / insert_many_returning (UNNEST bulk insert).
//!
//! Run with:
//!   cargo run --example insert_many -p pgorm
//!
//! Requires:
//!   DATABASE_URL=postgres://postgres:postgres@localhost/pgorm_example

use chrono::{DateTime, Utc};
use pgorm::{FromRow, InsertModel, Model, OrmError, OrmResult, query};
use std::env;

#[derive(Debug, Clone, FromRow, Model)]
#[orm(table = "products")]
struct Product {
    #[orm(id)]
    id: i64,
    sku: String,
    name: String,
    price_cents: i64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, InsertModel)]
#[orm(table = "products", returning = "Product")]
struct NewProduct {
    sku: String,
    name: String,
    price_cents: i64,

    // If None, the macro fills it with `Utc::now()` at insert time.
    #[orm(auto_now_add)]
    created_at: Option<DateTime<Utc>>,
    #[orm(auto_now_add)]
    updated_at: Option<DateTime<Utc>>,
}

#[tokio::main]
async fn main() -> OrmResult<()> {
    dotenvy::dotenv().ok();
    let database_url =
        env::var("DATABASE_URL").map_err(|_| OrmError::Connection("DATABASE_URL is not set".into()))?;

    let (client, connection) = tokio_postgres::connect(&database_url, tokio_postgres::NoTls)
        .await
        .map_err(OrmError::from_db_error)?;
    tokio::spawn(async move {
        let _ = connection.await;
    });

    query("DROP TABLE IF EXISTS products CASCADE")
        .execute(&client)
        .await?;
    query(
        "CREATE TABLE products (
            id BIGSERIAL PRIMARY KEY,
            sku TEXT NOT NULL UNIQUE,
            name TEXT NOT NULL,
            price_cents BIGINT NOT NULL,
            created_at TIMESTAMPTZ NOT NULL,
            updated_at TIMESTAMPTZ NOT NULL
        )",
    )
    .execute(&client)
    .await?;

    let rows = vec![
        NewProduct {
            sku: "SKU-001".into(),
            name: "Keyboard".into(),
            price_cents: 7999,
            created_at: None,
            updated_at: None,
        },
        NewProduct {
            sku: "SKU-002".into(),
            name: "Mouse".into(),
            price_cents: 2999,
            created_at: None,
            updated_at: None,
        },
        NewProduct {
            sku: "SKU-003".into(),
            name: "Monitor".into(),
            price_cents: 19999,
            created_at: None,
            updated_at: None,
        },
    ];

    let inserted = NewProduct::insert_many_returning(&client, rows).await?;
    println!("inserted {} product(s):", inserted.len());
    for p in &inserted {
        println!(
            "- id={} sku={} name={} price_cents={} created_at={} updated_at={}",
            p.id, p.sku, p.name, p.price_cents, p.created_at, p.updated_at
        );
    }

    let total: i64 = pgorm::query("SELECT COUNT(*) FROM products")
        .fetch_scalar_one(&client)
        .await?;
    println!("\nproducts count = {total}");

    Ok(())
}

