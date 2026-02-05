<p align="center">
  <img src="docs/docs/public/rspress-icon.png" alt="pgorm logo" width="200">
</p>

<h1 align="center">pgorm</h1>

<p align="center">
  <strong>模型定义优先、AI 友好的 Rust PostgreSQL ORM</strong>
</p>

<p align="center">
  <a href="https://crates.io/crates/pgorm"><img src="https://img.shields.io/crates/v/pgorm.svg" alt="crates.io"></a>
  <a href="https://docs.rs/pgorm"><img src="https://docs.rs/pgorm/badge.svg" alt="docs.rs"></a>
  <img src="https://img.shields.io/badge/rust-1.88%2B-orange.svg" alt="MSRV">
  <img src="https://img.shields.io/crates/l/pgorm.svg" alt="license">
</p>

> **注意：** 本项目正在快速迭代中（pre-1.0），API 在小版本之间可能会变化。
> 废弃项至少会有一个小版本的过渡期。
> MSRV: **1.88+** · 遵循 [semver](https://semver.org/) 的 0.x 版本约定。

---

## 功能特性

- **模型定义优先** — 使用派生宏定义模型，pgorm 自动生成查询
- **AI 友好** — 通过 `query()` / `sql()` 显式编写查询，运行时 SQL 检查拦截 AI 生成的错误查询
- **派生宏** — `FromRow`、`Model`、`InsertModel`、`UpdateModel`、`ViewModel`、`QueryParams`
- **连接池** — 基于 `deadpool-postgres`
- **预加载** — 关联关系（`has_many`、`belongs_to`、`has_one`、`many_to_many`）
- **批量插入/Upsert** — 使用 UNNEST 实现最大吞吐
- **批量更新/删除** — 类型安全的条件表达式
- **多表写入图** — 在单个事务中插入多表关联记录
- **乐观锁** — `#[orm(version)]`
- **PostgreSQL 特殊类型** — `PgEnum`、`PgComposite`、`Range<T>` 派生宏
- **事务与保存点** — `transaction!`、`savepoint!`、`nested_transaction!` 宏
- **CTE (WITH) 查询** — 包括递归 CTE
- **游标分页** — `Keyset1`、`Keyset2`，基于索引的稳定分页
- **流式查询** — 逐行 `Stream`，处理大结果集
- **预编译语句缓存** — LRU 淘汰策略
- **查询监控** — 指标统计、日志、Hook、慢查询检测
- **运行时 SQL 检查** — 拦截 AI 生成的错误查询
- **SQL 迁移** — 基于 `refinery`
- **输入验证宏** — 自动生成 Input 结构体
- **JSONB** — 开箱即用

## 安装

```toml
[dependencies]
pgorm = "0.2.0"
```

默认 feature（`pool`、`derive`、`check`、`validate`）覆盖大部分场景。
最小构建（仅 SQL 构建器 + 行映射）：

```toml
pgorm = { version = "0.2.0", default-features = false }
```

## 快速上手

**推荐：** 使用 `PgClient` — 它集成了监控、SQL 检查、语句缓存和安全策略。

```rust
use pgorm::prelude::*;
use pgorm::{PgClient, PgClientConfig, create_pool};
use std::time::Duration;

// 1. 定义模型
#[derive(Debug, Clone, FromRow, Model)]
#[orm(table = "users")]
struct User {
    #[orm(id)]
    id: i64,
    name: String,
    email: String,
}

// 2. 通过连接池 + PgClient 连接
let pool = create_pool(&std::env::var("DATABASE_URL")?)?;
let client = pool.get().await?;
let pg = PgClient::with_config(&client, PgClientConfig::new()
    .timeout(Duration::from_secs(30))
    .slow_threshold(Duration::from_secs(1))
    .with_logging());

// 3. 模型查询或 SQL 查询 — 都会被监控
let users = User::select_all(&pg).await?;

let active: Vec<User> = pg.sql_query_as(
    "SELECT * FROM users WHERE status = $1",
    &[&"active"],
).await?;

// 4. 查看查询统计
let stats = pg.stats();
println!("总查询数: {}, 最大耗时: {:?}", stats.total_queries, stats.max_duration);
```

> **不使用 PgClient 时：** 也可以直接使用 `query()` / `sql()` 配合 `tokio_postgres::Client` 或连接池。参见下方 [SQL 模式](#sql-模式)。

---

## Model 模式

定义模型并支持关联关系和预加载：

```rust
use pgorm::prelude::*;

#[derive(Debug, Clone, FromRow, Model)]
#[orm(table = "users")]
#[orm(has_many(Post, foreign_key = "user_id", as = "posts"))]
struct User {
    #[orm(id)]
    id: i64,
    name: String,
}

#[derive(Debug, Clone, FromRow, Model)]
#[orm(table = "posts")]
#[orm(belongs_to(User, foreign_key = "user_id", as = "author"))]
struct Post {
    #[orm(id)]
    id: i64,
    user_id: i64,
    title: String,
}

// 查询所有用户及其文章（批量预加载）
let users = User::select_all(&client).await?;
let posts_map = User::load_posts_map(&client, &users).await?;

for user in &users {
    let posts = posts_map.get(user.pk()).unwrap_or(&vec![]);
    println!("{} 有 {} 篇文章", user.name, posts.len());
}

// 查询文章及其作者
let posts = Post::select_all(&client).await?;
let posts_with_author = Post::load_author(&client, posts).await?;
```

### 批量插入

使用 UNNEST 高效批量插入：

```rust
use pgorm::InsertModel;

#[derive(InsertModel)]
#[orm(table = "products", returning = "Product")]
struct NewProduct {
    sku: String,
    name: String,
    price_cents: i64,
}

let products = vec![
    NewProduct { sku: "SKU-001".into(), name: "键盘".into(), price_cents: 7999 },
    NewProduct { sku: "SKU-002".into(), name: "鼠标".into(), price_cents: 2999 },
    NewProduct { sku: "SKU-003".into(), name: "显示器".into(), price_cents: 19999 },
];

// 批量插入并返回结果
let inserted = NewProduct::insert_many_returning(&client, products).await?;
```

### Update Model（补丁模式）

基于 `Option<T>` 语义的局部更新：

```rust
use pgorm::UpdateModel;

#[derive(UpdateModel)]
#[orm(table = "products", model = "Product", returning = "Product")]
struct ProductPatch {
    name: Option<String>,              // None = 跳过, Some(v) = 更新
    description: Option<Option<String>>, // Some(None) = 设为 NULL
    price_cents: Option<i64>,
}

let patch = ProductPatch {
    name: Some("新名称".into()),
    description: Some(None),  // 设为 NULL
    price_cents: None,        // 保持不变
};

// 更新单行
patch.update_by_id(&client, 1_i64).await?;

// 批量更新
patch.update_by_ids(&client, vec![1, 2, 3]).await?;

// 更新并返回结果
let updated = patch.update_by_id_returning(&client, 1_i64).await?;
```

### Upsert（ON CONFLICT）

```rust
#[derive(InsertModel)]
#[orm(
    table = "tags",
    returning = "Tag",
    conflict_target = "name",
    conflict_update = "color"
)]
struct TagUpsert {
    name: String,
    color: Option<String>,
}

// 单条 upsert
let tag = TagUpsert { name: "rust".into(), color: Some("orange".into()) }
    .upsert_returning(&client)
    .await?;

// 批量 upsert
let tags = TagUpsert::upsert_many_returning(&client, vec![...]).await?;
```

### 乐观锁

通过 `#[orm(version)]` 防止丢失更新：

```rust
#[derive(UpdateModel)]
#[orm(table = "articles", model = "Article", returning = "Article")]
struct ArticlePatch {
    title: Option<String>,
    body: Option<String>,
    #[orm(version)]        // WHERE 自动检查版本，SET 自动递增
    version: i32,
}

let patch = ArticlePatch {
    title: Some("更新标题".into()),
    body: None,
    version: article.version,  // 传入当前版本
};

match patch.update_by_id_returning(&client, article.id).await {
    Ok(updated) => println!("更新到版本 {}", updated.version),
    Err(OrmError::StaleRecord) => println!("冲突！其他人修改了这条记录"),
    Err(e) => return Err(e),
}
```

### 多表写入图

在一个事务中插入多表关联记录：

```rust
#[derive(InsertModel)]
#[orm(table = "products", returning = "Product")]
#[orm(graph_root_id_field = "id")]
#[orm(belongs_to(NewCategory, field = "category", set_fk_field = "category_id", mode = "insert_returning"))]
#[orm(has_one(NewProductDetail, field = "detail", fk_field = "product_id", mode = "insert"))]
#[orm(has_many(NewProductTag, field = "tags", fk_field = "product_id", mode = "insert"))]
struct NewProductGraph {
    id: uuid::Uuid,
    name: String,
    category_id: Option<i64>,

    // 图字段（自动插入到关联表）
    category: Option<NewCategory>,
    detail: Option<NewProductDetail>,
    tags: Option<Vec<NewProductTag>>,
}

let report = NewProductGraph {
    id: uuid::Uuid::new_v4(),
    name: "产品".into(),
    category_id: None,
    category: Some(NewCategory { name: "电子产品".into() }),
    detail: Some(NewProductDetail { product_id: None, description: "...".into() }),
    tags: Some(vec![
        NewProductTag { product_id: None, tag: "新品".into() },
        NewProductTag { product_id: None, tag: "促销".into() },
    ]),
}.insert_graph_report(&client).await?;
```

### PostgreSQL 特殊类型

```rust
// 枚举类型
use pgorm::PgEnum;

#[derive(PgEnum, Debug, Clone, PartialEq)]
#[orm(pg_type = "order_status")]
pub enum OrderStatus {
    #[orm(rename = "pending")]
    Pending,
    #[orm(rename = "shipped")]
    Shipped,
    #[orm(rename = "delivered")]
    Delivered,
}

// 复合类型
use pgorm::PgComposite;

#[derive(PgComposite, Debug, Clone)]
#[orm(pg_type = "address")]
pub struct Address {
    pub street: String,
    pub city: String,
    pub zip_code: String,
}

// 范围类型
use pgorm::types::Range;

Range::<i32>::inclusive(1, 10);   // [1, 10]
Range::<i32>::exclusive(1, 10);  // (1, 10)
Range::<i32>::lower_inc(1, 10);  // [1, 10)
Range::<i32>::empty();           // empty
Range::<i32>::unbounded();       // (-inf, +inf)

// 范围条件运算符
Condition::overlaps("during", range)?;      // &&
Condition::contains("during", timestamp)?;  // @>
Condition::range_left_of("r", range)?;      // <<
Condition::range_right_of("r", range)?;     // >>
Condition::range_adjacent("r", range)?;     // -|-
```

---

## SQL 模式

使用类型安全的条件构建器组装复杂查询：

```rust
use pgorm::prelude::*;

// 动态 WHERE 条件
let mut where_expr = WhereExpr::and(vec![
    Condition::eq("status", "active")?.into(),
    Condition::ilike("name", "%test%")?.into(),
    WhereExpr::or(vec![
        Condition::eq("role", "admin")?.into(),
        Condition::eq("role", "owner")?.into(),
    ]),
    Condition::new("id", Op::between(1_i64, 100_i64))?.into(),
]);

let mut q = sql("SELECT * FROM users");
if !where_expr.is_trivially_true() {
    q.push(" WHERE ");
    where_expr.append_to_sql(&mut q);
}

// 安全的动态 ORDER BY + 分页
OrderBy::new().desc("created_at")?.append_to_sql(&mut q);
Pagination::page(1, 20)?.append_to_sql(&mut q);

let users: Vec<User> = q.fetch_all_as(&client).await?;
```

### 批量更新与删除

```rust
use pgorm::prelude::*;

// 条件批量更新
let affected = sql("users")
    .update_many([
        SetExpr::set("status", "inactive")?,
        SetExpr::raw("updated_at = NOW()"),
    ])
    .filter(Condition::lt("last_login", one_year_ago)?)
    .execute(&client)
    .await?;

// 批量删除
let deleted = sql("sessions")
    .delete_many()
    .filter(Condition::lt("expires_at", now)?)
    .execute(&client)
    .await?;
```

### CTE (WITH) 查询

```rust
// 简单 CTE
let mut cte = sql("SELECT id, name FROM users WHERE status = ");
cte.push_bind("active");
let results = sql("")
    .with("active_users", cte)?
    .select(sql("SELECT * FROM active_users"))
    .fetch_all_as::<User>(&client)
    .await?;

// 递归 CTE（如组织架构树）
let tree = sql("")
    .with_recursive(
        "org_tree",
        sql("SELECT id, name, parent_id, 0 as level FROM employees WHERE parent_id IS NULL"),
        sql("SELECT e.id, e.name, e.parent_id, t.level + 1 FROM employees e JOIN org_tree t ON e.parent_id = t.id"),
    )?
    .select(sql("SELECT * FROM org_tree ORDER BY level"))
    .fetch_all_as::<OrgNode>(&client)
    .await?;
```

### 游标分页

```rust
use pgorm::prelude::*;

let mut where_expr = WhereExpr::and(Vec::new());

// 稳定排序：created_at DESC, id DESC（平局打破）
let mut keyset = Keyset2::desc("created_at", "id")?.limit(20);

// 后续页面，传入最后一行的游标值
if let (Some(last_ts), Some(last_id)) = (after_created_at, after_id) {
    keyset = keyset.after(last_ts, last_id);
    where_expr = where_expr.and_with(keyset.into_where_expr()?);
}

let mut q = sql("SELECT * FROM users");
if !where_expr.is_trivially_true() {
    q.push(" WHERE ");
    where_expr.append_to_sql(&mut q);
}
keyset.append_order_by_limit_to_sql(&mut q)?;
```

### 流式查询

逐行处理大结果集，无需全部加载到内存：

```rust
use futures_util::StreamExt;

let mut stream = query("SELECT * FROM large_table")
    .stream_as::<MyRow>(&client)
    .await?;

while let Some(row) = stream.next().await {
    let row = row?;
    // 逐行处理
}
```

### 事务与保存点

```rust
use pgorm::prelude::*;

// 顶级事务
pgorm::transaction!(&mut client, tx, {
    query("UPDATE accounts SET balance = balance - $1 WHERE id = $2")
        .bind(100_i64).bind(1_i64).execute(&tx).await?;

    // 命名保存点（手动控制）
    let sp = tx.pgorm_savepoint("bonus").await?;
    query("UPDATE accounts SET balance = balance + $1 WHERE id = $2")
        .bind(100_i64).bind(2_i64).execute(&sp).await?;
    sp.release().await?;  // 或 sp.rollback().await?

    Ok::<(), OrmError>(())
})?;

// savepoint! 宏 — Ok 时自动 release，Err 时自动 rollback
pgorm::transaction!(&mut client, tx, {
    let result: Result<(), OrmError> = pgorm::savepoint!(tx, "bonus", sp, {
        query("UPDATE ...").execute(&sp).await?;
        Ok(())
    });
    Ok::<(), OrmError>(())
})?;

// nested_transaction! — 匿名保存点，用于嵌套
pgorm::transaction!(&mut client, tx, {
    pgorm::nested_transaction!(tx, inner, {
        query("UPDATE ...").execute(&inner).await?;
        Ok::<(), OrmError>(())
    })?;
    Ok::<(), OrmError>(())
})?;
```

---

## 监控与检查

### 查询监控

通过内置的指标统计、日志记录和自定义 Hook 监控查询性能：

```rust
use pgorm::monitor::{
    CompositeMonitor, HookAction, InstrumentedClient, LoggingMonitor,
    MonitorConfig, QueryContext, QueryHook, QueryType, StatsMonitor,
};
use pgorm::{query, OrmError, OrmResult};
use std::sync::Arc;
use std::time::Duration;

// 自定义 Hook：阻止不带 WHERE 的 DELETE
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

let stats = Arc::new(StatsMonitor::new());
let monitor = CompositeMonitor::new()
    .add(LoggingMonitor::new()
        .prefix("[pgorm]")
        .min_duration(Duration::from_millis(10)))
    .add_arc(stats.clone());

let config = MonitorConfig::new()
    .with_query_timeout(Duration::from_secs(30))
    .with_slow_query_threshold(Duration::from_millis(100))
    .enable_monitoring();

let pg = InstrumentedClient::new(client)
    .with_config(config)
    .with_monitor(monitor)
    .with_hook(BlockDangerousDeleteHook);

// 所有查询自动被监控
let count: i64 = query("SELECT COUNT(*) FROM users")
    .tag("users.count")
    .fetch_scalar_one(&pg)
    .await?;
```

### 输入验证

通过 `#[orm(input)]` 自动生成带验证的 Input 结构体：

```rust
use pgorm::{FromRow, InsertModel, Model, UpdateModel};

#[derive(Debug, InsertModel)]
#[orm(table = "users", returning = "User")]
#[orm(input)]  // 自动生成 NewUserInput 结构体
struct NewUser {
    #[orm(len = "2..=100")]        // 字符串长度验证
    name: String,

    #[orm(email)]                   // 邮箱格式验证
    email: String,

    #[orm(range = "0..=150")]       // 数值范围验证
    age: Option<i32>,

    #[orm(uuid, input_as = "String")]  // 接收 String，验证并解析为 UUID
    external_id: uuid::Uuid,

    #[orm(url)]                     // URL 格式验证
    homepage: Option<String>,
}

// 从不可信输入反序列化（如 JSON API 请求）
let input: NewUserInput = serde_json::from_str(json_body)?;

// 一次性验证所有字段
let errors = input.validate();
if !errors.is_empty() {
    return Err(serde_json::to_string(&errors)?);
}

// 转换为模型（验证 + 转换 input_as 类型）
let new_user: NewUser = input.try_into_model()?;
let user: User = new_user.insert_returning(&client).await?;
```

**验证属性：**

| 属性 | 说明 |
|------|------|
| `#[orm(len = "min..=max")]` | 字符串长度验证 |
| `#[orm(range = "min..=max")]` | 数值范围验证 |
| `#[orm(email)]` | 邮箱格式验证 |
| `#[orm(url)]` | URL 格式验证 |
| `#[orm(uuid)]` | UUID 格式验证 |
| `#[orm(regex = "pattern")]` | 自定义正则匹配 |
| `#[orm(one_of = "a\|b\|c")]` | 值必须为列举选项之一 |
| `#[orm(custom = "path::to::fn")]` | 自定义验证函数 |
| `#[orm(input_as = "Type")]` | Input 结构体中使用不同类型接收 |

---

## 安全边界

pgorm 提供多层安全防护，用于处理动态和 AI 生成的 SQL：

### 动态标识符（`Ident`）

传递给 `Condition`、`OrderBy`、`SetExpr` 等的列名和表名通过 `Ident` 验证。仅接受 `[a-zA-Z0-9_]` 和点号限定名（`schema.table.column`）— **无法通过标识符进行 SQL 注入**。

```rust
// 安全 — 标识符经过验证
Condition::eq("user_name", value)?;      // OK
OrderBy::new().asc("created_at")?;       // OK

// 返回 Err（非法字符被拒绝）
Condition::eq("name; DROP TABLE", v);    // Err
OrderBy::new().asc("col -- comment");    // Err
```

### 原始 SQL

`query("...")`、`sql("...")`、`SetExpr::raw("...")` 接受**原始 SQL 字符串**，直接传递给 PostgreSQL — 始终使用 `$1` 参数占位符，不要使用字符串拼接。

```rust
// 安全：参数化
query("SELECT * FROM users WHERE id = $1").bind(user_id);

// 不安全：字符串拼接 — 不要这样做
query(&format!("SELECT * FROM users WHERE id = {user_id}"));
```

### SQL 策略（PgClient）

`PgClient` 可以强制执行运行时策略，拦截危险模式：

| 策略 | 选项 | 默认值 |
|------|------|--------|
| `select_without_limit` | `Allow`、`Warn`、`Error`、`AutoLimit(n)` | `Allow` |
| `delete_without_where` | `Allow`、`Warn`、`Error` | `Allow` |
| `update_without_where` | `Allow`、`Warn`、`Error` | `Allow` |
| `truncate` | `Allow`、`Warn`、`Error` | `Allow` |
| `drop_table` | `Allow`、`Warn`、`Error` | `Allow` |

```rust
let pg = PgClient::with_config(&client, PgClientConfig::new()
    .strict()
    .delete_without_where(DangerousDmlPolicy::Error)
    .update_without_where(DangerousDmlPolicy::Warn)
    .select_without_limit(SelectWithoutLimitPolicy::AutoLimit(1000)));
```

### SQL 模式检查

启用 `check` feature（默认开启）后，`PgClient` 会在运行时根据注册的 `#[derive(Model)]` 模式验证 SQL。三种模式：

- **`Disabled`** — 不检查
- **`WarnOnly`**（默认） — 对未知表/列记录警告但执行查询
- **`Strict`** — 对未知表/列返回错误，不执行查询

---

## Feature Flags

| Flag | 默认 | 依赖 | 用途 | 推荐 |
|------|------|------|------|------|
| `pool` | 是 | `deadpool-postgres` | 连接池 | 服务端推荐 |
| `derive` | 是 | `pgorm-derive`（过程宏） | `FromRow`、`Model`、`InsertModel` 等 | 是 |
| `check` | 是 | `pgorm-check` + `libpg_query` | SQL 模式检查、lint、`PgClient` | 开发/预发布推荐 |
| `validate` | 是 | `regex`、`url` | 输入验证（邮箱/URL/正则） | 接收用户输入时推荐 |
| `migrate` | 否 | `refinery` | SQL 迁移 | 仅迁移工具需要 |
| `tracing` | 否 | `tracing` | 通过 `tracing` 输出 SQL（target: `pgorm.sql`） | 使用 tracing 时推荐 |
| `rust_decimal` | 否 | `rust_decimal` | `Decimal` 类型支持 | 按需 |
| `time` | 否 | `time` | `time` crate 日期时间支持 | 按需 |
| `cidr` | 否 | `cidr` | 网络类型支持 | 按需 |
| `geo_types` | 否 | `geo-types` | 几何类型支持 | 按需 |
| `eui48` | 否 | `eui48` | MAC 地址支持 | 按需 |
| `bit_vec` | 否 | `bit-vec` | 位向量支持 | 按需 |
| `extra_types` | 否 | 以上全部 | 启用所有可选类型支持 | 便捷别名 |

## 示例

`crates/pgorm/examples/` 目录包含每个功能的可运行示例：

| 示例 | 说明 |
|------|------|
| `pg_client` | PgClient：SQL 检查和语句缓存 |
| `eager_loading` | 预加载关联关系（has_many、belongs_to） |
| `insert_many` | UNNEST 批量插入 |
| `insert_many_array` | 数组列的批量插入 |
| `upsert` | ON CONFLICT upsert（单条和批量） |
| `update_model` | Option 语义的局部更新 |
| `write_graph` | 多表写入图 |
| `sql_builder` | 动态 SQL：条件、排序、分页 |
| `bulk_operations` | 条件批量更新/删除 |
| `changeset` | Changeset 验证 |
| `monitoring` | 查询监控、日志、Hook |
| `statement_cache` | LRU 预编译语句缓存 |
| `jsonb` | JSONB 列支持 |
| `fetch_semantics` | fetch_one / fetch_optional / fetch_all / fetch_scalar |
| `query_params` | QueryParams 派生宏 |
| `streaming` | 逐行流式查询 |
| `keyset_pagination` | 游标分页 |
| `cte_queries` | CTE (WITH) 查询（含递归） |
| `optimistic_locking` | 乐观锁（版本列） |
| `pg_enum` | PostgreSQL ENUM 派生宏 |
| `pg_range` | 范围类型（tstzrange、daterange、int4range） |
| `pg_composite` | PostgreSQL 复合类型派生宏 |
| `savepoint` | 保存点和嵌套事务 |
| `migrate` | SQL 迁移（refinery） |

运行示例：

```bash
# 大部分示例需要 PostgreSQL 连接
DATABASE_URL=postgres://postgres:postgres@localhost/pgorm_example \
  cargo run --example <name> -p pgorm

# 部分示例无需数据库也能展示 SQL 生成
cargo run --example sql_builder -p pgorm
```

## 文档

详细用法请参阅[完整文档](https://docs.rs/pgorm)。

## 致谢

pgorm 基于以下优秀的 crate 构建：

- [tokio-postgres](https://github.com/sfackler/rust-postgres) - Rust 异步 PostgreSQL 客户端
- [deadpool-postgres](https://github.com/bikeshedder/deadpool) - 简洁的异步 PostgreSQL 连接池
- [refinery](https://github.com/rust-db/refinery) - 强大的 SQL 迁移工具
- [pg_query](https://github.com/pganalyze/pg_query) - 基于 libpg_query 的 PostgreSQL 查询解析器

## 许可证

MIT
