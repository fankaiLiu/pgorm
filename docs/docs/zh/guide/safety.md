# SQL 安全与检查

pgorm 提供了多层安全机制来防范 SQL 注入、危险查询和 schema 不匹配 -- 当 SQL 可能被动态生成或由 AI 产生时，这一点尤为重要。

## 1. 动态标识符安全（`Ident`）

传递给 pgorm 构建器类型（`Condition`、`OrderBy`、`SetExpr`、`Pagination`）的列名和表名会通过内部 `Ident` 类型进行验证。只有匹配 `[a-zA-Z0-9_]` 的字符和类似 `schema.table.column` 的限定名才会被接受。

**通过标识符进行 SQL 注入是不可能的。**

```rust
use pgorm::prelude::*;

// 以下是安全的 -- 标识符在构造时即被验证
Condition::eq("user_name", value)?;      // OK
OrderBy::new().asc("created_at")?;       // OK
SetExpr::set("status", "inactive")?;     // OK

// 以下会返回 Err -- 无效字符会被拒绝
Condition::eq("name; DROP TABLE", v);    // Err
OrderBy::new().asc("col -- comment");    // Err
```

这意味着你可以安全地将用户提供的列名传递给这些构建器类型，pgorm 会拒绝任何看起来像 SQL 注入的内容。

## 2. 原始 SQL 安全

`query("...")`、`sql("...")` 和 `SetExpr::raw("...")` 等函数接受直接传递给 PostgreSQL 的原始 SQL 字符串。对于这些情况，始终使用 `$1` 参数占位符传入用户输入。切勿使用字符串插值。

```rust
// 安全：参数化查询
query("SELECT * FROM users WHERE id = $1").bind(user_id);

// 安全：sql() 配合 push_bind（自动生成 $n 占位符）
let mut q = sql("SELECT * FROM users WHERE status = ");
q.push_bind("active");
```

```rust
// 不安全：字符串插值 -- 切勿这样做
query(&format!("SELECT * FROM users WHERE id = {user_id}"));
```

规则很简单：如果值来自用户输入，就必须通过 `.bind()` 或 `.push_bind()` 传递，而不是拼接到 SQL 字符串中。

## 3. SQL 安全策略（`PgClientConfig`）

`PgClient` 可以在运行时执行安全策略，在危险的查询模式到达数据库之前将其拦截。

### 策略表

| 策略 | 选项 | 默认值 |
|------|------|--------|
| `select_without_limit` | `Allow`、`Warn`、`Error`、`AutoLimit(n)` | `Allow` |
| `delete_without_where` | `Allow`、`Warn`、`Error` | `Allow` |
| `update_without_where` | `Allow`、`Warn`、`Error` | `Allow` |
| `truncate` | `Allow`、`Warn`、`Error` | `Allow` |
| `drop_table` | `Allow`、`Warn`、`Error` | `Allow` |

### 配置

```rust
use pgorm::{PgClient, PgClientConfig, DangerousDmlPolicy, SelectWithoutLimitPolicy};

// 单独配置各项策略
let pg = PgClient::with_config(&client, PgClientConfig::new()
    .delete_without_where(DangerousDmlPolicy::Error)
    .update_without_where(DangerousDmlPolicy::Warn)
    .select_without_limit(SelectWithoutLimitPolicy::AutoLimit(1000))
    .truncate_policy(DangerousDmlPolicy::Error)
    .drop_table_policy(DangerousDmlPolicy::Error));
```

### `strict()` 快捷方式

`strict()` 方法一次性启用严格 SQL 检查和合理的安全默认值：

```rust
let pg = PgClient::with_config(&client, PgClientConfig::new().strict());
```

### 策略行为

- **`Allow`** -- 无限制，查询正常执行
- **`Warn`** -- 记录警告但仍执行查询
- **`Error`** -- 在查询到达数据库之前返回 `OrmError::Validation`
- **`AutoLimit(n)`** -- （仅 SELECT）如果查询没有 LIMIT 子句，自动追加 `LIMIT n`

## 4. 运行时 SQL Schema 检查

启用 `check` feature（默认启用）后，`PgClient` 会在运行时根据已注册的 `#[derive(Model)]` schema 验证 SQL 语句。这可以在查询发送到 PostgreSQL 之前捕获对不存在的表或列的引用。

### 检查模式

- **`Disabled`** -- 完全不检查
- **`WarnOnly`**（默认）-- 对未知的表/列记录警告，但仍执行查询
- **`Strict`** -- 对未知的表/列返回错误，阻止执行

### 工作原理

使用 `#[derive(Model)]` 注解的模型会通过 `inventory` crate 自动注册到全局 schema 注册表中。当 `PgClient` 收到查询时，它会解析 SQL 并根据注册表验证表/列的引用。

```rust
use pgorm::{CheckMode, PgClient, PgClientConfig, query, FromRow, Model};

#[derive(Debug, FromRow, Model)]
#[orm(table = "products")]
struct Product {
    #[orm(id)]
    id: i64,
    name: String,
    price_cents: i64,
    in_stock: bool,
}

// 默认：WarnOnly 模式
let pg = PgClient::new(&client);

// 显式选择模式
let pg_strict = PgClient::with_config(&client, PgClientConfig::new().strict());
let pg_warn = PgClient::with_config(&client, PgClientConfig::new().check_mode(CheckMode::WarnOnly));
let pg_off = PgClient::with_config(&client, PgClientConfig::new().no_check());
```

### 严格模式实战

```rust
let pg_strict = PgClient::with_config(&client, PgClientConfig::new().strict());

// 正常工作 -- `id` 和 `name` 存在于 `products` 表中
let rows = query("SELECT id, name FROM products")
    .fetch_all(&pg_strict)
    .await?;

// 在到达数据库之前就失败 -- `email` 不存在于 `products` 表中
let result = query("SELECT id, email FROM products")
    .fetch_all(&pg_strict)
    .await;
// result 为 Err(OrmError::Validation(...))

// 失败 -- 表 `orders` 未注册
let result = query("SELECT id FROM orders")
    .fetch_all(&pg_strict)
    .await;
// result 为 Err(OrmError::Validation(...))
```

### 直接 Schema 验证

你也可以直接对注册表检查 SQL，而无需执行它：

```rust
let pg = PgClient::new(&client);

let issues = pg.registry().check_sql("SELECT id, email FROM products");
for issue in &issues {
    println!("{:?}: {}", issue.kind, issue.message);
}
```

### `CheckedClient` -- 轻量级替代方案

如果你只需要 schema 验证，而不需要监控或安全策略，可以使用 `CheckedClient`：

```rust
use pgorm::{CheckedClient, query};

let checked = CheckedClient::new(&client).strict();

let _ = query("SELECT id, name FROM products")
    .fetch_all(&checked)
    .await?;
```

## 5. `check_models!` 宏

`check_models!` 宏一次性验证模型生成的所有 SQL 是否符合 schema 注册表。这在启动验证或 CI 检查中非常有用：

```rust
use pgorm::{check_models, SchemaRegistry, Model, FromRow};

#[derive(Debug, FromRow, Model)]
#[orm(table = "users")]
struct User {
    #[orm(id)]
    id: i64,
    name: String,
}

#[derive(Debug, FromRow, Model)]
#[orm(table = "orders")]
struct Order {
    #[orm(id)]
    id: i64,
    user_id: i64,
}

let registry = SchemaRegistry::new();
// ... 注册表 ...

let results = check_models!(registry, User, Order);
for (name, issues) in &results {
    if issues.is_empty() {
        println!("  {} OK", name);
    } else {
        println!("  {} has {} issue(s)", name, issues.len());
    }
}
```

还有 `assert_models_valid!` 宏，如果任何模型存在 schema 问题则会 panic -- 适用于启动时的安全检查：

```rust
// 如果任何模型验证失败，将 panic 并输出详细消息
pgorm::assert_models_valid!(registry, User, Order);
```

## 综合使用

在典型的生产环境中，你会将 schema 检查、安全策略、监控和语句缓存结合使用：

```rust
use pgorm::{PgClient, PgClientConfig, CheckMode, DangerousDmlPolicy, SelectWithoutLimitPolicy};
use std::time::Duration;

let pg = PgClient::with_config(&client, PgClientConfig::new()
    .check_mode(CheckMode::WarnOnly)
    .timeout(Duration::from_secs(30))
    .slow_threshold(Duration::from_millis(100))
    .with_stats()
    .log_slow_queries(Duration::from_millis(50))
    .statement_cache(128)
    .delete_without_where(DangerousDmlPolicy::Error)
    .update_without_where(DangerousDmlPolicy::Warn)
    .select_without_limit(SelectWithoutLimitPolicy::AutoLimit(1000)));
```

## 可运行示例

参见 `crates/pgorm/examples/pg_client/main.rs`，包含 schema 注册表、检查模式（Strict/WarnOnly）、查询统计和安全策略的完整示例。

## 下一步

- 下一章：[输入验证](/zh/guide/validation)
