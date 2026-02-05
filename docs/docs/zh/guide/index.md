# 快速开始

pgorm 是一个**模型定义优先、AI 友好的 PostgreSQL ORM**，专为 Rust 设计。它根据模型定义生成查询，提供运行时 SQL 检查以校验 AI 生成的查询，并将监控、连接池和预处理语句缓存整合到单一的 `PgClient` 封装中。

- **版本：** 0.2.0
- **最低支持 Rust 版本（MSRV）：** Rust 1.88+
- **Edition：** 2024

## 安装

```toml
[dependencies]
pgorm = "0.2.0"
```

## 定义模型

使用 `#[derive(FromRow, Model)]` 定义模型。pgorm 会自动为你生成类型化常量、CRUD 方法和查询构建器。

```rust
use pgorm::prelude::*;

#[derive(Debug, Clone, FromRow, Model)]
#[orm(table = "users")]
struct User {
    #[orm(id)]
    id: i64,
    name: String,
    email: String,
}
```

## 连接

使用 `create_pool` 创建连接池，然后通过 `PgClient` 封装以获得监控、SQL 检查和预处理语句缓存功能。

```rust
use pgorm::{PgClient, PgClientConfig, create_pool};
use std::time::Duration;

let pool = create_pool(&std::env::var("DATABASE_URL")?)?;
let client = pool.get().await?;
let pg = PgClient::with_config(&client, PgClientConfig::new()
    .timeout(Duration::from_secs(30))
    .slow_threshold(Duration::from_secs(1))
    .with_logging());
```

## 查询

使用模型生成的方法或原始 SQL —— 两者都通过 `PgClient` 进行监控。

```rust
// 基于模型的查询
let users = User::select_all(&pg).await?;

// 原始 SQL 查询并映射到模型
let active: Vec<User> = pg.sql_query_as(
    "SELECT * FROM users WHERE status = $1",
    &[&"active"],
).await?;

// 带过滤条件的查询构建器
let admins = User::query()
    .eq(UserQuery::COL_EMAIL, "admin@example.com")?
    .find(&pg)
    .await?;

// 查看查询统计信息
let stats = pg.stats();
println!("total queries: {}, max: {:?}", stats.total_queries, stats.max_duration);
```

## 插入

使用 `#[derive(InsertModel)]` 定义插入模型，支持单条和批量插入。

```rust
use pgorm::InsertModel;

#[derive(InsertModel)]
#[orm(table = "users", returning = "User")]
struct NewUser {
    name: String,
    email: String,
}

// 单条插入并返回结果（RETURNING）
let user = NewUser {
    name: "Alice".into(),
    email: "alice@example.com".into(),
}.insert_returning(&pg).await?;

// 使用 UNNEST 批量插入
let users = NewUser::insert_many_returning(&pg, vec![
    NewUser { name: "Bob".into(), email: "bob@example.com".into() },
    NewUser { name: "Carol".into(), email: "carol@example.com".into() },
]).await?;
```

## 下一步

按照以下指南深入学习：

1. [安装与功能标志](/zh/guide/installation) -- 功能标志、MSRV 和最小化构建
2. [连接与连接池](/zh/guide/connection) -- `PgClient`、预处理语句缓存、TLS
3. [模型与派生宏](/zh/guide/models) -- `FromRow`、`Model`、`QueryParams`、`ViewModel`
4. [关系与预加载](/zh/guide/relations) -- `has_many`、`belongs_to`、`has_one`、`many_to_many`
5. [PostgreSQL 类型](/zh/guide/pg-types) -- `PgEnum`、`PgComposite`、`Range<T>`、JSONB

---

下一步：[安装与功能标志](/zh/guide/installation)
