# Monitoring & Hooks

pgorm provides a monitoring layer that wraps any `GenericClient` to add query metrics, logging, timeouts, slow query detection, and custom hooks. Use it when you want visibility into what SQL is being executed and how it performs.

> If you also want runtime SQL checking and safety policies, use [`PgClient`](/en/guide/safety) which builds on top of `InstrumentedClient`.

## 1. `InstrumentedClient` Setup

`InstrumentedClient` wraps a raw `tokio_postgres::Client` (or pool connection) and adds monitoring capabilities:

```rust
use pgorm::monitor::{
    CompositeMonitor, InstrumentedClient, LoggingMonitor,
    MonitorConfig, StatsMonitor,
};
use std::sync::Arc;
use std::time::Duration;

let stats = Arc::new(StatsMonitor::new());
let monitor = CompositeMonitor::new()
    .add(
        LoggingMonitor::new()
            .prefix("[pgorm]")
            .min_duration(Duration::from_millis(10)),
    )
    .add_arc(stats.clone());

let config = MonitorConfig::new()
    .with_query_timeout(Duration::from_secs(30))
    .with_slow_query_threshold(Duration::from_millis(100))
    .enable_monitoring();

let pg = InstrumentedClient::new(client)
    .with_config(config)
    .with_monitor(monitor);

// All queries through `pg` are now monitored
let n: i64 = pgorm::query("SELECT COUNT(*) FROM items")
    .tag("items.count")
    .fetch_scalar_one(&pg)
    .await?;

println!("stats = {:?}", stats.stats());
```

## 2. Monitors

Monitors receive notifications about query execution. pgorm ships with two built-in monitors and a combinator.

### `LoggingMonitor`

Logs query execution to stderr via `eprintln!`. Configurable with:

- **`prefix(str)`** -- a string prepended to each log line (e.g. `"[pgorm]"`)
- **`min_duration(Duration)`** -- only log queries that take longer than this threshold

```rust
use pgorm::monitor::LoggingMonitor;
use std::time::Duration;

let logger = LoggingMonitor::new()
    .prefix("[pgorm-monitor]")
    .min_duration(Duration::from_millis(0)); // log everything
```

### `StatsMonitor`

Collects aggregate query statistics: total queries, per-type counts (SELECT, INSERT, UPDATE, DELETE), total duration, and max duration. Thread-safe via `Arc`.

```rust
use pgorm::monitor::StatsMonitor;
use std::sync::Arc;

let stats = Arc::new(StatsMonitor::new());

// After running queries...
let snapshot = stats.stats();
println!("total queries: {}", snapshot.total_queries);
println!("select count: {}", snapshot.select_count);
println!("max duration: {:?}", snapshot.max_duration);
```

### `CompositeMonitor`

Combines multiple monitors so they all receive events:

```rust
use pgorm::monitor::CompositeMonitor;

let monitor = CompositeMonitor::new()
    .add(LoggingMonitor::new().prefix("[sql]"))
    .add_arc(stats.clone());
```

Use `.add()` for owned monitors and `.add_arc()` for `Arc`-wrapped monitors you want to query later (like `StatsMonitor`).

## 3. `MonitorConfig`

`MonitorConfig` controls the behavior of `InstrumentedClient`:

```rust
use pgorm::monitor::MonitorConfig;
use std::time::Duration;

let config = MonitorConfig::new()
    .with_query_timeout(Duration::from_secs(30))        // cancel queries exceeding this
    .with_slow_query_threshold(Duration::from_millis(100)) // flag slow queries
    .enable_monitoring();                                 // activate the monitoring layer
```

Key options:

- **`with_query_timeout(Duration)`** -- queries exceeding this duration are cancelled and return `OrmError::Timeout`
- **`with_slow_query_threshold(Duration)`** -- queries exceeding this are reported as slow
- **`enable_monitoring()`** -- must be called to activate monitoring (disabled by default)

## 4. Hooks: `QueryHook` Trait

Hooks let you intercept, modify, or block queries before they are executed. Implement the `QueryHook` trait:

```rust
use pgorm::monitor::{HookAction, QueryContext, QueryHook, QueryType};

struct BlockDangerousDeleteHook;

impl QueryHook for BlockDangerousDeleteHook {
    fn before_query(&self, ctx: &QueryContext) -> HookAction {
        if ctx.query_type == QueryType::Delete {
            let s = ctx.canonical_sql.to_ascii_lowercase();
            if !s.contains(" where ") {
                return HookAction::Abort("blocked: DELETE without WHERE".into());
            }
        }
        HookAction::Continue
    }
}
```

### `HookAction` Variants

- **`HookAction::Continue`** -- proceed with execution as normal
- **`HookAction::Abort(String)`** -- cancel the query and return an error with the given message
- **`HookAction::ModifySql { exec_sql, canonical_sql }`** -- rewrite the SQL before execution (e.g. add a SQL comment for tracing)

### Attaching Hooks

Use `.with_hook()` to set the primary hook, or `.add_hook()` to append additional hooks:

```rust
struct AddCommentHook;

impl QueryHook for AddCommentHook {
    fn before_query(&self, ctx: &QueryContext) -> HookAction {
        HookAction::ModifySql {
            exec_sql: format!("/* monitoring-example */ {}", ctx.exec_sql),
            canonical_sql: None,
        }
    }
}

let pg = InstrumentedClient::new(client)
    .with_config(config)
    .with_monitor(monitor)
    .with_hook(BlockDangerousDeleteHook)
    .add_hook(AddCommentHook);
```

### Timeout and Hook Abort Behavior

When a query times out, you get `OrmError::Timeout`. When a hook aborts, you get an `OrmError` with the abort message:

```rust
// Timeout example
match pgorm::query("SELECT pg_sleep(10)")
    .fetch_one(&pg).await
{
    Err(OrmError::Timeout(d)) => println!("timed out after {d:?}"),
    _ => {}
}

// Hook abort example
match pgorm::query("DELETE FROM items")
    .execute(&pg).await
{
    Err(e) => println!("hook blocked: {e}"),
    _ => {}
}
```

## 5. Tracing Integration (`tracing` Feature)

If your application uses the `tracing` crate, enable the `tracing` feature to have pgorm emit the actual executed SQL as tracing events:

```toml
[dependencies]
pgorm = { version = "0.3.0", features = ["tracing"] }
```

Then add `TracingSqlHook`:

```rust
use pgorm::monitor::{InstrumentedClient, MonitorConfig, TracingSqlHook};

let pg = InstrumentedClient::new(client)
    .with_config(MonitorConfig::new().enable_monitoring())
    .add_hook(TracingSqlHook::new());
```

This emits `tracing` events with target `pgorm.sql`, including fields for `sql` (the executed SQL), `tag`, `query_type`, and `param_count`. If you also use hooks that modify SQL, add `TracingSqlHook` last so it captures the final SQL.

## 6. Statement Cache Monitoring via `PgClient::stats()`

When using `PgClient` with a statement cache, you can monitor cache performance:

```rust
use pgorm::{PgClient, PgClientConfig, query};

let pg = PgClient::with_config(client, PgClientConfig::new().no_check().statement_cache(64));

// Run some queries...
for i in 0..100u64 {
    let _v: i64 = query("SELECT $1::bigint + $2::bigint")
        .tag("examples.add")
        .bind(i as i64)
        .bind(1_i64)
        .fetch_scalar_one(&pg)
        .await?;
}

let stats = pg.stats();
println!("cache hits: {}", stats.stmt_cache_hits);
println!("cache misses: {}", stats.stmt_cache_misses);
println!("prepare count: {}", stats.stmt_prepare_count);
println!("prepare time: {:?}", stats.stmt_prepare_duration);

// Reset stats for a new measurement window
pg.reset_stats();
```

The statement cache uses LRU eviction -- the least recently used prepared statements are evicted when the cache is full. This avoids unbounded memory growth while keeping hot queries fast.

## Runnable Examples

- `crates/pgorm/examples/monitoring/main.rs` -- slow query detection, timeouts, hook abort/modify, stats snapshot
- `crates/pgorm/examples/statement_cache/main.rs` -- prepared statement cache with LRU eviction and stats

## Next

- Next: [SQL Safety & Checking](/en/guide/safety)
