# Models & Derive Macros

pgorm provides several derive macros for mapping Rust structs to database tables. This page covers `FromRow`, `Model`, `QueryParams`, and `ViewModel`.

## `#[derive(FromRow)]`

Maps database rows to Rust structs. Each struct field maps to a column with the same name.

```rust
use pgorm::FromRow;

#[derive(FromRow)]
struct User {
    id: i64,
    username: String,
    email: Option<String>,
}
```

### Column Renaming

Use `#[orm(column = "...")]` to map a field to a differently-named column:

```rust
#[derive(FromRow)]
struct User {
    id: i64,
    #[orm(column = "user_name")]
    username: String,
}
```

## `#[derive(Model)]`

Builds on `FromRow` and adds table metadata, generated constants, CRUD methods, and a query builder. Requires `#[orm(table = "...")]` and `#[orm(id)]`.

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

### Generated Constants

`Model` generates constants on the struct and on a companion `<Name>Query` struct:

| Constant | Example Value | Description |
|----------|---------------|-------------|
| `User::TABLE` | `"users"` | Table name |
| `User::ID` | `"id"` | Primary key column name |
| `User::SELECT_LIST` | `"id, name, email"` | Comma-separated column list |
| `UserQuery::COL_ID` | `"id"` | Typed column name for query builder |
| `UserQuery::COL_NAME` | `"name"` | Typed column name for query builder |
| `UserQuery::COL_EMAIL` | `"email"` | Typed column name for query builder |

### CRUD Methods

`Model` generates the following methods:

```rust
// Fetch all rows
let users = User::select_all(&client).await?;

// Fetch one by primary key (returns OrmError::NotFound if missing)
let user = User::select_one(&client, 1_i64).await?;

// Delete by primary key
let affected = User::delete_by_id(&client, 1_i64).await?;

// Delete multiple by primary keys
let affected = User::delete_by_ids(&client, vec![1, 2, 3]).await?;

// Delete with RETURNING
let deleted = User::delete_by_id_returning(&client, 1_i64).await?;
```

## Query Builder: `Model::query()`

`Model` generates a lightweight query builder via `User::query()`. It returns a `UserQuery` instance with chainable filter methods.

```rust
// Find users by a column value
let users = User::query()
    .eq(UserQuery::COL_EMAIL, "admin@example.com")?
    .find(&client)
    .await?;

// Count matching rows
let count = User::query()
    .eq(UserQuery::COL_NAME, "Alice")?
    .count(&client)
    .await?;
```

### Optional Filters (`*_opt`, `apply_if_*`)

When inputs are `Option<T>`, use optional filter helpers to avoid `if let Some(...)` boilerplate. If the value is `None`, the filter is skipped.

```rust
let q = User::query()
    .eq_opt(UserQuery::COL_ID, user_id)?           // Option<i64>
    .eq_opt(UserQuery::COL_EMAIL, email)?           // Option<String>
    .apply_if_ok(ip_str.parse::<std::net::IpAddr>(), |q, ip| {
        q.eq("ip_address", ip)
    })?;
```

Convenience helpers:

- `eq_opt_str` -- accepts `Option<&str>` / `Option<String>` directly (auto-converts to owned `String`)
- `eq_opt_map` -- maps `Option<T>` (e.g., `parse()`), only applies the filter on success
- `range_opt` -- combines `gte_opt` + `lte_opt` into one call (common for time ranges)

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

When you want to reuse the same filter set for both `find` (search) and `count`, define your inputs as a struct and derive `QueryParams`:

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

### Usage

`QueryParams` generates `into_query()` which returns a reusable query builder. Use it for both list and count queries:

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

// Same query builder for both list and count
let rows = q.find(&client).await?;
let total = q.count(&client).await?;
```

### Supported Operators

`QueryParams` supports these field-level attributes:

**Conditions:** `eq`, `ne`, `gt`, `gte`, `lt`, `lte`, `like`, `ilike`, `not_like`, `not_ilike`, `is_null`, `is_not_null`, `in_list`, `not_in`, `between`, `not_between`

**Ordering/Pagination:** `order_by`, `order_by_asc`, `order_by_desc`, `order_by_raw`, `paginate`, `limit`, `offset`, `page`

**Escape hatches:** `map(...)`, `raw`, `and`, `or`

## `#[derive(ViewModel)]`

`ViewModel` is an alias of `Model` intended for read-only view models, optionally including JOINs. Write operations (`InsertModel`, `UpdateModel`) are derived separately.

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

## `RowExt` for Manual Typed Access

When you need to read columns from a raw `tokio_postgres::Row` without a full `FromRow` struct, use the `RowExt` trait:

```rust
use pgorm::{RowExt, query};

let row = query("SELECT id, name FROM users WHERE id = $1")
    .bind(1_i64)
    .fetch_one(&client)
    .await?;

let id: i64 = row.try_get_column("id")?;
let name: String = row.try_get_column("name")?;
```

`RowExt::try_get_column` returns `OrmError::Decode` on type mismatch or missing column, giving you consistent error handling across pgorm.

---

Next: [Relations & Eager Loading](/en/guide/relations)
