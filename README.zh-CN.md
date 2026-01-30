<p align="center">
  <img src="docs/docs/public/rspress-icon.png" alt="pgorm logo" width="200">
</p>

<h1 align="center">pgorm</h1>

<p align="center">
  <strong>一个轻量级、SQL 优先的 Rust PostgreSQL ORM</strong>
</p>

<p align="center">
  <a href="https://crates.io/crates/pgorm"><img src="https://img.shields.io/crates/v/pgorm.svg" alt="crates.io"></a>
  <a href="https://docs.rs/pgorm"><img src="https://docs.rs/pgorm/badge.svg" alt="docs.rs"></a>
  <img src="https://img.shields.io/badge/rust-1.88%2B-orange.svg" alt="MSRV">
  <img src="https://img.shields.io/crates/l/pgorm.svg" alt="license">
</p>

> **注意：** 本项目正在快速迭代中，API 在版本之间可能会发生变化。

---

## 功能特性

- SQL 优先设计，查询语句显式编写
- 派生宏：`FromRow`、`Model`、`InsertModel`、`UpdateModel`、`ViewModel`
- 基于 `deadpool-postgres` 的连接池
- 关联关系预加载（`has_many`、`belongs_to`）
- 开箱即用的 JSONB 支持
- 基于 `refinery` 的 SQL 迁移
- AI 生成查询的运行时 SQL 检查
- 查询监控：指标统计、Hook 拦截、慢查询检测
- 输入验证宏，自动生成 Input 结构体

## 安装

```toml
[dependencies]
pgorm = "0.1.1"
```

## 快速上手

### Model 模式

定义模型并支持关联关系和预加载：

```rust
use pgorm::{FromRow, Model, ModelPk as _};

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

### SQL 模式

使用类型安全的条件构建器组装复杂查询：

```rust
use pgorm::{sql, Condition, WhereExpr, Op, OrderBy, Pagination};

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

### 查询监控

通过内置的指标统计、日志记录和自定义 Hook 监控查询性能：

```rust
use pgorm::{
    CompositeMonitor, HookAction, InstrumentedClient, LoggingMonitor,
    MonitorConfig, QueryContext, QueryHook, StatsMonitor, query,
};
use std::sync::Arc;
use std::time::Duration;

// 自定义 Hook：阻止不带 WHERE 的 DELETE
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

// 创建监控器
let stats = Arc::new(StatsMonitor::new());
let monitor = CompositeMonitor::new()
    .add(LoggingMonitor::new()
        .prefix("[pgorm]")
        .min_duration(Duration::from_millis(10)))  // 仅记录 > 10ms 的查询
    .add_arc(stats.clone());

// 配置监控
let config = MonitorConfig::new()
    .with_query_timeout(Duration::from_secs(30))
    .with_slow_query_threshold(Duration::from_millis(100))
    .enable_monitoring();

// 包装客户端
let pg = InstrumentedClient::new(client)
    .with_config(config)
    .with_monitor(monitor)
    .with_hook(BlockDangerousDeleteHook);

// 正常使用，所有查询自动被监控
let count: i64 = query("SELECT COUNT(*) FROM users")
    .tag("users.count")  // 可选标签，用于指标分组
    .fetch_scalar_one(&pg)
    .await?;

// 获取统计指标
let metrics = stats.stats();
println!("总查询数: {}", metrics.total_queries);
println!("失败查询数: {}", metrics.failed_queries);
println!("最大耗时: {:?}", metrics.max_duration);
```

### 输入验证

通过 `#[orm(input)]` 自动生成带验证的 Input 结构体：

```rust
use pgorm::{FromRow, InsertModel, Model, UpdateModel};

#[derive(Debug, FromRow, Model)]
#[orm(table = "users")]
struct User {
    #[orm(id)]
    id: i64,
    name: String,
    email: String,
    age: Option<i32>,
    external_id: uuid::Uuid,
    homepage: Option<String>,
}

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
    // 将验证错误返回为 JSON
    return Err(serde_json::to_string(&errors)?);
}

// 转换为模型（验证 + 转换 input_as 类型）
let new_user: NewUser = input.try_into_model()?;

// 插入数据库
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

**三态语义的更新验证：**

```rust
#[derive(Debug, UpdateModel)]
#[orm(table = "users", model = "User", returning = "User")]
#[orm(input)]  // 自动生成 UserPatchInput 结构体
struct UserPatch {
    #[orm(len = "2..=100")]
    name: Option<String>,              // None = 跳过, Some(v) = 更新

    #[orm(email)]
    email: Option<String>,

    #[orm(url)]
    homepage: Option<Option<String>>,  // None = 跳过, Some(None) = 设为 NULL, Some(Some(v)) = 设值
}

// 从 JSON 补丁（缺失字段自动跳过）
let patch_input: UserPatchInput = serde_json::from_str(r#"{"email": "new@example.com"}"#)?;
let patch = patch_input.try_into_patch()?;

// 仅更新 email 字段
let updated: User = patch.update_by_id_returning(&client, user_id).await?;
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
