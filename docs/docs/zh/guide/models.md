# 模型与派生宏

pgorm 提供了多个派生宏来处理数据库模型。

## FromRow

`FromRow` 派生宏将数据库行映射到 Rust 结构体：

```rust
use pgorm::FromRow;

#[derive(FromRow)]
struct User {
    id: i64,
    username: String,
    email: Option<String>,
}
```

## Model

`Model` 派生宏提供 CRUD 操作和关系辅助方法：

```rust
use pgorm::{FromRow, Model};

#[derive(Debug, Clone, FromRow, Model)]
#[orm(table = "users")]
struct User {
    #[orm(id)]
    id: i64,
    name: String,
    email: String,
}
```

### 表名

使用 `#[orm(table = "table_name")]` 指定数据库表名。

### 主键

使用 `#[orm(id)]` 标记主键字段。

## Query Builder（`Model::query()`）

`Model` 还会生成一个轻量的 Query Builder：`<Model>Query` 和 `Model::query()`。

```rust
// 类型安全的列名常量：
// - UserQuery::COL_ID（永远可用）
// - UserQuery::id（仅当不与方法名冲突时生成）
let users = User::query()
    .eq(UserQuery::COL_ID, 1_i64)?
    .find(&client)
    .await?;
```

### 可选条件（`*_opt` / `apply_if_*`）

当你的输入是 `Option<T>` / `Result<T, E>` 时，可以用这些 helper 避免大量 `if let Some(...)` 样板代码。

```rust
let q = User::query()
    .eq_opt(UserQuery::COL_ID, user_id)?
    .eq_opt(UserQuery::COL_EMAIL, email)?
    .apply_if_ok(ip_str.parse::<std::net::IpAddr>(), |q, ip| q.eq("ip_address", ip))?;
```

另外也提供了一些常用的“更少样板”helper：

- `eq_opt_str`：`Option<&str>` / `Option<String>` 直接用于等值过滤（自动转成 owned `String`）
- `eq_opt_map`：`Option<T>` 先做一次转换（如 `parse()`），成功才追加过滤
- `range_opt`：把 `gte_opt + lte_opt` 合并成一次调用（常见的时间范围）

```rust
let q = AuditLog::query()
    .eq_opt(AuditLogQuery::COL_USER_ID, user_id)?
    .eq_opt_str(AuditLogQuery::COL_OPERATION_TYPE, operation_type)?
    .eq_opt_str(AuditLogQuery::COL_RESOURCE_TYPE, resource_type)?
    .range_opt(AuditLogQuery::COL_CREATED_AT, start_date, end_date)?
    .eq_opt_map(AuditLogQuery::COL_IP_ADDRESS, ip_address, |s| {
        s.parse::<std::net::IpAddr>().ok()
    })?;
```

## 关系

### has_many

定义一对多关系：

```rust
#[derive(Debug, Clone, FromRow, Model)]
#[orm(table = "users")]
#[orm(has_many(Post, foreign_key = "user_id", as = "posts"))]
struct User {
    #[orm(id)]
    id: i64,
    name: String,
}
```

### belongs_to

定义多对一关系：

```rust
#[derive(Debug, Clone, FromRow, Model)]
#[orm(table = "posts")]
#[orm(belongs_to(User, foreign_key = "user_id", as = "author"))]
struct Post {
    #[orm(id)]
    id: i64,
    user_id: i64,
    title: String,
}
```

## JSONB 支持

pgorm 支持 PostgreSQL JSONB 列：

```rust
use pgorm::{FromRow, Json};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct Meta {
    tags: Vec<String>,
}

#[derive(FromRow)]
struct User {
    id: i64,
    meta: Json<Meta>, // jsonb 列
}
```

## INET（IP 地址）支持

如果表里用 PostgreSQL `inet` 存 IP 地址，推荐直接映射成 `std::net::IpAddr`（可空用 `Option<IpAddr>`），这样查询/写入都不需要 `::text`。

```rust
use pgorm::FromRow;

#[derive(Debug, FromRow)]
struct AuditLog {
    id: i64,
    ip_address: Option<std::net::IpAddr>, // PG: inet
}
```

查询时把入参先 `parse()` 成 `IpAddr` 再 `bind()`：

```rust
use pgorm::query;
use std::net::IpAddr;

let ip: IpAddr = "1.2.3.4".parse()?;
let rows: Vec<AuditLog> = query("SELECT id, ip_address FROM audit_logs WHERE ip_address = $1")
    .bind(ip)
    .fetch_all_as(&client)
    .await?;
```

如果你的 API 输入是 `String/Option<String>`，推荐配合 `#[orm(input)]` 使用 `#[orm(ip, input_as = "String")]`，统一校验与错误返回：[`输入校验与 Input`](/zh/guide/validation-and-input)。

## 下一步

- 下一章：[`关系声明：has_many / belongs_to`](/zh/guide/relations)
