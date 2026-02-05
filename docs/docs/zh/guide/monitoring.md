# 监控与 Hooks

pgorm 提供了一个监控层，可以包装任意 `GenericClient` 来添加查询指标、日志记录、超时控制、慢查询检测和自定义 Hook。当你需要了解正在执行什么 SQL 以及其性能表现时，可以使用该功能。

> 如果你还需要运行时 SQL 检查和安全策略，请使用 [`PgClient`](/zh/guide/safety)，它构建在 `InstrumentedClient` 之上。

## 1. `InstrumentedClient` 设置

`InstrumentedClient` 包装原始的 `tokio_postgres::Client`（或连接池连接）并添加监控能力：

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

// 通过 `pg` 执行的所有查询现在都会被监控
let n: i64 = pgorm::query("SELECT COUNT(*) FROM items")
    .tag("items.count")
    .fetch_scalar_one(&pg)
    .await?;

println!("stats = {:?}", stats.stats());
```

## 2. 监控器

监控器接收查询执行的通知。pgorm 内置了两个监控器和一个组合器。

### `LoggingMonitor`

通过 `eprintln!` 将查询执行信息输出到 stderr。可配置项：

- **`prefix(str)`** -- 每行日志前缀字符串（例如 `"[pgorm]"`）
- **`min_duration(Duration)`** -- 仅记录执行时间超过此阈值的查询

```rust
use pgorm::monitor::LoggingMonitor;
use std::time::Duration;

let logger = LoggingMonitor::new()
    .prefix("[pgorm-monitor]")
    .min_duration(Duration::from_millis(0)); // 记录所有查询
```

### `StatsMonitor`

收集聚合查询统计信息：总查询数、按类型统计（SELECT、INSERT、UPDATE、DELETE）、总耗时和最大耗时。通过 `Arc` 实现线程安全。

```rust
use pgorm::monitor::StatsMonitor;
use std::sync::Arc;

let stats = Arc::new(StatsMonitor::new());

// 执行查询后...
let snapshot = stats.stats();
println!("total queries: {}", snapshot.total_queries);
println!("select count: {}", snapshot.select_count);
println!("max duration: {:?}", snapshot.max_duration);
```

### `CompositeMonitor`

组合多个监控器，使它们都能接收事件：

```rust
use pgorm::monitor::CompositeMonitor;

let monitor = CompositeMonitor::new()
    .add(LoggingMonitor::new().prefix("[sql]"))
    .add_arc(stats.clone());
```

使用 `.add()` 添加拥有所有权的监控器，使用 `.add_arc()` 添加 `Arc` 包装的监控器（适用于后续需要查询的场景，如 `StatsMonitor`）。

## 3. `MonitorConfig`

`MonitorConfig` 控制 `InstrumentedClient` 的行为：

```rust
use pgorm::monitor::MonitorConfig;
use std::time::Duration;

let config = MonitorConfig::new()
    .with_query_timeout(Duration::from_secs(30))        // 超过此时间的查询将被取消
    .with_slow_query_threshold(Duration::from_millis(100)) // 标记慢查询
    .enable_monitoring();                                 // 激活监控层
```

关键选项：

- **`with_query_timeout(Duration)`** -- 超过此时长的查询将被取消并返回 `OrmError::Timeout`
- **`with_slow_query_threshold(Duration)`** -- 超过此时长的查询将被报告为慢查询
- **`enable_monitoring()`** -- 必须调用此方法才能激活监控（默认关闭）

## 4. Hooks：`QueryHook` trait

Hooks 允许你在查询执行前拦截、修改或阻止查询。实现 `QueryHook` trait 即可：

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

### `HookAction` 变体

- **`HookAction::Continue`** -- 正常继续执行
- **`HookAction::Abort(String)`** -- 取消查询并返回包含给定消息的错误
- **`HookAction::ModifySql { exec_sql, canonical_sql }`** -- 在执行前重写 SQL（例如添加 SQL 注释用于追踪）

### 挂载 Hooks

使用 `.with_hook()` 设置主 Hook，或使用 `.add_hook()` 追加额外的 Hook：

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

### 超时和 Hook 中止行为

查询超时时，你会收到 `OrmError::Timeout`。Hook 中止时，你会收到包含中止消息的 `OrmError`：

```rust
// 超时示例
match pgorm::query("SELECT pg_sleep(10)")
    .fetch_one(&pg).await
{
    Err(OrmError::Timeout(d)) => println!("timed out after {d:?}"),
    _ => {}
}

// Hook 中止示例
match pgorm::query("DELETE FROM items")
    .execute(&pg).await
{
    Err(e) => println!("hook blocked: {e}"),
    _ => {}
}
```

## 5. Tracing 集成（`tracing` feature）

如果你的应用使用了 `tracing` crate，可以启用 `tracing` feature，让 pgorm 将实际执行的 SQL 作为 tracing 事件发出：

```toml
[dependencies]
pgorm = { version = "0.2.0", features = ["tracing"] }
```

然后添加 `TracingSqlHook`：

```rust
use pgorm::monitor::{InstrumentedClient, MonitorConfig, TracingSqlHook};

let pg = InstrumentedClient::new(client)
    .with_config(MonitorConfig::new().enable_monitoring())
    .add_hook(TracingSqlHook::new());
```

这会发出 target 为 `pgorm.sql` 的 `tracing` 事件，包含 `sql`（执行的 SQL）、`tag`、`query_type` 和 `param_count` 等字段。如果你同时使用了会修改 SQL 的 Hook，建议将 `TracingSqlHook` 放在最后，以便捕获最终的 SQL。

## 6. 通过 `PgClient::stats()` 监控语句缓存

使用带语句缓存的 `PgClient` 时，你可以监控缓存性能：

```rust
use pgorm::{PgClient, PgClientConfig, query};

let pg = PgClient::with_config(client, PgClientConfig::new().no_check().statement_cache(64));

// 执行一些查询...
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

// 重置统计信息以开始新的测量窗口
pg.reset_stats();
```

语句缓存使用 LRU 淘汰策略 -- 当缓存满时，最近最少使用的预处理语句将被淘汰。这样既避免了内存无限增长，又能让热点查询保持高速。

## 可运行示例

- `crates/pgorm/examples/monitoring/main.rs` -- 慢查询检测、超时、Hook 中止/修改、统计快照
- `crates/pgorm/examples/statement_cache/main.rs` -- 带 LRU 淘汰和统计的预处理语句缓存

## 下一步

- 下一章：[SQL 安全与检查](/zh/guide/safety)
