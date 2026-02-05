//! Example demonstrating PgComposite derive for PostgreSQL composite types.
//!
//! Run with:
//!   cargo run --example pg_composite -p pgorm
//!
//! Requires:
//!   DATABASE_URL=postgres://postgres:postgres@localhost/pgorm_example

use pgorm::{OrmError, OrmResult, PgComposite, RowExt, query};
use std::env;

#[derive(PgComposite, Debug, Clone)]
#[orm(pg_type = "address")]
pub struct Address {
    pub street: String,
    pub city: String,
    pub zip_code: String,
    pub country: String,
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
    query("DROP TABLE IF EXISTS contacts CASCADE")
        .execute(&client)
        .await?;
    query("DROP TYPE IF EXISTS address CASCADE")
        .execute(&client)
        .await?;
    query(
        "CREATE TYPE address AS (
            street TEXT,
            city TEXT,
            zip_code TEXT,
            country TEXT
        )",
    )
    .execute(&client)
    .await?;
    query(
        "CREATE TABLE contacts (
            id BIGSERIAL PRIMARY KEY,
            name TEXT NOT NULL,
            home_address address NOT NULL
        )",
    )
    .execute(&client)
    .await?;

    // ── Insert with composite value ──────────────────────────────────────────
    let addr = Address {
        street: "123 Main St".into(),
        city: "San Francisco".into(),
        zip_code: "94105".into(),
        country: "USA".into(),
    };

    let row =
        query("INSERT INTO contacts (name, home_address) VALUES ($1, $2) RETURNING id, name, home_address")
            .bind("Alice")
            .bind(addr.clone())
            .fetch_one(&client)
            .await?;
    let id: i64 = row.try_get_column("id")?;
    let name: String = row.try_get_column("name")?;
    let read_addr: Address = row.try_get_column("home_address")?;
    println!("[1] Inserted: id={id}, name={name}");
    println!(
        "    address: {} {}, {} {}",
        read_addr.street, read_addr.city, read_addr.zip_code, read_addr.country
    );

    // Insert another contact
    let addr2 = Address {
        street: "456 Oak Ave".into(),
        city: "New York".into(),
        zip_code: "10001".into(),
        country: "USA".into(),
    };
    query("INSERT INTO contacts (name, home_address) VALUES ($1, $2)")
        .bind("Bob")
        .bind(addr2)
        .execute(&client)
        .await?;

    // ── Read all ─────────────────────────────────────────────────────────────
    let rows = query("SELECT id, name, home_address FROM contacts ORDER BY id")
        .fetch_all(&client)
        .await?;
    println!("\n[2] All contacts:");
    for row in &rows {
        let id: i64 = row.try_get_column("id")?;
        let name: String = row.try_get_column("name")?;
        let addr: Address = row.try_get_column("home_address")?;
        println!(
            "    id={id} name={name:8} city={}, country={}",
            addr.city, addr.country
        );
    }

    // ── Query on composite field ─────────────────────────────────────────────
    // PostgreSQL allows accessing composite fields with (column).field syntax
    let rows = query("SELECT id, name FROM contacts WHERE (home_address).city = $1")
        .bind("San Francisco")
        .fetch_all(&client)
        .await?;
    println!("\n[3] Contacts in San Francisco ({} found):", rows.len());
    for row in &rows {
        let name: String = row.try_get_column("name")?;
        println!("    {name}");
    }

    println!("\nDone.");
    Ok(())
}
