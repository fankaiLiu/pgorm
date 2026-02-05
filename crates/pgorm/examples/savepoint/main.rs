//! Example demonstrating savepoints and nested transactions.
//!
//! Run with:
//!   cargo run --example savepoint -p pgorm
//!
//! Requires:
//!   DATABASE_URL=postgres://postgres:postgres@localhost/pgorm_example

use pgorm::{OrmError, OrmResult, RowExt, TransactionExt, query};
use std::env;

#[tokio::main]
async fn main() -> OrmResult<()> {
    dotenvy::dotenv().ok();
    let database_url = env::var("DATABASE_URL")
        .map_err(|_| OrmError::Connection("DATABASE_URL is not set".into()))?;

    let (mut client, connection) = tokio_postgres::connect(&database_url, tokio_postgres::NoTls)
        .await
        .map_err(OrmError::from_db_error)?;
    tokio::spawn(async move {
        let _ = connection.await;
    });

    // ── Setup ────────────────────────────────────────────────────────────────
    query("DROP TABLE IF EXISTS accounts CASCADE")
        .execute(&client)
        .await?;
    query(
        "CREATE TABLE accounts (
            id BIGSERIAL PRIMARY KEY,
            name TEXT NOT NULL,
            balance BIGINT NOT NULL DEFAULT 0
        )",
    )
    .execute(&client)
    .await?;
    query("INSERT INTO accounts (name, balance) VALUES ('Alice', 1000), ('Bob', 500)")
        .execute(&client)
        .await?;

    // ═══════════════════════════════════════════════════════════════════════════
    // Example A: Explicit savepoint with release/rollback
    // ═══════════════════════════════════════════════════════════════════════════
    println!("=== Example A: Explicit Savepoint API ===\n");

    pgorm::transaction!(&mut client, tx, {
        // Debit Alice
        query("UPDATE accounts SET balance = balance - $1 WHERE name = $2")
            .bind(200_i64)
            .bind("Alice")
            .execute(&tx)
            .await?;
        println!("[A1] Debited Alice by 200");

        // Try crediting Bob inside a savepoint
        let sp = tx.pgorm_savepoint("credit_bob").await?;
        println!("[A2] Created savepoint '{}'", sp.name());

        // Simulate: credit Bob
        query("UPDATE accounts SET balance = balance + $1 WHERE name = $2")
            .bind(200_i64)
            .bind("Bob")
            .execute(&sp)
            .await?;
        println!("[A3] Credited Bob by 200 (inside savepoint)");

        // Release the savepoint (make it permanent within the transaction)
        sp.release().await?;
        println!("[A4] Released savepoint — Bob's credit is now part of the transaction");

        Ok::<(), OrmError>(())
    })?;

    // Check balances
    let rows = query("SELECT name, balance FROM accounts ORDER BY name")
        .fetch_all(&client)
        .await?;
    println!("\n[A] Final balances:");
    for row in &rows {
        let name: String = row.try_get_column("name")?;
        let balance: i64 = row.try_get_column("balance")?;
        println!("    {name}: {balance}");
    }

    // Reset
    query("UPDATE accounts SET balance = CASE name WHEN 'Alice' THEN 1000 WHEN 'Bob' THEN 500 END")
        .execute(&client)
        .await?;

    // ═══════════════════════════════════════════════════════════════════════════
    // Example B: Savepoint rollback — partial failure doesn't affect outer tx
    // ═══════════════════════════════════════════════════════════════════════════
    println!("\n=== Example B: Savepoint Rollback ===\n");

    pgorm::transaction!(&mut client, tx, {
        // Debit Alice (this will survive)
        query("UPDATE accounts SET balance = balance - $1 WHERE name = $2")
            .bind(100_i64)
            .bind("Alice")
            .execute(&tx)
            .await?;
        println!("[B1] Debited Alice by 100");

        // Try a risky operation in a savepoint
        let sp = tx.pgorm_savepoint("risky_op").await?;

        query("UPDATE accounts SET balance = balance + $1 WHERE name = $2")
            .bind(100_i64)
            .bind("Bob")
            .execute(&sp)
            .await?;
        println!("[B2] Credited Bob by 100 (inside savepoint)");

        // Decide to roll back
        sp.rollback().await?;
        println!("[B3] Rolled back savepoint — Bob's credit is undone");

        Ok::<(), OrmError>(())
    })?;

    let rows = query("SELECT name, balance FROM accounts ORDER BY name")
        .fetch_all(&client)
        .await?;
    println!("\n[B] Final balances (Bob should still be 500):");
    for row in &rows {
        let name: String = row.try_get_column("name")?;
        let balance: i64 = row.try_get_column("balance")?;
        println!("    {name}: {balance}");
    }

    // Reset
    query("UPDATE accounts SET balance = CASE name WHEN 'Alice' THEN 1000 WHEN 'Bob' THEN 500 END")
        .execute(&client)
        .await?;

    // ═══════════════════════════════════════════════════════════════════════════
    // Example C: savepoint! macro — automatic release/rollback
    // ═══════════════════════════════════════════════════════════════════════════
    println!("\n=== Example C: savepoint! Macro ===\n");

    pgorm::transaction!(&mut client, tx, {
        query("UPDATE accounts SET balance = balance - $1 WHERE name = $2")
            .bind(50_i64)
            .bind("Alice")
            .execute(&tx)
            .await?;
        println!("[C1] Debited Alice by 50");

        // Named savepoint via macro — auto commits on Ok, rolls back on Err
        let result: Result<(), OrmError> = pgorm::savepoint!(tx, "bonus", sp, {
            query("UPDATE accounts SET balance = balance + $1 WHERE name = $2")
                .bind(50_i64)
                .bind("Bob")
                .execute(&sp)
                .await?;
            println!("[C2] Credited Bob by 50 (savepoint macro)");
            Ok(())
        });
        println!("[C3] Savepoint result: {:?}", result.is_ok());

        Ok::<(), OrmError>(())
    })?;

    let rows = query("SELECT name, balance FROM accounts ORDER BY name")
        .fetch_all(&client)
        .await?;
    println!("\n[C] Final balances:");
    for row in &rows {
        let name: String = row.try_get_column("name")?;
        let balance: i64 = row.try_get_column("balance")?;
        println!("    {name}: {balance}");
    }

    // Reset
    query("UPDATE accounts SET balance = CASE name WHEN 'Alice' THEN 1000 WHEN 'Bob' THEN 500 END")
        .execute(&client)
        .await?;

    // ═══════════════════════════════════════════════════════════════════════════
    // Example D: nested_transaction! macro
    // ═══════════════════════════════════════════════════════════════════════════
    println!("\n=== Example D: nested_transaction! Macro ===\n");

    pgorm::transaction!(&mut client, tx, {
        query("UPDATE accounts SET balance = balance - $1 WHERE name = $2")
            .bind(300_i64)
            .bind("Alice")
            .execute(&tx)
            .await?;
        println!("[D1] Debited Alice by 300");

        // Nested transaction (auto-creates anonymous savepoint)
        pgorm::nested_transaction!(tx, inner, {
            query("UPDATE accounts SET balance = balance + $1 WHERE name = $2")
                .bind(300_i64)
                .bind("Bob")
                .execute(&inner)
                .await?;
            println!("[D2] Credited Bob by 300 (nested transaction)");
            Ok::<(), OrmError>(())
        })?;

        println!("[D3] Nested transaction committed");
        Ok::<(), OrmError>(())
    })?;

    let rows = query("SELECT name, balance FROM accounts ORDER BY name")
        .fetch_all(&client)
        .await?;
    println!("\n[D] Final balances:");
    for row in &rows {
        let name: String = row.try_get_column("name")?;
        let balance: i64 = row.try_get_column("balance")?;
        println!("    {name}: {balance}");
    }

    // Reset
    query("UPDATE accounts SET balance = CASE name WHEN 'Alice' THEN 1000 WHEN 'Bob' THEN 500 END")
        .execute(&client)
        .await?;

    // ═══════════════════════════════════════════════════════════════════════════
    // Example E: Batch processing — per-record savepoints
    // ═══════════════════════════════════════════════════════════════════════════
    println!("\n=== Example E: Batch Processing with Savepoints ===\n");

    // Insert some items to process
    query("DROP TABLE IF EXISTS items CASCADE")
        .execute(&client)
        .await?;
    query("CREATE TABLE items (id BIGSERIAL PRIMARY KEY, value INT NOT NULL, processed BOOLEAN DEFAULT FALSE)")
        .execute(&client)
        .await?;
    query("INSERT INTO items (value) VALUES (10), (20), (-1), (40), (50)")
        .execute(&client)
        .await?;

    pgorm::transaction!(&mut client, tx, {
        let rows = query("SELECT id, value FROM items ORDER BY id")
            .fetch_all(&tx)
            .await?;

        let mut success = 0_i32;
        let mut failed = 0_i32;

        for row in &rows {
            let id: i64 = row.try_get_column("id")?;
            let value: i32 = row.try_get_column("value")?;

            // Each record gets its own savepoint
            let result: Result<(), OrmError> = pgorm::savepoint!(tx, sp, {
                if value < 0 {
                    return Err(OrmError::validation(format!("invalid value {value} for item {id}")));
                }
                query("UPDATE items SET processed = TRUE WHERE id = $1")
                    .bind(id)
                    .execute(&sp)
                    .await?;
                Ok(())
            });

            match result {
                Ok(()) => {
                    success += 1;
                    println!("[E] Processed item {id} (value={value})");
                }
                Err(e) => {
                    failed += 1;
                    println!("[E] Failed item {id}: {e}");
                }
            }
        }

        println!("\n[E] Batch complete: {success} succeeded, {failed} failed");
        Ok::<(), OrmError>(())
    })?;

    let rows = query("SELECT id, value, processed FROM items ORDER BY id")
        .fetch_all(&client)
        .await?;
    println!("\n[E] Final items:");
    for row in &rows {
        let id: i64 = row.try_get_column("id")?;
        let value: i32 = row.try_get_column("value")?;
        let processed: bool = row.try_get_column("processed")?;
        println!("    id={id} value={value:3} processed={processed}");
    }

    println!("\nDone.");
    Ok(())
}
