# 连接与连接池

pgorm 通过 `deadpool-postgres` 提供连接池，并推荐使用 `PgClient` 封装，它集成了监控、SQL 检查和预处理语句缓存。

## 使用 `create_pool` 快速开始

最简单的连接方式是 `create_pool`，它解析 `DATABASE_URL` 并返回一个带有合理默认值的连接池（NoTls）：

```rust
use pgorm::create_pool;

let pool = create_pool(&std::env::var("DATABASE_URL")?)?;
let client = pool.get().await?;
```

## PgClient（推荐）

`PgClient` 封装任意 `GenericClient`（连接池连接、原始客户端或事务），并添加以下功能：

- **SQL 模式检查** -- 根据已注册的 `#[derive(Model)]` 模式校验查询
- **查询统计** -- 追踪查询次数、耗时和慢查询
- **日志** -- 可选的查询日志，支持配置阈值
- **预处理语句缓存** -- LRU 缓存预处理语句
- **安全策略** -- 在运行时拦截危险的 DML 模式

### 基本用法

```rust
use pgorm::{PgClient, PgClientConfig, create_pool};

let pool = create_pool(&database_url)?;
let client = pool.get().await?;

// 默认配置：WarnOnly 检查模式、无统计、无缓存
let pg = PgClient::new(&client);
```

### 配置用法

```rust
use pgorm::{PgClient, PgClientConfig, CheckMode, create_pool};
use std::time::Duration;

let pool = create_pool(&database_url)?;
let client = pool.get().await?;

let pg = PgClient::with_config(&client, PgClientConfig::new()
    .timeout(Duration::from_secs(30))
    .slow_threshold(Duration::from_secs(1))
    .with_logging()
    .with_stats()
    .statement_cache(64)
    .strict());
```

## PgClientConfig 配置项

| 方法 | 描述 |
|------|------|
| `.timeout(Duration)` | 设置查询超时时间 |
| `.slow_threshold(Duration)` | 设置慢查询告警阈值 |
| `.with_logging()` | 启用查询日志 |
| `.log_slow_queries(Duration)` | 仅记录超过指定时长的查询 |
| `.with_stats()` | 启用查询统计收集 |
| `.statement_cache(cap)` | 启用 LRU 预处理语句缓存，指定容量 |
| `.strict()` | 将 SQL 检查设为 `Strict` 模式（拦截无效查询） |
| `.no_check()` | 完全禁用 SQL 检查 |
| `.check_mode(CheckMode)` | 设置检查模式：`Disabled`、`WarnOnly` 或 `Strict` |

### SQL 检查模式

- **`Disabled`** -- 不进行检查
- **`WarnOnly`**（默认） -- 对未知表/列记录警告日志，但仍执行查询
- **`Strict`** -- 对未知表/列在执行前返回错误

```rust
use pgorm::{PgClient, PgClientConfig};

// Strict：拦截引用未知表/列的查询
let pg = PgClient::with_config(&client, PgClientConfig::new().strict());

// Disabled：跳过所有 SQL 检查
let pg = PgClient::with_config(&client, PgClientConfig::new().no_check());
```

## 预处理语句缓存

PgClient 内置了 LRU 预处理语句缓存。预处理语句是按连接粒度的，因此缓存也是按连接粒度。当缓存满时，最近最少使用的语句会被淘汰。

```rust
use pgorm::{PgClient, PgClientConfig, query};
use tokio_postgres::NoTls;

// 不使用连接池直接连接（原始 tokio_postgres::Client）
let (client, connection) = tokio_postgres::connect(&database_url, NoTls)
    .await
    .map_err(pgorm::OrmError::from_db_error)?;
tokio::spawn(async move { let _ = connection.await; });

// 启用容量为 64 的缓存
let pg = PgClient::with_config(client, PgClientConfig::new()
    .no_check()
    .statement_cache(64));

// 首次执行：准备语句（缓存未命中）
let _v: i64 = query("SELECT $1::bigint + $2::bigint")
    .tag("add")
    .bind(1_i64)
    .bind(2_i64)
    .fetch_scalar_one(&pg)
    .await?;

// 后续执行：命中缓存（无需重新准备）
let _v: i64 = query("SELECT $1::bigint + $2::bigint")
    .tag("add")
    .bind(3_i64)
    .bind(4_i64)
    .fetch_scalar_one(&pg)
    .await?;

// 查看缓存统计
let stats = pg.stats();
println!(
    "hits={}, misses={}, prepares={}, prepare_time={:?}",
    stats.stmt_cache_hits,
    stats.stmt_cache_misses,
    stats.stmt_prepare_count,
    stats.stmt_prepare_duration
);
```

## 查询统计

启用统计后，`PgClient` 会追踪查询次数和耗时：

```rust
let pg = PgClient::with_config(&client, PgClientConfig::new()
    .with_stats()
    .log_slow_queries(Duration::from_millis(50)));

// 执行查询...
let users = User::select_all(&pg).await?;

// 读取统计信息
let stats = pg.stats();
println!("total queries: {}", stats.total_queries);
println!("SELECT count: {}", stats.select_count);
println!("max duration: {:?}", stats.max_duration);

// 重置统计以开始新的测量窗口
pg.reset_stats();
```

## 自定义连接池配置

生产环境中，建议配置连接池大小、回收策略和 TLS：

```rust
use deadpool_postgres::{ManagerConfig, RecyclingMethod};
use pgorm::create_pool_with_manager_config;
use tokio_postgres::NoTls;

let mgr_cfg = ManagerConfig {
    recycling_method: RecyclingMethod::Fast,
};

let pool = create_pool_with_manager_config(
    &database_url,
    NoTls,
    mgr_cfg,
    |b| b.max_size(32)
)?;
```

## TLS 支持

使用 `create_pool_with_tls` 通过 TLS 连接器进行连接：

```rust
use pgorm::create_pool_with_tls;

let tls = /* 例如 tokio_postgres_rustls::MakeRustlsConnect */;
let pool = create_pool_with_tls(&database_url, tls)?;
```

## 不使用连接池

你可以不使用 `deadpool-postgres`，直接通过 `tokio_postgres::connect` 连接来使用 pgorm：

```rust
use tokio_postgres::NoTls;

let (client, connection) = tokio_postgres::connect(&database_url, NoTls)
    .await
    .map_err(pgorm::OrmError::from_db_error)?;
tokio::spawn(async move { let _ = connection.await; });

// 直接使用原始客户端
let users = User::select_all(&client).await?;

// 或用 PgClient 封装以获得监控功能
let pg = PgClient::with_config(client, PgClientConfig::new()
    .statement_cache(64)
    .with_stats());
```

---

下一步：[模型与派生宏](/zh/guide/models)
