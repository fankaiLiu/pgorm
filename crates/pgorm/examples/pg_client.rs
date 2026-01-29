//! Example demonstrating PgClient - the unified client with monitoring and SQL checking
//!
//! Run with: cargo run --example pg_client -p pgorm
//!
//! Set DATABASE_URL in .env file or environment variable:
//! DATABASE_URL=postgres://postgres:postgres@localhost/pgorm_example

use colored::Colorize;
use comfy_table::{Attribute, Cell, Color, ContentArrangement, Table, presets::UTF8_FULL};
use pgorm::{CheckMode, FromRow, Model, OrmError, PgClient, PgClientConfig, create_pool, query};
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

/// Print a section header with styling
fn print_header(title: &str) {
    println!();
    println!("{}", "─".repeat(60).bright_black());
    println!("{}", title.bold().cyan());
    println!("{}", "─".repeat(60).bright_black());
}

/// Print success message
fn print_success(msg: &str) {
    println!("  {} {}", "✓".green().bold(), msg);
}

/// Print warning message
fn print_warning(msg: &str) {
    println!("  {} {}", "⚠".yellow().bold(), msg);
}

/// Print info message
fn print_info(msg: &str) {
    println!("  {} {}", "ℹ".blue(), msg);
}

/// Create a styled table for schema registry
fn create_registry_table(pg: &PgClient<impl pgorm::GenericClient>) -> Table {
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec![
            Cell::new("Table")
                .add_attribute(Attribute::Bold)
                .fg(Color::Cyan),
            Cell::new("Schema")
                .add_attribute(Attribute::Bold)
                .fg(Color::Cyan),
            Cell::new("Columns")
                .add_attribute(Attribute::Bold)
                .fg(Color::Cyan),
        ]);

    for t in pg.registry().tables() {
        let columns: Vec<String> = t
            .columns
            .iter()
            .map(|c| {
                if c.is_primary_key {
                    format!("{}*", c.name)
                } else {
                    c.name.clone()
                }
            })
            .collect();

        table.add_row(vec![
            Cell::new(&t.name).fg(Color::Green),
            Cell::new(&t.schema).fg(Color::DarkGrey),
            Cell::new(columns.join(", ")),
        ]);
    }

    table
}

/// Create a styled table for SQL check results
fn create_check_results_table(results: &[(String, String, Vec<pgorm::SchemaIssue>)]) -> Table {
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec![
            Cell::new("Status")
                .add_attribute(Attribute::Bold)
                .fg(Color::Cyan),
            Cell::new("Test")
                .add_attribute(Attribute::Bold)
                .fg(Color::Cyan),
            Cell::new("SQL")
                .add_attribute(Attribute::Bold)
                .fg(Color::Cyan),
            Cell::new("Issues")
                .add_attribute(Attribute::Bold)
                .fg(Color::Cyan),
        ]);

    for (desc, sql, issues) in results {
        let (status, status_color) = if issues.is_empty() {
            ("✓ PASS", Color::Green)
        } else {
            ("✗ FAIL", Color::Red)
        };

        let issue_text = if issues.is_empty() {
            "None".to_string()
        } else {
            issues
                .iter()
                .map(|i| format!("{:?}: {}", i.kind, i.message))
                .collect::<Vec<_>>()
                .join("\n")
        };

        table.add_row(vec![
            Cell::new(status)
                .fg(status_color)
                .add_attribute(Attribute::Bold),
            Cell::new(desc),
            Cell::new(sql).fg(Color::DarkGrey),
            Cell::new(&issue_text),
        ]);
    }

    table
}

/// Create a styled table for query statistics
fn create_stats_table(stats: &pgorm::QueryStats) -> Table {
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec![
            Cell::new("Metric")
                .add_attribute(Attribute::Bold)
                .fg(Color::Cyan),
            Cell::new("Value")
                .add_attribute(Attribute::Bold)
                .fg(Color::Cyan),
        ]);

    table.add_row(vec![
        Cell::new("Total Queries"),
        Cell::new(stats.total_queries.to_string()).fg(Color::Yellow),
    ]);
    table.add_row(vec![
        Cell::new("Total Duration"),
        Cell::new(format!("{:?}", stats.total_duration)).fg(Color::Yellow),
    ]);
    table.add_row(vec![
        Cell::new("SELECT Count"),
        Cell::new(stats.select_count.to_string()).fg(Color::Green),
    ]);
    table.add_row(vec![
        Cell::new("INSERT Count"),
        Cell::new(stats.insert_count.to_string()).fg(Color::Blue),
    ]);
    table.add_row(vec![
        Cell::new("UPDATE Count"),
        Cell::new(stats.update_count.to_string()).fg(Color::Magenta),
    ]);
    table.add_row(vec![
        Cell::new("DELETE Count"),
        Cell::new(stats.delete_count.to_string()).fg(Color::Red),
    ]);
    table.add_row(vec![
        Cell::new("Max Duration"),
        Cell::new(format!("{:?}", stats.max_duration)).fg(Color::Yellow),
    ]);
    if stats.total_queries > 0 {
        let avg = stats.total_duration / stats.total_queries as u32;
        table.add_row(vec![
            Cell::new("Avg Duration"),
            Cell::new(format!("{:?}", avg)).fg(Color::Yellow),
        ]);
    }

    table
}

/// Create a styled table for validation results
fn create_validation_table(
    results: &[(&str, Result<Vec<tokio_postgres::Row>, OrmError>)],
) -> Table {
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec![
            Cell::new("Status")
                .add_attribute(Attribute::Bold)
                .fg(Color::Cyan),
            Cell::new("Description")
                .add_attribute(Attribute::Bold)
                .fg(Color::Cyan),
            Cell::new("Result")
                .add_attribute(Attribute::Bold)
                .fg(Color::Cyan),
        ]);

    for (desc, result) in results {
        let (status, status_color, result_text) = match result {
            Ok(rows) => ("✓", Color::Green, format!("Success: {} rows", rows.len())),
            Err(OrmError::Validation(msg)) => ("✗", Color::Red, format!("Validation: {}", msg)),
            Err(e) => ("⚠", Color::Yellow, format!("DB Error: {}", e)),
        };

        table.add_row(vec![
            Cell::new(status)
                .fg(status_color)
                .add_attribute(Attribute::Bold),
            Cell::new(*desc),
            Cell::new(&result_text),
        ]);
    }

    table
}

#[tokio::main]
async fn main() -> Result<(), OrmError> {
    dotenvy::dotenv().ok();

    println!();
    println!(
        "{}",
        "╔══════════════════════════════════════════════════════════╗".cyan()
    );
    println!(
        "{}",
        "║           PgClient Demo - Schema Checking                ║"
            .cyan()
            .bold()
    );
    println!(
        "{}",
        "╚══════════════════════════════════════════════════════════╝".cyan()
    );

    let database_url =
        env::var("DATABASE_URL").expect("DATABASE_URL must be set in .env or environment");

    let pool = create_pool(&database_url)?;
    let client = pool.get().await?;

    // Setup: Create tables
    print_header("Setup: Creating Tables");

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

    print_success("Tables created: products, categories");

    // ============================================
    // Example 1: Basic usage with defaults
    // ============================================
    print_header("1. Schema Registry (Auto-Registered Models)");

    let pg = PgClient::new(&client);

    print_info(&format!(
        "Registered {} tables automatically via #[derive(Model)]",
        pg.registry().len()
    ));

    println!();
    println!("{}", create_registry_table(&pg));

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

    let products = Product::select_all(&pg).await?;
    print_success(&format!("Inserted and queried {} products", products.len()));

    // ============================================
    // Example 2: Direct SQL checking
    // ============================================
    print_header("2. SQL Schema Validation");

    let test_queries = vec![
        (
            "Valid: existing columns".to_string(),
            "SELECT id, name FROM products".to_string(),
        ),
        (
            "Invalid: missing column 'email'".to_string(),
            "SELECT id, email FROM products".to_string(),
        ),
        (
            "Valid: JOIN query".to_string(),
            "SELECT * FROM products JOIN categories ON true".to_string(),
        ),
        (
            "Valid: multi-table".to_string(),
            "SELECT products.id, categories.id FROM products, categories".to_string(),
        ),
        (
            "Invalid: table 'orders'".to_string(),
            "SELECT id FROM orders".to_string(),
        ),
        (
            "Invalid: qualified column".to_string(),
            "SELECT products.nonexistent FROM products".to_string(),
        ),
    ];

    let results: Vec<_> = test_queries
        .iter()
        .map(|(desc, sql)| {
            let issues = pg.registry().check_sql(sql);
            (desc.clone(), sql.clone(), issues)
        })
        .collect();

    println!();
    println!("{}", create_check_results_table(&results));

    // ============================================
    // Example 3: Strict mode validation
    // ============================================
    print_header("3. Strict Mode (Blocks Invalid Queries)");

    let pg_strict = PgClient::with_config(&client, PgClientConfig::new().strict());

    let validation_tests: Vec<(&str, Result<Vec<tokio_postgres::Row>, OrmError>)> = vec![
        (
            "Valid query: SELECT id, name FROM products",
            query("SELECT id, name FROM products")
                .fetch_all(&pg_strict)
                .await,
        ),
        (
            "Invalid: non-existent table 'orders'",
            query("SELECT id FROM orders").fetch_all(&pg_strict).await,
        ),
        (
            "Invalid: non-existent column 'description'",
            query("SELECT id, description FROM products")
                .fetch_all(&pg_strict)
                .await,
        ),
        (
            "Invalid: qualified non-existent column",
            query("SELECT products.nonexistent FROM products")
                .fetch_all(&pg_strict)
                .await,
        ),
    ];

    println!();
    println!("{}", create_validation_table(&validation_tests));

    // ============================================
    // Example 4: WarnOnly mode
    // ============================================
    print_header("4. WarnOnly Mode (Logs But Doesn't Block)");

    let pg_warn = PgClient::with_config(&client, PgClientConfig::new());

    print_info("Running query with invalid column (will warn but attempt execution)...");
    println!();

    let result = query("SELECT id, nonexistent FROM products")
        .fetch_all(&pg_warn)
        .await;

    match result {
        Ok(_) => print_success("Query attempted (passed validation phase)"),
        Err(e) => print_warning(&format!("DB error (expected): {}", e)),
    }

    // ============================================
    // Example 5: Query Statistics
    // ============================================
    print_header("5. Query Statistics");

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

    // Insert
    query("INSERT INTO categories (name) VALUES ($1)")
        .bind("Electronics")
        .execute(&pg_full)
        .await?;

    // Update
    query("UPDATE products SET in_stock = $1 WHERE id = $2")
        .bind(false)
        .bind(1_i64)
        .execute(&pg_full)
        .await?;

    println!();
    println!("{}", create_stats_table(&pg_full.stats()));

    // ============================================
    // Summary
    // ============================================
    print_header("Summary");

    let mut summary_table = Table::new();
    summary_table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec![
            Cell::new("Feature")
                .add_attribute(Attribute::Bold)
                .fg(Color::Cyan),
            Cell::new("Description")
                .add_attribute(Attribute::Bold)
                .fg(Color::Cyan),
        ]);

    summary_table.add_row(vec![
        Cell::new("Auto-Registration").fg(Color::Green),
        Cell::new("Models with #[derive(Model)] are automatically registered"),
    ]);
    summary_table.add_row(vec![
        Cell::new("Schema Checking").fg(Color::Green),
        Cell::new("Validates SQL against registered table schemas"),
    ]);
    summary_table.add_row(vec![
        Cell::new("Check Modes").fg(Color::Green),
        Cell::new("Disabled, WarnOnly, or Strict"),
    ]);
    summary_table.add_row(vec![
        Cell::new("Query Statistics").fg(Color::Green),
        Cell::new("Tracks query counts, durations, and types"),
    ]);
    summary_table.add_row(vec![
        Cell::new("Slow Query Logging").fg(Color::Green),
        Cell::new("Configurable threshold for alerting"),
    ]);

    println!();
    println!("{}", summary_table);

    println!();
    println!("{}", "═".repeat(60).cyan());
    println!("{}", "  Demo completed successfully!".green().bold());
    println!("{}", "═".repeat(60).cyan());
    println!();

    Ok(())
}
