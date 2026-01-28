//! Transaction example for pgorm using the built-in deadpool-postgres pool.
//!
//! Run with:
//!   cargo run --example transaction_pool -p pgorm --features "pool"
//!
//! Set DATABASE_URL in .env file or environment variable:
//!   DATABASE_URL=postgres://postgres:postgres@localhost/pgorm_example

use pgorm::{GenericClient, OrmError, FromRow, create_pool, query};
use std::env;

#[derive(Debug, FromRow)]
struct Account {
    id: i64,
    name: String,
    balance: i64,
}

async fn transfer<C: GenericClient>(
    client: &C,
    from_id: i64,
    to_id: i64,
    amount: i64,
) -> Result<(), OrmError> {
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

    query("UPDATE accounts SET balance = balance - $1 WHERE id = $2")
        .bind(amount)
        .bind(from_id)
        .execute(client)
        .await?;

    query("UPDATE accounts SET balance = balance + $1 WHERE id = $2")
        .bind(amount)
        .bind(to_id)
        .execute(client)
        .await?;

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), OrmError> {
    dotenvy::dotenv().ok();

    let database_url = env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set in .env or environment");

    let pool = create_pool(&database_url)?;
    let mut client = pool.get().await?;

    // Setup
    query(
        "CREATE TABLE IF NOT EXISTS accounts (
            id BIGSERIAL PRIMARY KEY,
            name TEXT NOT NULL,
            balance BIGINT NOT NULL DEFAULT 0
        )",
    )
    .execute(&client)
    .await?;

    query("DELETE FROM accounts").execute(&client).await?;

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

    println!("\n=== Successful Transaction ===");
    println!("Transferring $200 from Alice to Bob...");

    pgorm::transaction!(&mut client, tx, {
        transfer(&tx, alice_id, bob_id, 200).await?;
        Ok(())
    })?;

    let accounts: Vec<Account> = query("SELECT id, name, balance FROM accounts ORDER BY id")
        .fetch_all_as(&client)
        .await?;

    println!("After successful transfer:");
    for acc in &accounts {
        println!("  {}: ${}", acc.name, acc.balance);
    }

    println!("\n=== Failed Transaction (Rollback) ===");
    println!("Attempting to transfer $10000 from Alice to Bob (should fail)...");

    match pgorm::transaction!(&mut client, tx, {
        transfer(&tx, alice_id, bob_id, 10000).await?;
        Ok(())
    }) {
        Ok(()) => println!("Transfer succeeded"),
        Err(e) => {
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

    Ok(())
}
