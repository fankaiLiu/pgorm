//! Example demonstrating Model schema validation against the database
//!
//! Run with: cargo run --example model_check -p pgorm --features "derive,pool,check"
//!
//! Set DATABASE_URL in .env file or environment variable.

use pgorm::{
    assert_models_db_valid, check_models_db, create_pool, print_models_db_check, FromRow, Model,
    OrmError, PgClient,
};
use std::env;

// ============================================
// Define models
// ============================================

#[derive(Debug, FromRow, Model)]
#[orm(table = "users")]
#[allow(dead_code)]
struct User {
    #[orm(id)]
    id: i64,
    name: String,
    email: String,
}

#[derive(Debug, FromRow, Model)]
#[orm(table = "orders")]
#[allow(dead_code)]
struct Order {
    #[orm(id)]
    id: i64,
    user_id: i64,
    total: i64,
}

// Model with wrong columns (for testing)
#[derive(Debug, FromRow, Model)]
#[orm(table = "users")]
#[allow(dead_code)]
struct UserWithWrongColumns {
    #[orm(id)]
    id: i64,
    name: String,
    phone: String,   // doesn't exist in DB
    address: String, // doesn't exist in DB
}

// Model pointing to non-existent table
#[derive(Debug, FromRow, Model)]
#[orm(table = "nonexistent_table")]
#[allow(dead_code)]
struct NonExistentModel {
    #[orm(id)]
    id: i64,
    data: String,
}

#[tokio::main]
async fn main() -> Result<(), OrmError> {
    dotenvy::dotenv().ok();

    let database_url =
        env::var("DATABASE_URL").expect("DATABASE_URL must be set in .env or environment");

    let pool = create_pool(&database_url)?;
    let client = pool.get().await?;

    // Create test tables
    client
        .execute("DROP TABLE IF EXISTS orders CASCADE", &[])
        .await
        .map_err(OrmError::from_db_error)?;
    client
        .execute("DROP TABLE IF EXISTS users CASCADE", &[])
        .await
        .map_err(OrmError::from_db_error)?;

    client
        .execute(
            "CREATE TABLE users (
                id BIGSERIAL PRIMARY KEY,
                name TEXT NOT NULL,
                email TEXT NOT NULL
            )",
            &[],
        )
        .await
        .map_err(OrmError::from_db_error)?;

    client
        .execute(
            "CREATE TABLE orders (
                id BIGSERIAL PRIMARY KEY,
                user_id BIGINT NOT NULL,
                total BIGINT NOT NULL
            )",
            &[],
        )
        .await
        .map_err(OrmError::from_db_error)?;

    println!("=== Model Database Schema Check ===\n");

    // Create PgClient
    let pg = PgClient::new(&client);

    // ============================================
    // Method 1: print_models_db_check! - prints results
    // ============================================
    println!("--- Using print_models_db_check! ---\n");

    let all_valid = print_models_db_check!(pg, User, Order).await?;
    println!("\nAll valid: {}\n", all_valid);

    // Check with invalid models
    println!("--- Including invalid models ---\n");
    let all_valid = print_models_db_check!(pg, User, Order, UserWithWrongColumns, NonExistentModel).await?;
    println!("\nAll valid: {}\n", all_valid);

    // ============================================
    // Method 2: check_models_db! - returns results
    // ============================================
    println!("--- Using check_models_db! ---\n");

    let results = check_models_db!(pg, User, Order, UserWithWrongColumns).await?;
    for result in &results {
        if result.is_valid() {
            println!("  {} - OK", result.model);
        } else if !result.table_found {
            println!("  {} - table not found", result.model);
        } else {
            println!("  {} - missing: {:?}", result.model, result.missing_in_db);
        }
    }

    // ============================================
    // Method 3: Check single model
    // ============================================
    println!("\n--- Single model check ---\n");

    let result = pg.check_model::<User>().await?;
    println!("User model check:");
    println!("  Table found: {}", result.table_found);
    println!("  Model columns: {:?}", result.model_columns);
    println!("  DB columns: {:?}", result.db_columns);
    println!("  Missing in DB: {:?}", result.missing_in_db);
    println!("  Extra in DB: {:?}", result.extra_in_db);

    // ============================================
    // Method 4: assert_models_db_valid!
    // ============================================
    println!("\n--- Using assert_models_db_valid! ---\n");

    println!("Checking valid models...");
    assert_models_db_valid!(pg, User, Order).await?;
    println!("✓ Valid models passed!\n");

    // Uncomment to see panic:
    // assert_models_db_valid!(pg, User, UserWithWrongColumns).await?;

    println!("=== Done ===");

    Ok(())
}
