//! Query hooks example for pgorm
//!
//! Run with: cargo run --example hooks -p pgorm
//!
//! Set DATABASE_URL in .env file or environment variable:
//! DATABASE_URL=postgres://postgres:postgres@localhost/pgorm_example
//!
//! This example demonstrates:
//! - QueryHook for intercepting queries before/after execution
//! - Modifying SQL on the fly (adding comments, hints)
//! - Blocking certain query types
//! - Audit logging
//! - Query validation
//! - CompositeHook for chaining hooks

use pgorm::{
    create_pool, query, CompositeHook, FromRow, HookAction, InstrumentedClient, LoggingMonitor,
    Model, OrmError, QueryContext, QueryHook, QueryResult, QueryType,
};
use std::env;
use std::sync::{Arc, RwLock};
use std::time::Duration;

#[derive(Debug, FromRow, Model)]
#[orm(table = "users")]
#[allow(dead_code)]
struct User {
    #[orm(id)]
    id: i64,
    username: String,
    email: Option<String>,
}

/// Hook that adds a SQL comment to identify the application
struct AppCommentHook {
    app_name: String,
}

impl AppCommentHook {
    fn new(app_name: impl Into<String>) -> Self {
        Self {
            app_name: app_name.into(),
        }
    }
}

impl QueryHook for AppCommentHook {
    fn before_query(&self, ctx: &QueryContext) -> HookAction {
        // Add application identifier as SQL comment
        let modified = format!("/* app={} */ {}", self.app_name, ctx.sql);
        HookAction::ModifySql(modified)
    }
}

/// Hook that blocks DELETE without WHERE clause
struct SafeDeleteHook;

impl QueryHook for SafeDeleteHook {
    fn before_query(&self, ctx: &QueryContext) -> HookAction {
        if ctx.query_type == QueryType::Delete {
            let upper = ctx.sql.to_uppercase();
            // Check for DELETE without WHERE (dangerous!)
            if !upper.contains("WHERE") {
                return HookAction::Abort(
                    "DELETE without WHERE clause is not allowed. Use DELETE FROM table WHERE 1=1 to delete all.".to_string()
                );
            }
        }
        HookAction::Continue
    }
}

/// Hook that blocks all DELETE operations (read-only mode)
struct ReadOnlyHook;

impl QueryHook for ReadOnlyHook {
    fn before_query(&self, ctx: &QueryContext) -> HookAction {
        match ctx.query_type {
            QueryType::Insert | QueryType::Update | QueryType::Delete => {
                HookAction::Abort(format!(
                    "Database is in read-only mode. {:?} operations are not allowed.",
                    ctx.query_type
                ))
            }
            _ => HookAction::Continue,
        }
    }
}

/// Hook that logs all mutations for audit purposes
struct AuditLogHook {
    logs: Arc<RwLock<Vec<AuditEntry>>>,
}

#[derive(Debug, Clone)]
struct AuditEntry {
    query_type: QueryType,
    sql: String,
    duration: Duration,
    success: bool,
}

impl AuditLogHook {
    fn new() -> Self {
        Self {
            logs: Arc::new(RwLock::new(Vec::new())),
        }
    }

    fn entries(&self) -> Vec<AuditEntry> {
        self.logs.read().unwrap().clone()
    }
}

impl QueryHook for AuditLogHook {
    fn before_query(&self, _ctx: &QueryContext) -> HookAction {
        HookAction::Continue
    }

    fn after_query(&self, ctx: &QueryContext, duration: Duration, result: &QueryResult) {
        // Only log mutations
        if matches!(
            ctx.query_type,
            QueryType::Insert | QueryType::Update | QueryType::Delete
        ) {
            let entry = AuditEntry {
                query_type: ctx.query_type,
                sql: ctx.sql.clone(),
                duration,
                success: !matches!(result, QueryResult::Error(_)),
            };
            self.logs.write().unwrap().push(entry);
        }
    }
}

/// Hook that adds PostgreSQL query hints (example, not used in main)
#[allow(dead_code)]
struct QueryHintHook {
    enable_seqscan: bool,
    work_mem: Option<String>,
}

#[allow(dead_code)]
impl QueryHintHook {
    fn new() -> Self {
        Self {
            enable_seqscan: true,
            work_mem: None,
        }
    }

    fn disable_seqscan(mut self) -> Self {
        self.enable_seqscan = false;
        self
    }

    fn work_mem(mut self, mem: impl Into<String>) -> Self {
        self.work_mem = Some(mem.into());
        self
    }
}

impl QueryHook for QueryHintHook {
    fn before_query(&self, ctx: &QueryContext) -> HookAction {
        // Only apply hints to SELECT queries
        if ctx.query_type != QueryType::Select {
            return HookAction::Continue;
        }

        let mut hints = Vec::new();

        if !self.enable_seqscan {
            hints.push("SET LOCAL enable_seqscan = off;".to_string());
        }

        if let Some(ref mem) = self.work_mem {
            hints.push(format!("SET LOCAL work_mem = '{}';", mem));
        }

        if hints.is_empty() {
            return HookAction::Continue;
        }

        // Wrap in transaction-local settings
        let modified = format!("{} {}", hints.join(" "), ctx.sql);
        HookAction::ModifySql(modified)
    }
}

/// Hook that validates query parameters
struct ParameterValidationHook {
    max_params: usize,
}

impl ParameterValidationHook {
    fn new(max_params: usize) -> Self {
        Self { max_params }
    }
}

impl QueryHook for ParameterValidationHook {
    fn before_query(&self, ctx: &QueryContext) -> HookAction {
        if ctx.param_count > self.max_params {
            return HookAction::Abort(format!(
                "Too many parameters: {} (max: {})",
                ctx.param_count, self.max_params
            ));
        }
        HookAction::Continue
    }
}

#[tokio::main]
async fn main() -> Result<(), OrmError> {
    dotenvy::dotenv().ok();

    let database_url =
        env::var("DATABASE_URL").expect("DATABASE_URL must be set in .env or environment");

    let pool = create_pool(&database_url)?;
    let raw_client = pool.get().await?;

    // Setup table
    raw_client
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

    raw_client
        .execute("DELETE FROM users", &[])
        .await
        .map_err(OrmError::from_db_error)?;

    // ============================================
    // Example 1: Add application comment to SQL
    // ============================================
    println!("=== Example 1: AppCommentHook ===\n");

    let client = InstrumentedClient::new(&*raw_client)
        .with_hook(AppCommentHook::new("my-service"))
        .with_monitor(LoggingMonitor::new().prefix("[SQL]").max_sql_length(100));

    // The SQL will be prefixed with /* app=my-service */
    query("INSERT INTO users (username, email) VALUES ($1, $2)")
        .bind("alice")
        .bind(Some("alice@example.com"))
        .execute(&client)
        .await?;

    let _: Vec<User> = query("SELECT * FROM users").fetch_all_as(&client).await?;

    println!("Queries executed with app comment prefix");

    // ============================================
    // Example 2: Safe DELETE hook
    // ============================================
    println!("\n=== Example 2: SafeDeleteHook ===\n");

    let client = InstrumentedClient::new(&*raw_client).with_hook(SafeDeleteHook);

    // This will be blocked (no WHERE clause)
    let result = query("DELETE FROM users").execute(&client).await;

    match result {
        Err(OrmError::Validation(msg)) => {
            println!("DELETE blocked as expected: {}", msg);
        }
        Ok(_) => println!("WARNING: DELETE was not blocked!"),
        Err(e) => println!("Other error: {}", e),
    }

    // This will work (has WHERE clause)
    let affected = query("DELETE FROM users WHERE username = $1")
        .bind("nonexistent")
        .execute(&client)
        .await?;

    println!("DELETE with WHERE succeeded: {} rows affected", affected);

    // ============================================
    // Example 3: Read-only mode
    // ============================================
    println!("\n=== Example 3: ReadOnlyHook ===\n");

    let client = InstrumentedClient::new(&*raw_client).with_hook(ReadOnlyHook);

    // SELECT works
    let users: Vec<User> = query("SELECT * FROM users").fetch_all_as(&client).await?;

    println!("SELECT worked: {} users found", users.len());

    // INSERT is blocked
    let result = query("INSERT INTO users (username) VALUES ($1)")
        .bind("blocked_user")
        .execute(&client)
        .await;

    match result {
        Err(OrmError::Validation(msg)) => {
            println!("INSERT blocked: {}", msg);
        }
        _ => println!("WARNING: INSERT was not blocked!"),
    }

    // ============================================
    // Example 4: Audit logging
    // ============================================
    println!("\n=== Example 4: AuditLogHook ===\n");

    let audit_hook = Arc::new(AuditLogHook::new());
    let client =
        InstrumentedClient::new(&*raw_client).with_hook_arc(Arc::clone(&audit_hook) as Arc<dyn QueryHook>);

    // Perform some mutations
    query("INSERT INTO users (username) VALUES ($1)")
        .bind("audit_user1")
        .execute(&client)
        .await?;

    query("INSERT INTO users (username) VALUES ($1)")
        .bind("audit_user2")
        .execute(&client)
        .await?;

    query("UPDATE users SET email = $1 WHERE username = $2")
        .bind(Some("audit@example.com"))
        .bind("audit_user1")
        .execute(&client)
        .await?;

    // SELECT is not logged (not a mutation)
    let _: Vec<User> = query("SELECT * FROM users").fetch_all_as(&client).await?;

    query("DELETE FROM users WHERE username = $1")
        .bind("audit_user2")
        .execute(&client)
        .await?;

    // Print audit log
    println!("Audit log entries:");
    for (i, entry) in audit_hook.entries().iter().enumerate() {
        println!(
            "  {}. {:?} ({:?}) - success: {}",
            i + 1,
            entry.query_type,
            entry.duration,
            entry.success
        );
        println!("     SQL: {}", &entry.sql[..60.min(entry.sql.len())]);
    }

    // ============================================
    // Example 5: CompositeHook (chaining hooks)
    // ============================================
    println!("\n=== Example 5: CompositeHook ===\n");

    let composite = CompositeHook::new()
        .add(AppCommentHook::new("composite-app"))
        .add(SafeDeleteHook)
        .add(ParameterValidationHook::new(100));

    let client = InstrumentedClient::new(&*raw_client)
        .with_hook(composite)
        .with_monitor(LoggingMonitor::new().prefix("[COMPOSITE]").max_sql_length(120));

    // This query goes through all hooks
    query("INSERT INTO users (username, email) VALUES ($1, $2)")
        .bind("composite_user")
        .bind(Some("composite@example.com"))
        .execute(&client)
        .await?;

    let _: Vec<User> = query("SELECT * FROM users WHERE username = $1")
        .bind("composite_user")
        .fetch_all_as(&client)
        .await?;

    // SafeDeleteHook will still block unsafe DELETE
    let result = query("DELETE FROM users").execute(&client).await;

    match result {
        Err(OrmError::Validation(_)) => println!("Composite hook correctly blocked DELETE"),
        _ => println!("WARNING: DELETE was not blocked!"),
    }

    // ============================================
    // Example 6: Parameter validation
    // ============================================
    println!("\n=== Example 6: ParameterValidationHook ===\n");

    let client = InstrumentedClient::new(&*raw_client).with_hook(ParameterValidationHook::new(3));

    // This works (2 params <= 3)
    query("INSERT INTO users (username, email) VALUES ($1, $2)")
        .bind("param_user")
        .bind(Some("param@example.com"))
        .execute(&client)
        .await?;

    println!("Query with 2 params succeeded");

    // This would be blocked if we had > 3 params
    // (In real usage, you might set a higher limit)

    // Cleanup
    raw_client
        .execute("DELETE FROM users WHERE username LIKE 'audit%'", &[])
        .await
        .map_err(OrmError::from_db_error)?;

    raw_client
        .execute("DELETE FROM users WHERE username = 'composite_user'", &[])
        .await
        .map_err(OrmError::from_db_error)?;

    raw_client
        .execute("DELETE FROM users WHERE username = 'param_user'", &[])
        .await
        .map_err(OrmError::from_db_error)?;

    println!("\n=== Hook Examples Complete ===");
    Ok(())
}
