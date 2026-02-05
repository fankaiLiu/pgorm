//! Example demonstrating PgEnum derive for PostgreSQL ENUM types.
//!
//! Run with:
//!   cargo run --example pg_enum -p pgorm
//!
//! Requires:
//!   DATABASE_URL=postgres://postgres:postgres@localhost/pgorm_example

use pgorm::{OrmError, OrmResult, PgEnum, RowExt, query};
use std::env;

#[derive(PgEnum, Debug, Clone, PartialEq)]
#[orm(pg_type = "order_status")]
pub enum OrderStatus {
    #[orm(rename = "pending")]
    Pending,
    #[orm(rename = "processing")]
    Processing,
    #[orm(rename = "shipped")]
    Shipped,
    #[orm(rename = "delivered")]
    Delivered,
    #[orm(rename = "cancelled")]
    Cancelled,
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

    // ── Setup ────────────────────────────────────────────────────────────────
    query("DROP TABLE IF EXISTS orders CASCADE")
        .execute(&client)
        .await?;
    query("DROP TYPE IF EXISTS order_status CASCADE")
        .execute(&client)
        .await?;
    query(
        "CREATE TYPE order_status AS ENUM (
            'pending', 'processing', 'shipped', 'delivered', 'cancelled'
        )",
    )
    .execute(&client)
    .await?;
    query(
        "CREATE TABLE orders (
            id BIGSERIAL PRIMARY KEY,
            user_id BIGINT NOT NULL,
            status order_status NOT NULL DEFAULT 'pending',
            total NUMERIC(10,2) NOT NULL
        )",
    )
    .execute(&client)
    .await?;

    // ── Insert with enum value ───────────────────────────────────────────────
    let row = query(
        "INSERT INTO orders (user_id, status, total) VALUES ($1, $2, $3) RETURNING id, status",
    )
    .bind(1_i64)
    .bind(OrderStatus::Pending)
    .bind(99.99_f64)
    .fetch_one(&client)
    .await?;
    let id: i64 = row.try_get_column("id")?;
    let status: OrderStatus = row.try_get_column("status")?;
    println!("[1] Inserted order id={id}, status={status:?}");

    // Insert more orders
    query("INSERT INTO orders (user_id, status, total) VALUES ($1, $2, $3)")
        .bind(2_i64)
        .bind(OrderStatus::Shipped)
        .bind(149.50_f64)
        .execute(&client)
        .await?;

    query("INSERT INTO orders (user_id, status, total) VALUES ($1, $2, $3)")
        .bind(1_i64)
        .bind(OrderStatus::Processing)
        .bind(200.00_f64)
        .execute(&client)
        .await?;

    // ── Query by enum value ──────────────────────────────────────────────────
    let rows = query("SELECT id, user_id, status, total FROM orders WHERE status = $1")
        .bind(OrderStatus::Pending)
        .fetch_all(&client)
        .await?;
    println!("\n[2] Pending orders ({} found):", rows.len());
    for row in &rows {
        let id: i64 = row.try_get_column("id")?;
        let status: OrderStatus = row.try_get_column("status")?;
        println!("    order id={id}, status={status:?}");
    }

    // ── Update enum value ────────────────────────────────────────────────────
    query("UPDATE orders SET status = $1 WHERE id = $2")
        .bind(OrderStatus::Delivered)
        .bind(id)
        .execute(&client)
        .await?;

    let row = query("SELECT status FROM orders WHERE id = $1")
        .bind(id)
        .fetch_one(&client)
        .await?;
    let updated: OrderStatus = row.try_get_column("status")?;
    println!("\n[3] Updated order id={id} to status={updated:?}");

    // ── Query all, read enum from each row ───────────────────────────────────
    let rows = query("SELECT id, status FROM orders ORDER BY id")
        .fetch_all(&client)
        .await?;
    println!("\n[4] All orders:");
    for row in &rows {
        let id: i64 = row.try_get_column("id")?;
        let status: OrderStatus = row.try_get_column("status")?;
        println!("    id={id}, status={status:?}");
    }

    println!("\nDone.");
    Ok(())
}
