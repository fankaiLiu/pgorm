# 模型与派生宏

pgorm 提供了多个派生宏，用于将 Rust 结构体映射到数据库表。本页涵盖 `FromRow`、`Model`、`QueryParams` 和 `ViewModel`。

## `#[derive(FromRow)]`

将数据库行映射到 Rust 结构体。每个结构体字段对应同名的列。

```rust
use pgorm::FromRow;

#[derive(FromRow)]
struct User {
    id: i64,
    username: String,
    email: Option<String>,
}
```

### 列重命名

使用 `#[orm(column = "...")]` 将字段映射到不同名称的列：

```rust
#[derive(FromRow)]
struct User {
    id: i64,
    #[orm(column = "user_name")]
    username: String,
}
```

## `#[derive(Model)]`

基于 `FromRow` 构建，添加表元数据、生成常量、CRUD 方法和查询构建器。需要 `#[orm(table = "...")]` 和 `#[orm(id)]`。

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

### 生成的常量

`Model` 会在结构体本身和一个伴生的 `<Name>Query` 结构体上生成常量：

| 常量 | 示例值 | 描述 |
|------|--------|------|
| `User::TABLE` | `"users"` | 表名 |
| `User::ID` | `"id"` | 主键列名 |
| `User::SELECT_LIST` | `"id, name, email"` | 逗号分隔的列列表 |
| `UserQuery::COL_ID` | `"id"` | 用于查询构建器的类型化列名 |
| `UserQuery::COL_NAME` | `"name"` | 用于查询构建器的类型化列名 |
| `UserQuery::COL_EMAIL` | `"email"` | 用于查询构建器的类型化列名 |

### CRUD 方法

`Model` 生成以下方法：

```rust
// 查询所有行
let users = User::select_all(&client).await?;

// 按主键查询单行（不存在时返回 OrmError::NotFound）
let user = User::select_one(&client, 1_i64).await?;

// 按主键删除
let affected = User::delete_by_id(&client, 1_i64).await?;

// 按多个主键删除
let affected = User::delete_by_ids(&client, vec![1, 2, 3]).await?;

// 删除并返回（RETURNING）
let deleted = User::delete_by_id_returning(&client, 1_i64).await?;
```

## 查询构建器：`Model::query()`

`Model` 通过 `User::query()` 生成一个轻量的查询构建器。它返回一个 `UserQuery` 实例，支持链式调用过滤方法。

```rust
// 按列值查找用户
let users = User::query()
    .eq(UserQuery::COL_EMAIL, "admin@example.com")?
    .find(&client)
    .await?;

// 统计匹配的行数
let count = User::query()
    .eq(UserQuery::COL_NAME, "Alice")?
    .count(&client)
    .await?;
```

### 可选过滤（`*_opt`、`apply_if_*`）

当输入为 `Option<T>` 时，可以使用可选过滤辅助方法来避免 `if let Some(...)` 的样板代码。如果值为 `None`，则跳过该过滤条件。

```rust
let q = User::query()
    .eq_opt(UserQuery::COL_ID, user_id)?           // Option<i64>
    .eq_opt(UserQuery::COL_EMAIL, email)?           // Option<String>
    .apply_if_ok(ip_str.parse::<std::net::IpAddr>(), |q, ip| {
        q.eq("ip_address", ip)
    })?;
```

便捷辅助方法：

- `eq_opt_str` -- 接受 `Option<&str>` / `Option<String>`（自动转换为 owned `String`）
- `eq_opt_map` -- 对 `Option<T>` 进行转换（如 `parse()`），仅在成功时应用过滤
- `range_opt` -- 将 `gte_opt` + `lte_opt` 合并为一次调用（常用于时间范围）

```rust
let q = AuditLog::query()
    .eq_opt(AuditLogQuery::COL_USER_ID, user_id)?
    .eq_opt_str(AuditLogQuery::COL_OPERATION_TYPE, operation_type)?
    .range_opt(AuditLogQuery::COL_CREATED_AT, start_date, end_date)?
    .eq_opt_map(AuditLogQuery::COL_IP_ADDRESS, ip_address, |s| {
        s.parse::<std::net::IpAddr>().ok()
    })?;
```

## `#[derive(QueryParams)]`

当你希望对 `find`（搜索）和 `count` 复用同一套过滤条件时，可以将输入定义为一个结构体并派生 `QueryParams`：

```rust
use pgorm::QueryParams;

fn parse_ip(s: &str) -> Option<std::net::IpAddr> {
    s.parse().ok()
}

#[derive(QueryParams)]
#[orm(model = "AuditLog")]
struct AuditLogSearchParams<'a> {
    #[orm(eq(AuditLogQuery::COL_USER_ID))]
    user_id: Option<uuid::Uuid>,

    #[orm(eq(AuditLogQuery::COL_OPERATION_TYPE))]
    operation_type: Option<&'a str>,

    #[orm(gte(AuditLogQuery::COL_CREATED_AT))]
    start_date: Option<DateTime<Utc>>,

    #[orm(lte(AuditLogQuery::COL_CREATED_AT))]
    end_date: Option<DateTime<Utc>>,

    #[orm(eq(AuditLogQuery::COL_IP_ADDRESS), map(parse_ip))]
    ip_address: Option<&'a str>,

    #[orm(in_list(AuditLogQuery::COL_STATUS_CODE))]
    status_any: Option<Vec<i16>>,

    #[orm(order_by_desc)]
    order_by_desc: Option<&'a str>,

    #[orm(page(per_page = per_page.unwrap_or(10)))]
    page: Option<i64>,

    per_page: Option<i64>,
}
```

### 用法

`QueryParams` 生成 `into_query()` 方法，返回一个可复用的查询构建器。可同时用于列表查询和计数查询：

```rust
let params = AuditLogSearchParams {
    user_id: Some(uuid::Uuid::nil()),
    operation_type: Some("LOGIN"),
    start_date: None,
    end_date: None,
    ip_address: Some("127.0.0.1"),
    status_any: Some(vec![200, 201, 204]),
    order_by_desc: Some("created_at"),
    page: Some(1),
    per_page: Some(10),
};

let q = params.into_query()?;

// 同一个查询构建器同时用于列表和计数
let rows = q.find(&client).await?;
let total = q.count(&client).await?;
```

### 支持的操作符

`QueryParams` 支持以下字段级属性：

**条件：** `eq`、`ne`、`gt`、`gte`、`lt`、`lte`、`like`、`ilike`、`not_like`、`not_ilike`、`is_null`、`is_not_null`、`in_list`、`not_in`、`between`、`not_between`

**排序/分页：** `order_by`、`order_by_asc`、`order_by_desc`、`order_by_raw`、`paginate`、`limit`、`offset`、`page`

**逃逸手段：** `map(...)`、`raw`、`and`、`or`

## `#[derive(ViewModel)]`

`ViewModel` 是 `Model` 的别名，用于只读视图模型，可选包含 JOIN。写操作（`InsertModel`、`UpdateModel`）需要单独派生。

```rust
use pgorm::{FromRow, ViewModel};

#[derive(Debug, Clone, FromRow, ViewModel)]
#[orm(table = "posts")]
struct PostWithAuthor {
    #[orm(id)]
    id: i64,
    title: String,
    #[orm(table = "users", column = "name")]
    author_name: String,
}
```

## 使用 `RowExt` 手动类型化访问

当你需要从原始 `tokio_postgres::Row` 中读取列，而不使用完整的 `FromRow` 结构体时，可以使用 `RowExt` trait：

```rust
use pgorm::{RowExt, query};

let row = query("SELECT id, name FROM users WHERE id = $1")
    .bind(1_i64)
    .fetch_one(&client)
    .await?;

let id: i64 = row.try_get_column("id")?;
let name: String = row.try_get_column("name")?;
```

`RowExt::try_get_column` 在类型不匹配或列缺失时返回 `OrmError::Decode`，为你在 pgorm 中提供一致的错误处理。

---

下一步：[关系与预加载](/zh/guide/relations)
