//! Example demonstrating query monitoring + hooks via InstrumentedClient.
//!
//! Run with:
//!   cargo run --example monitoring -p pgorm
//!
//! Requires:
//!   DATABASE_URL=postgres://postgres:postgres@localhost/pgorm_example

use pgorm::monitor::{
    CompositeMonitor, HookAction, InstrumentedClient, LoggingMonitor, MonitorConfig, QueryContext,
    QueryHook, StatsMonitor,
};
use pgorm::{OrmError, OrmResult, query};
use std::env;
use std::sync::Arc;
use std::time::Duration;

struct AddCommentHook;

impl QueryHook for AddCommentHook {
    fn before_query(&self, ctx: &QueryContext) -> HookAction {
        HookAction::ModifySql {
            exec_sql: format!("/* monitoring-example */ {}", ctx.exec_sql),
            canonical_sql: None,
        }
    }
}

/// A tiny safety hook for demo purposes: block `DELETE` without `WHERE`.
struct BlockDangerousDeleteHook;

impl QueryHook for BlockDangerousDeleteHook {
    fn before_query(&self, ctx: &QueryContext) -> HookAction {
        if ctx.query_type != pgorm::monitor::QueryType::Delete {
            return HookAction::Continue;
        }
        let s = ctx.canonical_sql.to_ascii_lowercase();
        if s.contains(" where ") {
            HookAction::Continue
        } else {
            HookAction::Abort("blocked: DELETE without WHERE".into())
        }
    }
}

#[tokio::main]
async fn main() -> OrmResult<()> {
    dotenvy::dotenv().ok();
    let database_url = env::var("DATABASE_URL")
        .map_err(|_| OrmError::Connection("DATABASE_URL is not set".into()))?;

    let (client, connection) = tokio_postgres::connect(&database_url, tokio_postgres::NoTls)
        .await
        .map_err(OrmError::from_db_error)?;
    tokio::spawn(async move {
        let _ = connection.await;
    });

    // Minimal schema for the demo.
    query("DROP TABLE IF EXISTS items CASCADE")
        .execute(&client)
        .await?;
    query(
        "CREATE TABLE items (
            id BIGSERIAL PRIMARY KEY,
            name TEXT NOT NULL
        )",
    )
    .execute(&client)
    .await?;
    query("INSERT INTO items (name) VALUES ($1), ($2)")
        .bind("a")
        .bind("b")
        .execute(&client)
        .await?;

    let stats = Arc::new(StatsMonitor::new());
    let monitor = CompositeMonitor::new()
        .add(
            LoggingMonitor::new()
                .prefix("[pgorm-monitor]")
                .min_duration(Duration::from_millis(0)),
        )
        .add_arc(stats.clone());

    let config = MonitorConfig::new()
        .with_query_timeout(Duration::from_millis(20))
        .with_slow_query_threshold(Duration::from_millis(5))
        .enable_monitoring();

    let pg = InstrumentedClient::new(client)
        .with_config(config)
        .with_monitor(monitor)
        .with_hook(BlockDangerousDeleteHook)
        .add_hook(AddCommentHook);

    // 1) A normal query
    let n: i64 = pgorm::query("SELECT COUNT(*) FROM items")
        .tag("items.count")
        .fetch_scalar_one(&pg)
        .await?;
    println!("items count = {n}");

    // 2) Slow query (should trigger slow-query path if threshold is low)
    let _ = pgorm::query("SELECT pg_sleep(0.01)")
        .tag("demo.sleep")
        .fetch_one(&pg)
        .await?;

    // 3) Timeout demo
    match pgorm::query("SELECT pg_sleep(0.05)")
        .tag("demo.timeout")
        .fetch_one(&pg)
        .await
    {
        Ok(_) => println!("unexpected: sleep(0.05) finished within timeout"),
        Err(OrmError::Timeout(d)) => println!("timeout as expected after {d:?}"),
        Err(e) => println!("unexpected error: {e}"),
    }

    // 4) Hook abort demo (DELETE without WHERE)
    match pgorm::query("DELETE FROM items")
        .tag("demo.delete.no_where")
        .execute(&pg)
        .await
    {
        Ok(_) => println!("unexpected: dangerous delete was allowed"),
        Err(e) => println!("hook blocked delete as expected: {e}"),
    }

    // 5) Allowed delete
    let affected = pgorm::query("DELETE FROM items WHERE id = $1")
        .tag("demo.delete.where")
        .bind(1_i64)
        .execute(&pg)
        .await?;
    println!("delete with WHERE affected = {affected}");

    println!("\nstats snapshot: {:?}", stats.stats());
    Ok(())
}
