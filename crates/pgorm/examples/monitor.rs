//! Query monitoring example for pgorm
//!
//! Run with: cargo run --example monitor -p pgorm
//!
//! Set DATABASE_URL in .env file or environment variable:
//! DATABASE_URL=postgres://postgres:postgres@localhost/pgorm_example
//!
//! This example demonstrates:
//! - LoggingMonitor for query logging
//! - StatsMonitor for collecting statistics
//! - CompositeMonitor for combining multiple monitors
//! - Custom monitor implementation
//! - Slow query detection

use pgorm::{
    create_pool, query, CompositeMonitor, FromRow, InstrumentedClient, LoggingMonitor, Model,
    OrmError, QueryContext, QueryMonitor, QueryResult, StatsMonitor,
};
use std::env;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
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

/// Custom monitor that counts queries by type
struct QueryCounter {
    selects: AtomicU64,
    inserts: AtomicU64,
    updates: AtomicU64,
    deletes: AtomicU64,
}

impl QueryCounter {
    fn new() -> Self {
        Self {
            selects: AtomicU64::new(0),
            inserts: AtomicU64::new(0),
            updates: AtomicU64::new(0),
            deletes: AtomicU64::new(0),
        }
    }

    fn report(&self) {
        println!("\n=== Query Counter Report ===");
        println!("SELECTs: {}", self.selects.load(Ordering::Relaxed));
        println!("INSERTs: {}", self.inserts.load(Ordering::Relaxed));
        println!("UPDATEs: {}", self.updates.load(Ordering::Relaxed));
        println!("DELETEs: {}", self.deletes.load(Ordering::Relaxed));
    }
}

impl QueryMonitor for QueryCounter {
    fn on_query_complete(&self, ctx: &QueryContext, _duration: Duration, _result: &QueryResult) {
        match ctx.query_type {
            pgorm::QueryType::Select => self.selects.fetch_add(1, Ordering::Relaxed),
            pgorm::QueryType::Insert => self.inserts.fetch_add(1, Ordering::Relaxed),
            pgorm::QueryType::Update => self.updates.fetch_add(1, Ordering::Relaxed),
            pgorm::QueryType::Delete => self.deletes.fetch_add(1, Ordering::Relaxed),
            pgorm::QueryType::Other => 0,
        };
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
    // Example 1: Simple LoggingMonitor
    // ============================================
    println!("=== Example 1: LoggingMonitor ===\n");

    let logging_monitor = LoggingMonitor::new()
        .prefix("[SQL]")
        .max_sql_length(80);

    let client = InstrumentedClient::new(&*raw_client).with_monitor(logging_monitor);

    // These queries will be logged to stderr
    query("INSERT INTO users (username, email) VALUES ($1, $2) RETURNING *")
        .bind("alice")
        .bind(Some("alice@example.com"))
        .fetch_one_as::<User>(&client)
        .await?;

    query("SELECT * FROM users WHERE username = $1")
        .bind("alice")
        .fetch_one_as::<User>(&client)
        .await?;

    // ============================================
    // Example 2: StatsMonitor for metrics
    // ============================================
    println!("\n=== Example 2: StatsMonitor ===\n");

    let stats_monitor = Arc::new(StatsMonitor::new());
    let client = InstrumentedClient::new(&*raw_client)
        .with_monitor_arc(Arc::clone(&stats_monitor) as Arc<dyn QueryMonitor>);

    // Run some queries
    for i in 0..5 {
        query("INSERT INTO users (username, email) VALUES ($1, $2)")
            .bind(format!("user{}", i))
            .bind(Some(format!("user{}@example.com", i)))
            .execute(&client)
            .await?;
    }

    let _users: Vec<User> = query("SELECT * FROM users").fetch_all_as(&client).await?;

    query("UPDATE users SET email = $1 WHERE username = $2")
        .bind(Some("updated@example.com"))
        .bind("user0")
        .execute(&client)
        .await?;

    // Print statistics
    let stats = stats_monitor.stats();
    println!("Total queries: {}", stats.total_queries);
    println!("Failed queries: {}", stats.failed_queries);
    println!("Total duration: {:?}", stats.total_duration);
    println!("SELECT count: {}", stats.select_count);
    println!("INSERT count: {}", stats.insert_count);
    println!("UPDATE count: {}", stats.update_count);
    println!("DELETE count: {}", stats.delete_count);
    println!("Max duration: {:?}", stats.max_duration);
    if let Some(slowest) = &stats.slowest_query {
        println!("Slowest query: {}", slowest);
    }

    // ============================================
    // Example 3: CompositeMonitor (multiple monitors)
    // ============================================
    println!("\n=== Example 3: CompositeMonitor ===\n");

    let counter = Arc::new(QueryCounter::new());
    let stats = Arc::new(StatsMonitor::new());

    let composite = CompositeMonitor::new()
        .add_arc(Arc::clone(&counter) as Arc<dyn QueryMonitor>)
        .add_arc(Arc::clone(&stats) as Arc<dyn QueryMonitor>)
        .add(LoggingMonitor::new().prefix("[COMPOSITE]").max_sql_length(60));

    let client = InstrumentedClient::new(&*raw_client).with_monitor(composite);

    // Run various queries
    let _: Vec<User> = query("SELECT * FROM users").fetch_all_as(&client).await?;

    query("INSERT INTO users (username) VALUES ($1)")
        .bind("composite_user")
        .execute(&client)
        .await?;

    query("UPDATE users SET email = $1 WHERE username = $2")
        .bind(Some("new@email.com"))
        .bind("composite_user")
        .execute(&client)
        .await?;

    query("DELETE FROM users WHERE username = $1")
        .bind("composite_user")
        .execute(&client)
        .await?;

    // Report from custom counter
    counter.report();

    // Report from stats monitor
    let s = stats.stats();
    println!("\nStatsMonitor total: {} queries", s.total_queries);

    // ============================================
    // Example 4: Slow query detection
    // ============================================
    println!("\n=== Example 4: Slow Query Detection ===\n");

    let client = InstrumentedClient::new(&*raw_client)
        .with_monitor(
            LoggingMonitor::new()
                .prefix("[SLOW]")
                .min_duration(Duration::from_micros(1)), // Very low threshold for demo
        )
        .with_slow_query_threshold(Duration::from_micros(100));

    // Run a query (will likely trigger slow query warning)
    let _: Vec<User> = query("SELECT * FROM users ORDER BY id")
        .fetch_all_as(&client)
        .await?;

    // ============================================
    // Example 5: Filter logging by duration
    // ============================================
    println!("\n=== Example 5: Duration-filtered Logging ===\n");

    // Only log queries that take more than 10ms
    let filtered_monitor = LoggingMonitor::new()
        .prefix("[SLOW-ONLY]")
        .min_duration(Duration::from_millis(10));

    let client = InstrumentedClient::new(&*raw_client).with_monitor(filtered_monitor);

    // Fast query - won't be logged
    let _: Option<User> = query("SELECT * FROM users WHERE id = $1")
        .bind(1_i64)
        .fetch_opt_as(&client)
        .await?;

    println!("Fast query executed (not logged because < 10ms)");

    // ============================================
    // Example 6: Custom monitor with alerting
    // ============================================
    println!("\n=== Example 6: Custom Alerting Monitor ===\n");

    struct AlertingMonitor;

    impl QueryMonitor for AlertingMonitor {
        fn on_query_start(&self, ctx: &QueryContext) {
            println!("  [START] {:?}: {}", ctx.query_type, &ctx.sql[..50.min(ctx.sql.len())]);
        }

        fn on_query_complete(&self, ctx: &QueryContext, duration: Duration, result: &QueryResult) {
            let status = match result {
                QueryResult::Error(_) => "FAILED",
                _ => "OK",
            };
            println!(
                "  [END]   {:?}: {:?} - {} - {}",
                ctx.query_type, duration, status, result
            );
        }

        fn on_slow_query(&self, ctx: &QueryContext, duration: Duration) {
            eprintln!(
                "  [ALERT] SLOW QUERY DETECTED! {:?} took {:?}",
                ctx.query_type, duration
            );
            eprintln!("  [ALERT] SQL: {}", ctx.sql);
        }
    }

    let client = InstrumentedClient::new(&*raw_client)
        .with_monitor(AlertingMonitor)
        .with_slow_query_threshold(Duration::from_micros(500));

    // Run queries
    query("INSERT INTO users (username) VALUES ($1)")
        .bind("alert_user")
        .execute(&client)
        .await?;

    let _: Vec<User> = query("SELECT * FROM users").fetch_all_as(&client).await?;

    query("DELETE FROM users WHERE username = $1")
        .bind("alert_user")
        .execute(&client)
        .await?;

    println!("\n=== Monitor Examples Complete ===");
    Ok(())
}
