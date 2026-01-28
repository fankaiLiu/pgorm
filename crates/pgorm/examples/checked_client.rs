//! Example demonstrating CheckedClient with automatic model registration
//!
//! Run with: cargo run --example checked_client -p pgorm --features "derive,pool,check"
//!
//! Set DATABASE_URL in .env file or environment variable:
//! DATABASE_URL=postgres://postgres:postgres@localhost/pgorm_example

use pgorm::{create_pool, query, CheckMode, CheckedClient, FromRow, Model, OrmError};
use std::env;

#[derive(Debug, FromRow, Model)]
#[orm(table = "products")]
#[allow(dead_code)]
struct Product {
    #[orm(id)]
    id: i64,
    name: String,
    price_cents: i64,
    in_stock: bool,
}

#[derive(Debug, FromRow, Model)]
#[orm(table = "categories")]
#[allow(dead_code)]
struct Category {
    #[orm(id)]
    id: i64,
    name: String,
}

#[tokio::main]
async fn main() -> Result<(), OrmError> {
    dotenvy::dotenv().ok();

    let database_url =
        env::var("DATABASE_URL").expect("DATABASE_URL must be set in .env or environment");

    let pool = create_pool(&database_url)?;
    let client = pool.get().await?;

    // Setup: Create tables
    client
        .execute("DROP TABLE IF EXISTS products CASCADE", &[])
        .await
        .map_err(OrmError::from_db_error)?;
    client
        .execute("DROP TABLE IF EXISTS categories CASCADE", &[])
        .await
        .map_err(OrmError::from_db_error)?;

    client
        .execute(
            "CREATE TABLE categories (
                id BIGSERIAL PRIMARY KEY,
                name TEXT NOT NULL
            )",
            &[],
        )
        .await
        .map_err(OrmError::from_db_error)?;

    client
        .execute(
            "CREATE TABLE products (
                id BIGSERIAL PRIMARY KEY,
                name TEXT NOT NULL,
                price_cents BIGINT NOT NULL,
                in_stock BOOLEAN NOT NULL DEFAULT true
            )",
            &[],
        )
        .await
        .map_err(OrmError::from_db_error)?;

    // Insert test data
    query("INSERT INTO categories (name) VALUES ($1)")
        .bind("Electronics")
        .execute(&client)
        .await?;

    query("INSERT INTO products (name, price_cents, in_stock) VALUES ($1, $2, $3)")
        .bind("Laptop")
        .bind(99999_i64)
        .bind(true)
        .execute(&client)
        .await?;

    println!("=== CheckedClient Demo ===\n");

    // ============================================
    // Example 1: Auto-registered models
    // ============================================
    println!("1. Auto-registration:");

    // Models are auto-registered via inventory - no manual register::<T>() needed!
    let checked = CheckedClient::new(&client);

    println!(
        "   Registered {} tables automatically",
        checked.registry().len()
    );

    for table in checked.registry().tables() {
        println!("   - Table '{}': {:?}", table.name, table.columns);
    }

    // ============================================
    // Example 2: WarnOnly mode (default)
    // ============================================
    println!("\n2. WarnOnly mode (default):");

    // Query against registered table - no warnings
    let products = Product::select_all(&checked).await?;
    println!("   Found {} products (no warnings)", products.len());

    // Query against unregistered table - prints warning but continues
    println!("   Querying unregistered table 'nonexistent':");
    let result = query("SELECT * FROM nonexistent")
        .fetch_all(&checked)
        .await;
    match result {
        Ok(_) => println!("   Query succeeded"),
        Err(e) => println!("   Query failed (expected): {}", e),
    }

    // ============================================
    // Example 3: Strict mode
    // ============================================
    println!("\n3. Strict mode:");

    let strict_client = CheckedClient::new(&client).strict();

    // Valid query works
    let products = Product::select_all(&strict_client).await?;
    println!("   Valid query succeeded: {} products", products.len());

    // Invalid query returns error
    println!("   Querying unregistered table in strict mode:");
    let result = query("SELECT * FROM nonexistent")
        .fetch_all(&strict_client)
        .await;
    match result {
        Ok(_) => println!("   Query succeeded (unexpected!)"),
        Err(OrmError::Validation(msg)) => {
            println!("   Validation error (expected): {}", msg)
        }
        Err(e) => println!("   Other error: {}", e),
    }

    // ============================================
    // Example 4: Disabled mode
    // ============================================
    println!("\n4. Disabled mode:");

    let disabled_client = CheckedClient::new(&client).check_mode(CheckMode::Disabled);

    // All queries pass without checking
    println!("   Querying unregistered table with checks disabled:");
    let result = query("SELECT * FROM nonexistent")
        .fetch_all(&disabled_client)
        .await;
    match result {
        Ok(_) => println!("   Query succeeded (table exists)"),
        Err(e) => println!("   Query failed at DB level: {}", e),
    }

    // ============================================
    // Example 5: Using registry directly
    // ============================================
    println!("\n5. Direct registry access:");

    let registry = checked.registry();

    // Check SQL manually
    let issues = registry.check_sql("SELECT * FROM products JOIN categories ON true");
    println!("   Check 'SELECT * FROM products JOIN categories': {} issues", issues.len());

    let issues = registry.check_sql("SELECT * FROM missing_table");
    println!("   Check 'SELECT * FROM missing_table': {} issues", issues.len());
    for issue in &issues {
        println!("     - {:?}: {}", issue.kind, issue.message);
    }

    println!("\n=== Done ===");

    Ok(())
}
