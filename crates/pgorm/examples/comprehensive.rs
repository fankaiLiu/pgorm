//! Comprehensive pgorm example showcasing all major features
//!
//! This example demonstrates:
//! - Connection pooling
//! - Model derive macros (FromRow, Model, InsertModel, UpdateModel)
//! - Query builders (SELECT, INSERT, UPDATE, DELETE)
//! - Dynamic SQL construction
//! - Transactions
//! - Query monitoring and hooks
//! - SQL checking and linting (with --features check)
//!
//! Run with:
//!   cargo run --example comprehensive -p pgorm --features "derive,pool,builder,check"
//!
//! Set DATABASE_URL in .env file or environment variable:
//!   DATABASE_URL=postgres://postgres:postgres@localhost/pgorm_example

use pgorm::{
    create_pool, query, sql, FromRow, GenericClient, InsertModel, Model, OrmError, UpdateModel,
};
use std::env;

// ============================================
// 1. Model Definitions
// ============================================

/// User model with derive macros
#[derive(Debug, Clone, FromRow, Model)]
#[orm(table = "users")]
#[allow(dead_code)]
struct User {
    #[orm(id)]
    id: i64,
    username: String,
    email: Option<String>,
    active: bool,
}

/// Post model with foreign key to User
#[derive(Debug, Clone, FromRow, Model)]
#[orm(table = "posts")]
#[allow(dead_code)]
struct Post {
    #[orm(id)]
    id: i64,
    user_id: i64,
    title: String,
    content: String,
    published: bool,
}

/// Insert model for creating new users
#[derive(Debug, InsertModel)]
#[orm(table = "users", returning = "User")]
struct NewUser {
    username: String,
    email: Option<String>,
    active: bool,
}

/// Update model for patching users (Option fields are skipped if None)
#[derive(Debug, UpdateModel)]
#[orm(table = "users", returning = "User")]
struct UserPatch {
    username: Option<String>,
    email: Option<Option<String>>, // None = skip, Some(None) = set NULL, Some(Some(v)) = set v
    active: Option<bool>,
}

/// Insert model for posts
#[derive(Debug, InsertModel)]
#[orm(table = "posts", returning = "Post")]
struct NewPost {
    user_id: i64,
    title: String,
    content: String,
    published: bool,
}

#[tokio::main]
async fn main() -> Result<(), OrmError> {
    dotenvy::dotenv().ok();

    let database_url =
        env::var("DATABASE_URL").expect("DATABASE_URL must be set in .env or environment");

    println!("=== pgorm Comprehensive Example ===\n");

    // ============================================
    // 2. Connection Pool
    // ============================================
    println!("1. Creating connection pool...");
    let pool = create_pool(&database_url)?;
    let client = pool.get().await?;
    println!("   Connected to database\n");

    // Setup tables
    setup_tables(&client).await?;

    // ============================================
    // 3. Insert with InsertModel derive
    // ============================================
    println!("2. Insert with InsertModel derive");

    let alice = NewUser {
        username: "alice".to_string(),
        email: Some("alice@example.com".to_string()),
        active: true,
    }
    .insert_returning(&client)
    .await?;
    println!("   Created: {:?}", alice);

    let bob = NewUser {
        username: "bob".to_string(),
        email: None,
        active: true,
    }
    .insert_returning(&client)
    .await?;
    println!("   Created: {:?}", bob);
    println!();

    // ============================================
    // 4. Query with query() builder
    // ============================================
    println!("3. Query with query() builder");

    // Fetch single user
    let user: User = query("SELECT * FROM users WHERE username = $1")
        .bind("alice")
        .fetch_one_as(&client)
        .await?;
    println!("   Found: {:?}", user);

    // Fetch all users
    let users: Vec<User> = query("SELECT * FROM users ORDER BY id")
        .fetch_all_as(&client)
        .await?;
    println!("   All users: {} total", users.len());
    println!();

    // ============================================
    // 5. Dynamic SQL with sql()
    // ============================================
    println!("4. Dynamic SQL with sql()");

    let search_email = Some("alice@example.com");
    let only_active = true;

    let mut q = sql("SELECT * FROM users WHERE 1=1");

    if let Some(email) = search_email {
        q.push(" AND email = ").push_bind(email);
    }
    if only_active {
        q.push(" AND active = ").push_bind(true);
    }
    q.push(" ORDER BY id");

    let found: Vec<User> = q.fetch_all_as(&client).await?;
    println!("   Dynamic query found: {} users", found.len());
    println!();

    // ============================================
    // 6. Update with UpdateModel derive
    // ============================================
    println!("5. Update with UpdateModel derive");

    let updated = UserPatch {
        username: Some("alice_updated".to_string()),
        email: None, // skip - keep existing
        active: None, // skip - keep existing
    }
    .update_by_id_returning(&client, alice.id)
    .await?;
    println!("   Updated: {:?}", updated);
    println!();

    // ============================================
    // 7. Model static methods
    // ============================================
    println!("6. Model static methods");
    println!("   User::TABLE = {}", User::TABLE);
    println!("   User::ID = {}", User::ID);
    println!("   User::SELECT_LIST = {}", User::SELECT_LIST);

    // select_one by ID
    let user = User::select_one(&client, alice.id).await?;
    println!("   select_one({}): {:?}", alice.id, user);

    // select_all
    let all = User::select_all(&client).await?;
    println!("   select_all(): {} users", all.len());
    println!();

    // ============================================
    // 8. Query struct (dynamic queries)
    // ============================================
    println!("7. Query struct (UserQuery)");

    // Using generated UserQuery
    let active_users = User::query()
        .eq(UserQuery::active, true)
        .order_by("username ASC")
        .find(&client)
        .await?;
    println!("   Active users: {}", active_users.len());

    let count = User::query().eq("active", true).count(&client).await?;
    println!("   Count of active users: {}", count);
    println!();

    // ============================================
    // 9. Create posts and demonstrate relations
    // ============================================
    println!("8. Creating posts");

    let post1 = NewPost {
        user_id: alice.id,
        title: "Hello World".to_string(),
        content: "My first post!".to_string(),
        published: true,
    }
    .insert_returning(&client)
    .await?;
    println!("   Created post: {:?}", post1);

    let _post2 = NewPost {
        user_id: alice.id,
        title: "Second Post".to_string(),
        content: "Another post".to_string(),
        published: false,
    }
    .insert_returning(&client)
    .await?;

    // Query posts by user
    let user_posts = Post::query()
        .eq("user_id", alice.id)
        .order_by("id DESC")
        .find(&client)
        .await?;
    println!("   Alice's posts: {}", user_posts.len());
    println!();

    // ============================================
    // 10. SQL Checking (requires --features check)
    // ============================================
    #[cfg(feature = "check")]
    {
        use pgorm::{lint_sql, SchemaRegistry};

        println!("9. SQL Checking & Linting");

        // Register tables
        let mut registry = SchemaRegistry::new();
        registry.register::<User>();
        registry.register::<Post>();

        // Check SQL against schema
        let issues = registry.check_sql("SELECT * FROM users JOIN posts ON users.id = posts.user_id");
        println!("   Schema check (valid JOIN): {} issues", issues.len());

        let issues = registry.check_sql("SELECT * FROM nonexistent_table");
        println!("   Schema check (missing table): {} issues", issues.len());
        for issue in &issues {
            println!("     - {:?}: {}", issue.kind, issue.message);
        }

        // Lint SQL
        let result = lint_sql("DELETE FROM users");
        println!("   Lint 'DELETE FROM users': {} issues", result.issues.len());
        for issue in &result.issues {
            println!("     - {} [{}]: {}",
                match issue.level {
                    pgorm::LintLevel::Error => "ERROR",
                    pgorm::LintLevel::Warning => "WARN",
                    pgorm::LintLevel::Info => "INFO",
                },
                issue.code,
                issue.message
            );
        }

        let result = lint_sql("DELETE FROM users WHERE id = 1");
        println!("   Lint 'DELETE FROM users WHERE id = 1': {} issues", result.issues.len());
        println!();
    }

    // ============================================
    // 11. Delete
    // ============================================
    println!("10. Cleanup - Delete operations");

    // Delete posts first (foreign key)
    let deleted = Post::delete_by_ids(&client, vec![post1.id]).await?;
    println!("   Deleted {} posts", deleted);

    // Delete by ID
    let deleted = User::delete_by_id(&client, bob.id).await?;
    println!("   Deleted {} user (bob)", deleted);

    // Delete with returning
    let deleted_user = User::delete_by_id_returning(&client, alice.id).await?;
    println!("   Deleted user: {:?}", deleted_user);
    println!();

    // Final count
    let final_count: i64 = query("SELECT COUNT(*) FROM users")
        .fetch_one(&client)
        .await?
        .get(0);
    println!("Final user count: {}", final_count);

    println!("\n=== Example completed ===");
    Ok(())
}

async fn setup_tables(client: &impl GenericClient) -> Result<(), OrmError> {
    // Drop tables if exist
    client.execute("DROP TABLE IF EXISTS posts", &[]).await?;
    client.execute("DROP TABLE IF EXISTS users", &[]).await?;

    // Create users table
    client
        .execute(
            "CREATE TABLE users (
                id BIGSERIAL PRIMARY KEY,
                username TEXT NOT NULL UNIQUE,
                email TEXT,
                active BOOLEAN NOT NULL DEFAULT true
            )",
            &[],
        )
        .await?;

    // Create posts table
    client
        .execute(
            "CREATE TABLE posts (
                id BIGSERIAL PRIMARY KEY,
                user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                title TEXT NOT NULL,
                content TEXT NOT NULL,
                published BOOLEAN NOT NULL DEFAULT false
            )",
            &[],
        )
        .await?;

    Ok(())
}
