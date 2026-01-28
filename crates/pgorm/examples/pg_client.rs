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
    for table in pg.registry().tables() {
        println!(
            "   - Table '{}': columns = {:?}",
            table.name,
            table.columns.iter().map(|c| &c.name).collect::<Vec<_>>()
        );
    }

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
    println!(
        "   Stats: {} queries, {:?} total time",
        stats.total_queries, stats.total_duration
    );

    // ============================================
    // Example 2: Schema mismatch detection - Missing Table
    // ============================================
    println!("\n2. Schema mismatch detection - Missing Table:");

    let pg_strict = PgClient::with_config(&client, PgClientConfig::new().strict());

    println!("   Querying non-existent table 'orders':");
    let result = query("SELECT id, total FROM orders").fetch_all(&pg_strict).await;
    match result {
        Ok(_) => println!("   Query succeeded (unexpected!)"),
        Err(OrmError::Validation(msg)) => println!("   Validation error: {}", msg),
        Err(e) => println!("   DB error: {}", e),
    }

    // ============================================
    // Example 3: Schema mismatch detection - Missing Column
    // ============================================
    println!("\n3. Schema mismatch detection - Missing Column:");

    println!("   Querying non-existent column 'description' from products:");
    let result = query("SELECT id, name, description FROM products")
        .fetch_all(&pg_strict)
        .await;
    match result {
        Ok(_) => println!("   Query succeeded (unexpected!)"),
        Err(OrmError::Validation(msg)) => println!("   Validation error: {}", msg),
        Err(e) => println!("   DB error: {}", e),
    }

    println!("\n   Querying non-existent column with table qualifier:");
    let result = query("SELECT products.id, products.nonexistent_column FROM products")
        .fetch_all(&pg_strict)
        .await;
    match result {
        Ok(_) => println!("   Query succeeded (unexpected!)"),
        Err(OrmError::Validation(msg)) => println!("   Validation error: {}", msg),
        Err(e) => println!("   DB error: {}", e),
    }

    // ============================================
    // Example 4: Valid queries pass
    // ============================================
    println!("\n4. Valid queries pass:");

    println!("   Querying existing columns:");
    let result = query("SELECT id, name, price_cents FROM products")
        .fetch_all(&pg_strict)
        .await;
    match result {
        Ok(rows) => println!("   Query succeeded: {} rows", rows.len()),
        Err(e) => println!("   Error: {}", e),
    }

    println!("\n   Querying with table qualifier:");
    let result = query("SELECT products.id, products.name FROM products")
        .fetch_all(&pg_strict)
        .await;
    match result {
        Ok(rows) => println!("   Query succeeded: {} rows", rows.len()),
        Err(e) => println!("   Error: {}", e),
    }

    // ============================================
    // Example 5: WarnOnly mode (logs but doesn't block)
    // ============================================
    println!("\n5. WarnOnly mode (logs but doesn't block):");

    let pg_warn = PgClient::with_config(&client, PgClientConfig::new());
    println!("   Querying with invalid column (check mode = WarnOnly):");
    println!("   (Watch stderr for warning messages)");

    // This will print a warning but still try to execute
    let result = query("SELECT id, nonexistent FROM products")
        .fetch_all(&pg_warn)
        .await;
    match result {
        Ok(_) => println!("   Query was attempted (passed validation)"),
        Err(e) => println!("   DB error (as expected): {}", e),
    }

    // ============================================
    // Example 6: Direct registry access for checking
    // ============================================
    println!("\n6. Direct registry access:");

    let registry = pg.registry();

    let test_queries = [
        ("SELECT id, name FROM products", "valid"),
        ("SELECT id, email FROM products", "invalid column 'email'"),
        ("SELECT * FROM products JOIN categories ON true", "valid JOIN"),
        (
            "SELECT products.id, categories.id FROM products, categories",
            "valid multi-table",
        ),
    ];

    for (sql, desc) in test_queries {
        let issues = registry.check_sql(sql);
        println!("   Check '{}' ({}):", desc, sql);
        if issues.is_empty() {
            println!("     OK - no issues");
        } else {
            for issue in &issues {
                println!("     {:?}: {}", issue.kind, issue.message);
            }
        }
    }

    // ============================================
    // Example 7: Full configuration
    // ============================================
    println!("\n7. Full configuration:");

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

    println!("\n=== Done ===");

    Ok(())
}
