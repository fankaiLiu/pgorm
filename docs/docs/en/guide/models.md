# Models & Derive Macros

pgorm provides several derive macros for working with database models.

## FromRow

The `FromRow` derive macro maps database rows to Rust structs:

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

The `Model` derive macro provides CRUD operations and relation helpers:

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

### Table Name

Use `#[orm(table = "table_name")]` to specify the database table name.

### Primary Key

Mark the primary key field with `#[orm(id)]`.

## Query Builder (`Model::query()`)

`Model` also generates a lightweight query builder: `<Model>Query` and `Model::query()`.

```rust
// Type-safe column names:
// - UserQuery::COL_ID (always available)
// - UserQuery::id (only when it doesn't conflict with method names)
let users = User::query()
    .eq(UserQuery::COL_ID, 1_i64)?
    .find(&client)
    .await?;
```

### Optional filters (`*_opt` / `apply_if_*`)

When your inputs are `Option<T>` / `Result<T, E>`, use these helpers to avoid a lot of `if let Some(...)` boilerplate.

```rust
let q = User::query()
    .eq_opt(UserQuery::COL_ID, user_id)?
    .eq_opt(UserQuery::COL_EMAIL, email)?
    .apply_if_ok(ip_str.parse::<std::net::IpAddr>(), |q, ip| q.eq("ip_address", ip))?;
```

There are also a few frequently used helpers to reduce boilerplate:

- `eq_opt_str`: use `Option<&str>` / `Option<String>` directly (auto converts to owned `String`)
- `eq_opt_map`: map `Option<T>` (e.g. `parse()`), and only apply the filter on success
- `range_opt`: combine `gte_opt + lte_opt` into a single call (common for time ranges)

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

## Relations

### has_many

Define a one-to-many relationship:

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

Define a many-to-one relationship:

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

## JSONB Support

pgorm supports PostgreSQL JSONB columns:

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
    meta: Json<Meta>, // jsonb column
}
```

## INET (IP Address) Support

For PostgreSQL `inet` columns, map them to `std::net::IpAddr` (nullable: `Option<IpAddr>`). This keeps reads/writes type-safe and avoids sprinkling `::text` casts in SQL.

```rust
use pgorm::FromRow;

#[derive(Debug, FromRow)]
struct AuditLog {
    id: i64,
    ip_address: Option<std::net::IpAddr>, // PG: inet
}
```

When filtering, parse the input first and then `bind()`:

```rust
use pgorm::query;
use std::net::IpAddr;

let ip: IpAddr = "1.2.3.4".parse()?;
let rows: Vec<AuditLog> = query("SELECT id, ip_address FROM audit_logs WHERE ip_address = $1")
    .bind(ip)
    .fetch_all_as(&client)
    .await?;
```

If your API input is `String/Option<String>`, consider using `#[orm(input)]` + `#[orm(ip, input_as = "String")]` to validate+parse and return consistent `ValidationErrors`: [`Validation & Input`](/en/guide/validation-and-input).

## Next

- Next: [`Relations: has_many / belongs_to`](/en/guide/relations)
