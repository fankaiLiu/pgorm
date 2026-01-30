# 连接池

pgorm 提供了使用 deadpool-postgres 的连接池辅助函数。

## 快速开始

`create_pool` 是一个快速启动辅助函数，使用 `NoTls` 和一组默认配置（适用于本地/开发）：

```rust
use pgorm::create_pool;

let pool = create_pool(&database_url)?;
let client = pool.get().await?;
```

## 自定义连接池配置

对于生产环境，从应用配置注入 TLS 和连接池设置：

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

TLS 连接器原样传递：

```rust
use pgorm::create_pool_with_tls;

let tls = /* 例如 tokio_postgres_rustls::MakeRustlsConnect */;
let pool = create_pool_with_tls(&database_url, tls)?;
```

## 推荐客户端（监控 + SQL 检查）

如果你在生成 SQL（尤其是使用 AI），包装你的客户端以获得保护：

```rust
use pgorm::{create_pool, PgClient, PgClientConfig};

let pool = create_pool(&database_url)?;
let client = pool.get().await?;
let pg = PgClient::with_config(client, PgClientConfig::new().strict());

// 现在所有 pgorm 查询都经过检查和监控。
let user: User = pgorm::query("SELECT id, username FROM users WHERE id = $1")
    .tag("users.by_id")
    .bind(1_i64)
    .fetch_one_as(&pg)
    .await?;
```

## 迁移

启用 `migrate` 功能并从应用嵌入迁移：

```rust
use pgorm::{create_pool, migrate};

mod embedded {
    use pgorm::embed_migrations;
    embed_migrations!("./migrations");
}

let pool = create_pool(&database_url)?;
migrate::run_pool(&pool, embedded::migrations::runner()).await?;
```
