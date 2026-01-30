//! Example demonstrating fetch semantics:
//! - fetch_one* returns the first row (no error on multiple rows)
//! - fetch_one_strict* errors on multiple rows
//! - fetch_opt* returns Option
//!
//! Run with:
//!   cargo run --example fetch_semantics -p pgorm
//!
//! Requires:
//!   DATABASE_URL=postgres://postgres:postgres@localhost/pgorm_example

use pgorm::{OrmError, OrmResult, RowExt, query};
use std::env;

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

    query("DROP TABLE IF EXISTS items CASCADE").execute(&client).await?;
    query(
        "CREATE TABLE items (
            id BIGSERIAL PRIMARY KEY,
            name TEXT NOT NULL
        )",
    )
    .execute(&client)
    .await?;

    // Insert 2 rows with the same name.
    query("INSERT INTO items (name) VALUES ($1), ($1)")
        .bind("dup")
        .execute(&client)
        .await?;

    // fetch_one: returns the first row (even if multiple rows match).
    let row = query("SELECT id, name FROM items WHERE name = $1 ORDER BY id")
        .bind("dup")
        .fetch_one(&client)
        .await?;
    let id: i64 = row.try_get_column("id")?;
    let name: String = row.try_get_column("name")?;
    println!("fetch_one => id={id} name={name}");

    // fetch_one_strict: errors if multiple rows match.
    match query("SELECT id, name FROM items WHERE name = $1 ORDER BY id")
        .bind("dup")
        .fetch_one_strict(&client)
        .await
    {
        Ok(_) => println!("unexpected: strict query succeeded"),
        Err(OrmError::TooManyRows { expected, got }) => {
            println!("fetch_one_strict => TooManyRows (expected {expected}, got {got})")
        }
        Err(e) => println!("fetch_one_strict => other error: {e}"),
    }

    // fetch_opt: Ok(None) when no rows match.
    let row = query("SELECT id FROM items WHERE id = $1")
        .bind(9999_i64)
        .fetch_opt(&client)
        .await?;
    println!("fetch_opt => {}", if row.is_some() { "Some(row)" } else { "None" });

    // fetch_one: NotFound when no rows match.
    match query("SELECT id FROM items WHERE id = $1")
        .bind(9999_i64)
        .fetch_one(&client)
        .await
    {
        Ok(_) => println!("unexpected: missing row found"),
        Err(OrmError::NotFound(_)) => println!("fetch_one => NotFound (as expected)"),
        Err(e) => println!("fetch_one => other error: {e}"),
    }

    Ok(())
}

