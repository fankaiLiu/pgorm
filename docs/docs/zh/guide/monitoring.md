# 监控与 Hook：`InstrumentedClient`

当你想要：

- 给查询打点（统计次数/耗时/慢查询）
- 统一加日志（按 tag 分组）
- 在执行前拦截/修改 SQL（例如阻止危险 DML）

可以用 `InstrumentedClient` 包装你的数据库客户端。

> 如果你同时还想要“运行时 SQL 检查 + 安全策略”，可以直接用更高层的 [`PgClient`](/zh/guide/runtime-sql-check)。

## 1) 最小示例：日志 + 统计

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

// 正常使用：你的 query()/sql() 都可以用 &pg 作为 conn
let n: i64 = pgorm::query("SELECT COUNT(*) FROM items")
    .tag("items.count")
    .fetch_scalar_one(&pg)
    .await?;

println!("stats = {:?}", stats.stats());
```

## 2) Hook：在执行前拦截/修改/拒绝

Hook 通过实现 `QueryHook`：

```rust
use pgorm::{HookAction, QueryContext, QueryHook};

/// 阻止不带 WHERE 的 DELETE（示意用）
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

装配到 `InstrumentedClient`：

```rust
let pg = InstrumentedClient::new(client)
    .with_config(MonitorConfig::new().enable_monitoring())
    .with_hook(BlockDangerousDeleteHook);
```

## 3) 看完整可运行示例

- `crates/pgorm/examples/monitoring`：包含慢查询、超时、Hook abort/modify 的演示

## 下一步

- 下一章：[`运行时 SQL 检查：PgClient / CheckedClient`](/zh/guide/runtime-sql-check)
