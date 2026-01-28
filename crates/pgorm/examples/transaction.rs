//! Transaction example for pgorm
//!
//! Run with: cargo run --example transaction -p pgorm
//!
//! Set DATABASE_URL in .env file or environment variable:
//! DATABASE_URL=postgres://postgres:postgres@localhost/pgorm_example
//!
//! Demonstrates transaction support with the GenericClient trait.
//! Note: This example uses tokio_postgres directly to demonstrate transactions,
//! as transactions work with tokio_postgres::Transaction which implements GenericClient.

use pgorm::{query, FromRow, GenericClient, OrmError};
use std::env;
use tokio_postgres::NoTls;

#[derive(Debug, FromRow)]
struct Account {
    id: i64,
    name: String,
    balance: i64,
}

/// Transfer money between accounts - works with any GenericClient (connection or transaction)
async fn transfer<C: GenericClient>(
    client: &C,
    from_id: i64,
    to_id: i64,
    amount: i64,
) -> Result<(), OrmError> {
    // Check source balance
    let from_account: Account = query("SELECT id, name, balance FROM accounts WHERE id = $1")
        .bind(from_id)
        .fetch_one_as(client)
        .await?;

    if from_account.balance < amount {
        return Err(OrmError::Validation(format!(
            "Insufficient balance: {} < {}",
            from_account.balance, amount
        )));
    }

    // Deduct from source
    query("UPDATE accounts SET balance = balance - $1 WHERE id = $2")
        .bind(amount)
        .bind(from_id)
        .execute(client)
        .await?;

    // Add to destination
    query("UPDATE accounts SET balance = balance + $1 WHERE id = $2")
        .bind(amount)
        .bind(to_id)
        .execute(client)
        .await?;

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), OrmError> {
    // Load .env file
    dotenvy::dotenv().ok();

    // Read DATABASE_URL from environment
    let database_url = env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set in .env or environment");

    // Connect directly using tokio_postgres for transaction support
    let (mut client, connection) =
        tokio_postgres::connect(&database_url, NoTls)
            .await
            .map_err(OrmError::from_db_error)?;

    // Spawn the connection handler
    tokio::spawn(async move {
        if let Err(e) = connection.await {
            eprintln!("connection error: {}", e);
        }
    });

    // Setup
    client
        .execute(
            "CREATE TABLE IF NOT EXISTS accounts (
                id BIGSERIAL PRIMARY KEY,
                name TEXT NOT NULL,
                balance BIGINT NOT NULL DEFAULT 0
            )",
            &[],
        )
        .await
        .map_err(OrmError::from_db_error)?;

    client
        .execute("DELETE FROM accounts", &[])
        .await
        .map_err(OrmError::from_db_error)?;

    // Create accounts
    query("INSERT INTO accounts (name, balance) VALUES ($1, $2)")
        .bind("Alice")
        .bind(1000_i64)
        .execute(&client)
        .await?;

    query("INSERT INTO accounts (name, balance) VALUES ($1, $2)")
        .bind("Bob")
        .bind(500_i64)
        .execute(&client)
        .await?;

    let accounts: Vec<Account> = query("SELECT id, name, balance FROM accounts ORDER BY id")
        .fetch_all_as(&client)
        .await?;

    println!("Initial balances:");
    for acc in &accounts {
        println!("  {}: ${}", acc.name, acc.balance);
    }

    let alice_id = accounts[0].id;
    let bob_id = accounts[1].id;

    // ============================================
    // Successful transaction
    // ============================================
    println!("\n=== Successful Transaction ===");
    println!("Transferring $200 from Alice to Bob...");

    let tx = client
        .transaction()
        .await
        .map_err(OrmError::from_db_error)?;

    transfer(&tx, alice_id, bob_id, 200).await?;

    tx.commit().await.map_err(OrmError::from_db_error)?;

    let accounts: Vec<Account> = query("SELECT id, name, balance FROM accounts ORDER BY id")
        .fetch_all_as(&client)
        .await?;

    println!("After successful transfer:");
    for acc in &accounts {
        println!("  {}: ${}", acc.name, acc.balance);
    }

    // ============================================
    // Failed transaction (rolled back)
    // ============================================
    println!("\n=== Failed Transaction (Rollback) ===");
    println!("Attempting to transfer $10000 from Alice to Bob (should fail)...");

    let tx = client
        .transaction()
        .await
        .map_err(OrmError::from_db_error)?;

    match transfer(&tx, alice_id, bob_id, 10000).await {
        Ok(_) => {
            tx.commit().await.map_err(OrmError::from_db_error)?;
            println!("Transfer succeeded");
        }
        Err(e) => {
            tx.rollback().await.map_err(OrmError::from_db_error)?;
            println!("Transfer failed: {}", e);
            println!("Transaction rolled back");
        }
    }

    let accounts: Vec<Account> = query("SELECT id, name, balance FROM accounts ORDER BY id")
        .fetch_all_as(&client)
        .await?;

    println!("After failed transfer (unchanged):");
    for acc in &accounts {
        println!("  {}: ${}", acc.name, acc.balance);
    }

    // ============================================
    // Without transaction (for comparison)
    // ============================================
    println!("\n=== Without Transaction ===");
    println!("The same transfer function works without a transaction too:");

    transfer(&client, alice_id, bob_id, 100).await?;

    let accounts: Vec<Account> = query("SELECT id, name, balance FROM accounts ORDER BY id")
        .fetch_all_as(&client)
        .await?;

    println!("After direct transfer:");
    for acc in &accounts {
        println!("  {}: ${}", acc.name, acc.balance);
    }

    Ok(())
}
