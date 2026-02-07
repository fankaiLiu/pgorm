//! Example demonstrating transaction isolation/read-only/deferrable options.
//!
//! Run with:
//!   cargo run --example transaction_options -p pgorm
//!
//! Optional (run against a real DB):
//!   DATABASE_URL=postgres://postgres:postgres@localhost/pgorm_example

use pgorm::{OrmError, OrmResult, TransactionIsolation, TransactionOptions, query};
use std::env;

#[tokio::main]
async fn main() -> OrmResult<()> {
    dotenvy::dotenv().ok();

    let db_url = match env::var("DATABASE_URL") {
        Ok(v) => v,
        Err(_) => {
            println!("DATABASE_URL not set; skipping DB execution.");
            println!(
                "sample options: {:?}",
                TransactionOptions::new()
                    .isolation_level(TransactionIsolation::Serializable)
                    .read_only(true)
                    .deferrable(true)
            );
            return Ok(());
        }
    };

    let (mut client, connection) = tokio_postgres::connect(&db_url, tokio_postgres::NoTls)
        .await
        .map_err(OrmError::from_db_error)?;
    tokio::spawn(async move {
        let _ = connection.await;
    });

    query("DROP TABLE IF EXISTS tx_demo")
        .execute(&client)
        .await?;
    query(
        "CREATE TABLE tx_demo (
            id BIGSERIAL PRIMARY KEY,
            amount BIGINT NOT NULL
        )",
    )
    .execute(&client)
    .await?;
    query("INSERT INTO tx_demo (amount) VALUES (100), (200)")
        .execute(&client)
        .await?;

    // Function API: begin_transaction_with(...)
    let write_opts = TransactionOptions::new()
        .isolation_level(TransactionIsolation::ReadCommitted)
        .read_only(false);
    let tx = pgorm::begin_transaction_with(&mut client, write_opts).await?;
    query("UPDATE tx_demo SET amount = amount + 10 WHERE id = 1")
        .execute(&tx)
        .await?;
    tx.commit().await.map_err(OrmError::from_db_error)?;

    // Macro API: transaction_with!(..., opts, {...})
    let read_opts = TransactionOptions::new()
        .isolation_level(TransactionIsolation::Serializable)
        .read_only(true)
        .deferrable(true);
    let total: i64 = pgorm::transaction_with!(&mut client, tx, read_opts, {
        let sum: i64 = query("SELECT COALESCE(SUM(amount), 0) FROM tx_demo")
            .fetch_scalar_one(&tx)
            .await?;
        Ok::<i64, OrmError>(sum)
    })?;
    println!("read-only total amount: {total}");

    Ok(())
}
