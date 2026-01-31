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

### QueryParams（按参数 struct 自动生成 `apply()`）

当你希望复用同一套过滤条件（比如同时用于 `search` 和 `count`），可以把入参收敛成一个 struct，并用 `#[derive(QueryParams)]` 生成 `apply()/into_query()`：

- 支持：`eq/ne/gt/gte/lt/lte/like/ilike/not_like/not_ilike/is_null/is_not_null/in_list/not_in/between/not_between`，排序分页支持 `order_by/order_by_asc/order_by_desc/order_by_raw/paginate/limit/offset/page`，以及 `map(...)` / `raw` / `and` / `or` 这些 escape hatch

```rust
use pgorm::QueryParams;

#[derive(QueryParams)]
#[orm(model = "AuditLog")]
pub struct AuditLogSearchParams<'a> {
    #[orm(eq(AuditLogQuery::COL_USER_ID))]
    pub user_id: Option<uuid::Uuid>,
    #[orm(eq(AuditLogQuery::COL_OPERATION_TYPE))]
    pub operation_type: Option<&'a str>,
    #[orm(gte(AuditLogQuery::COL_CREATED_AT))]
    pub start_date: Option<chrono::DateTime<chrono::Utc>>,
    #[orm(lte(AuditLogQuery::COL_CREATED_AT))]
    pub end_date: Option<chrono::DateTime<chrono::Utc>>,
    #[orm(eq_map(AuditLogQuery::COL_IP_ADDRESS, parse_ip))]
    pub ip_address: Option<&'a str>,

    // 排序/分页（可选）
    #[orm(order_by_desc)]
    pub order_by_desc: Option<&'a str>,
    #[orm(page(per_page = per_page.unwrap_or(20)))]
    pub page: Option<i64>,
    pub per_page: Option<i64>,
}

fn parse_ip(s: &str) -> Option<std::net::IpAddr> {
    s.parse().ok()
}

let q = AuditLogSearchParams { user_id, operation_type, start_date, end_date, ip_address }
    .into_query()?;
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
