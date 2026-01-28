//! Basic usage example for pgorm
//!
//! Run with: cargo run --example basic -p pgorm
//!
//! Set DATABASE_URL in .env file or environment variable:
//! DATABASE_URL=postgres://postgres:postgres@localhost/pgorm_example

use pgorm::{create_pool, query, sql, FromRow, Model, OrmError};
use std::env;

#[derive(Debug, FromRow, Model)]
#[orm(table = "users")]
#[allow(dead_code)]
struct User {
    #[orm(id)]
    id: i64,
    username: String,
    email: Option<String>,
}

#[tokio::main]
async fn main() -> Result<(), OrmError> {
    // Load .env file
    dotenvy::dotenv().ok();

    // Read DATABASE_URL from environment
    let database_url = env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set in .env or environment");

    // Create connection pool
    let pool = create_pool(&database_url)?;
    let client = pool.get().await?;

    // Setup: Create table if not exists
    client
        .execute(
            "CREATE TABLE IF NOT EXISTS users (
                id BIGSERIAL PRIMARY KEY,
                username TEXT NOT NULL,
                email TEXT
            )",
            &[],
        )
        .await
        .map_err(OrmError::from_db_error)?;

    // Clean up existing data
    client
        .execute("DELETE FROM users", &[])
        .await
        .map_err(OrmError::from_db_error)?;

    // ============================================
    // Example 1: Insert with query()
    // ============================================
    println!("=== Insert with query() ===");

    let new_user: User = query("INSERT INTO users (username, email) VALUES ($1, $2) RETURNING *")
        .bind("alice")
        .bind(Some("alice@example.com"))
        .fetch_one_as(&client)
        .await?;

    println!("Inserted: {:?}", new_user);

    // Insert another user
    query("INSERT INTO users (username, email) VALUES ($1, $2)")
        .bind("bob")
        .bind(None::<String>)
        .execute(&client)
        .await?;

    // ============================================
    // Example 2: Select with query()
    // ============================================
    println!("\n=== Select with query() ===");

    // Fetch single user
    let user: User = query("SELECT id, username, email FROM users WHERE username = $1")
        .bind("alice")
        .fetch_one_as(&client)
        .await?;

    println!("Found user: {:?}", user);

    // Fetch all users
    let users: Vec<User> = query("SELECT id, username, email FROM users ORDER BY id")
        .fetch_all_as(&client)
        .await?;

    println!("All users: {:?}", users);

    // ============================================
    // Example 3: Dynamic SQL with sql()
    // ============================================
    println!("\n=== Dynamic SQL with sql() ===");

    // Build query dynamically - placeholders are auto-generated
    let search_term = Some("alice");
    let mut q = sql("SELECT id, username, email FROM users WHERE 1=1");

    if let Some(term) = search_term {
        q.push(" AND username = ").push_bind(term);
    }

    let found: Vec<User> = q.fetch_all_as(&client).await?;
    println!("Dynamic search result: {:?}", found);

    // ============================================
    // Example 4: Optional fetch
    // ============================================
    println!("\n=== Optional fetch ===");

    let maybe_user: Option<User> =
        query("SELECT id, username, email FROM users WHERE username = $1")
            .bind("nonexistent")
            .fetch_opt_as(&client)
            .await?;

    println!("Maybe user: {:?}", maybe_user);

    // ============================================
    // Example 5: Update
    // ============================================
    println!("\n=== Update ===");

    let updated: User = query("UPDATE users SET email = $1 WHERE username = $2 RETURNING *")
        .bind(Some("bob@example.com"))
        .bind("bob")
        .fetch_one_as(&client)
        .await?;

    println!("Updated user: {:?}", updated);

    // ============================================
    // Example 6: Using Model constants
    // ============================================
    println!("\n=== Using Model constants ===");

    println!("Table name: {}", User::TABLE);
    println!("ID column: {}", User::ID);
    println!("Select list: {}", User::SELECT_LIST);

    // Use in query
    let sql_str = format!(
        "SELECT {} FROM {} WHERE {} = $1",
        User::SELECT_LIST,
        User::TABLE,
        User::ID
    );
    let user: User = query(&sql_str).bind(new_user.id).fetch_one_as(&client).await?;

    println!("Fetched with Model constants: {:?}", user);

    // ============================================
    // Example 7: Delete
    // ============================================
    println!("\n=== Delete ===");

    let rows_affected = query("DELETE FROM users WHERE username = $1")
        .bind("bob")
        .execute(&client)
        .await?;

    println!("Deleted {} row(s)", rows_affected);

    // Final count
    let count: i64 = query("SELECT COUNT(*) FROM users")
        .fetch_one(&client)
        .await?
        .get(0);

    println!("\nFinal user count: {}", count);

    Ok(())
}
