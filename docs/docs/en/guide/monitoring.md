# Monitoring & Hooks: `InstrumentedClient`

Use `InstrumentedClient` when you want to:

- record query stats (counts/durations/slow queries)
- log queries (grouped by tag)
- intercept/modify/block SQL before execution

> If you also want “runtime SQL checking + safety policies”, use [`PgClient`](/en/guide/runtime-sql-check).

## 1) Minimal example: logging + stats

```rust
use pgorm::{
    CompositeMonitor, InstrumentedClient, LoggingMonitor, MonitorConfig, StatsMonitor,
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

let n: i64 = pgorm::query("SELECT COUNT(*) FROM items")
    .tag("items.count")
    .fetch_scalar_one(&pg)
    .await?;

println!("stats = {:?}", stats.stats());
```

## 2) Hooks: intercept/modify/block

Implement `QueryHook`:

```rust
use pgorm::{HookAction, QueryContext, QueryHook};

/// Demo safety hook: block DELETE without WHERE.
struct BlockDangerousDeleteHook;

impl QueryHook for BlockDangerousDeleteHook {
    fn before_query(&self, ctx: &QueryContext) -> HookAction {
        if ctx.query_type == pgorm::QueryType::Delete {
            let s = ctx.canonical_sql.to_ascii_lowercase();
            if !s.contains(" where ") {
                return HookAction::Abort("blocked: DELETE without WHERE".into());
            }
        }
        HookAction::Continue
    }
}
```

Attach it:

```rust
let pg = InstrumentedClient::new(client)
    .with_config(MonitorConfig::new().enable_monitoring())
    .with_hook(BlockDangerousDeleteHook);
```

## 3) Runnable example

- `crates/pgorm/examples/monitoring`: slow query, timeouts, hook abort/modify, stats snapshot

## Next

- Next: [`Runtime SQL Checking: PgClient / CheckedClient`](/en/guide/runtime-sql-check)
