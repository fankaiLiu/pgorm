//! Example demonstrating PgClient - the unified client with monitoring and SQL checking
//!
//! Run with: cargo run --example pg_client -p pgorm --features "derive,pool,check"
//!
//! Set DATABASE_URL in .env file or environment variable:
//! DATABASE_URL=postgres://postgres:postgres@localhost/pgorm_example

use pgorm::{create_pool, query, CheckMode, FromRow, Model, OrmError, PgClient, PgClientConfig};
use std::env;
use std::time::Duration;

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

    println!("=== PgClient Demo ===\n");

    // ============================================
    // Example 1: Basic usage with defaults
    // ============================================
    println!("1. Basic usage (default config):");

    // PgClient with defaults:
    // - Auto-registers all #[derive(Model)] types
    // - Statistics collection enabled
    // - SQL check in warn-only mode
    let pg = PgClient::new(&client);

    println!(
        "   Registered {} tables automatically",
        pg.registry().len()
    );

    // Insert some data
    query("INSERT INTO products (name, price_cents, in_stock) VALUES ($1, $2, $3)")
        .bind("Laptop")
        .bind(99999_i64)
        .bind(true)
        .execute(&pg)
        .await?;

    query("INSERT INTO products (name, price_cents, in_stock) VALUES ($1, $2, $3)")
        .bind("Mouse")
        .bind(2999_i64)
        .bind(true)
        .execute(&pg)
        .await?;

    // Query data
    let products = Product::select_all(&pg).await?;
    println!("   Found {} products", products.len());

    // Get statistics
    let stats = pg.stats();
    println!("   Stats: {} queries, {:?} total time", stats.total_queries, stats.total_duration);

    // ============================================
    // Example 2: With logging enabled
    // ============================================
    println!("\n2. With logging enabled:");

    let pg_logged = PgClient::with_config(
        &client,
        PgClientConfig::new().with_logging(),
    );

    println!("   Running query (will be logged to stderr):");
    let _ = Product::select_all(&pg_logged).await?;

    // ============================================
    // Example 3: Strict mode
    // ============================================
    println!("\n3. Strict mode:");

    let pg_strict = PgClient::with_config(&client, PgClientConfig::new().strict());

    // Valid query works
    let products = Product::select_all(&pg_strict).await?;
    println!("   Valid query succeeded: {} products", products.len());

    // Invalid query returns error
    println!("   Querying unregistered table:");
    let result = query("SELECT * FROM nonexistent").fetch_all(&pg_strict).await;
    match result {
        Ok(_) => println!("   Query succeeded (unexpected!)"),
        Err(OrmError::Validation(msg)) => println!("   Validation error: {}", msg),
        Err(e) => println!("   Other error: {}", e),
    }

    // ============================================
    // Example 4: With timeout
    // ============================================
    println!("\n4. With timeout:");

    let pg_timeout = PgClient::with_config(
        &client,
        PgClientConfig::new().timeout(Duration::from_secs(5)),
    );

    let products = Product::select_all(&pg_timeout).await?;
    println!("   Query with timeout succeeded: {} products", products.len());

    // ============================================
    // Example 5: Full configuration
    // ============================================
    println!("\n5. Full configuration:");

    let pg_full = PgClient::with_config(
        &client,
        PgClientConfig::new()
            .check_mode(CheckMode::WarnOnly)
            .timeout(Duration::from_secs(30))
            .slow_threshold(Duration::from_millis(100))
            .with_stats()
            .log_slow_queries(Duration::from_millis(50)),
    );

    // Run several queries
    for _ in 0..5 {
        let _ = Product::select_all(&pg_full).await?;
    }

    let stats = pg_full.stats();
    println!("   Stats after 5 queries:");
    println!("     Total queries: {}", stats.total_queries);
    println!("     Total duration: {:?}", stats.total_duration);
    println!("     SELECT count: {}", stats.select_count);
    println!("     Max duration: {:?}", stats.max_duration);

    // ============================================
    // Example 6: Comparing with raw client
    // ============================================
    println!("\n6. Comparison - PgClient vs raw client:");
    println!("   PgClient provides:");
    println!("   - Auto model registration");
    println!("   - SQL validation");
    println!("   - Query statistics");
    println!("   - Timeout handling");
    println!("   - Slow query logging");
    println!("   All in one unified interface!");

    println!("\n=== Done ===");

    Ok(())
}
