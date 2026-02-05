<p align="center">
  <img src="docs/docs/public/rspress-icon.png" alt="pgorm logo" width="200">
</p>

<h1 align="center">pgorm</h1>

<p align="center">
  <strong>A lightweight, SQL-first PostgreSQL ORM for Rust</strong>
</p>

<p align="center">
  <a href="https://crates.io/crates/pgorm"><img src="https://img.shields.io/crates/v/pgorm.svg" alt="crates.io"></a>
  <a href="https://docs.rs/pgorm"><img src="https://docs.rs/pgorm/badge.svg" alt="docs.rs"></a>
  <img src="https://img.shields.io/badge/rust-1.88%2B-orange.svg" alt="MSRV">
  <img src="https://img.shields.io/crates/l/pgorm.svg" alt="license">
</p>

> **Note:** This project is under active development and iterating rapidly. APIs may change between versions.

---

## Features

- **SQL-first design** — explicit queries with `query()` / `sql()`, no hidden SQL
- **Derive macros** — `FromRow`, `Model`, `InsertModel`, `UpdateModel`, `ViewModel`, `QueryParams`
- **Connection pooling** via `deadpool-postgres`
- **Eager loading** for relations (`has_many`, `belongs_to`, `has_one`, `many_to_many`)
- **Batch insert / upsert** with UNNEST for maximum throughput
- **Bulk update / delete** with type-safe conditions
- **Multi-table write graphs** — insert related records across tables in one transaction
- **Optimistic locking** with `#[orm(version)]`
- **PostgreSQL special types** — `PgEnum`, `PgComposite`, `Range<T>` with derive macros
- **Transactions & savepoints** — `transaction!`, `savepoint!`, `nested_transaction!` macros
- **CTE (WITH) queries** — including recursive CTEs
- **Keyset (cursor) pagination** — `Keyset1`, `Keyset2` for stable, index-friendly paging
- **Streaming queries** — row-by-row `Stream` for large result sets
- **Prepared statement cache** with LRU eviction
- **Query monitoring** — metrics, logging, hooks, and slow query detection
- **Runtime SQL checking** for AI-generated queries
- **SQL migrations** via `refinery`
- **Input validation** macros with automatic Input struct generation
- **JSONB** support out of the box

## Installation

```toml
[dependencies]
pgorm = "0.1.6"
```

## Quick Start

### Model Mode

Define models with relations and eager loading support:

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

// Fetch all users with their posts (batch preload)
let users = User::select_all(&client).await?;
let posts_map = User::load_posts_map(&client, &users).await?;

for user in &users {
    let posts = posts_map.get(user.pk()).unwrap_or(&vec![]);
    println!("{} has {} posts", user.name, posts.len());
}

// Fetch posts with their authors
let posts = Post::select_all(&client).await?;
let posts_with_author = Post::load_author(&client, posts).await?;
```

### SQL Mode

Build complex queries with type-safe condition helpers:

```rust
use pgorm::{sql, Condition, WhereExpr, Op, OrderBy, Pagination};

// Dynamic WHERE conditions
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

// Safe dynamic ORDER BY + pagination
OrderBy::new().desc("created_at")?.append_to_sql(&mut q);
Pagination::page(1, 20)?.append_to_sql(&mut q);

let users: Vec<User> = q.fetch_all_as(&client).await?;
```

### Batch Insert

Insert multiple rows efficiently with UNNEST:

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
    NewProduct { sku: "SKU-001".into(), name: "Keyboard".into(), price_cents: 7999 },
    NewProduct { sku: "SKU-002".into(), name: "Mouse".into(), price_cents: 2999 },
    NewProduct { sku: "SKU-003".into(), name: "Monitor".into(), price_cents: 19999 },
];

// Bulk insert with RETURNING
let inserted = NewProduct::insert_many_returning(&client, products).await?;
```

### Update Model (Patch Style)

Partial updates with `Option<T>` semantics:

```rust
use pgorm::UpdateModel;

#[derive(UpdateModel)]
#[orm(table = "products", model = "Product", returning = "Product")]
struct ProductPatch {
    name: Option<String>,              // None = skip, Some(v) = update
    description: Option<Option<String>>, // Some(None) = set NULL
    price_cents: Option<i64>,
}

let patch = ProductPatch {
    name: Some("New Name".into()),
    description: Some(None),  // set to NULL
    price_cents: None,        // keep existing
};

// Update single row
patch.update_by_id(&client, 1_i64).await?;

// Update multiple rows
patch.update_by_ids(&client, vec![1, 2, 3]).await?;

// Update with RETURNING
let updated = patch.update_by_id_returning(&client, 1_i64).await?;
```

### Upsert (ON CONFLICT)

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

// Single upsert
let tag = TagUpsert { name: "rust".into(), color: Some("orange".into()) }
    .upsert_returning(&client)
    .await?;

// Batch upsert
let tags = TagUpsert::upsert_many_returning(&client, vec![...]).await?;
```

### Optimistic Locking

Prevent lost updates with `#[orm(version)]`:

```rust
#[derive(UpdateModel)]
#[orm(table = "articles", model = "Article", returning = "Article")]
struct ArticlePatch {
    title: Option<String>,
    body: Option<String>,
    #[orm(version)]        // auto-checked in WHERE, auto-incremented in SET
    version: i32,
}

let patch = ArticlePatch {
    title: Some("Updated Title".into()),
    body: None,
    version: article.version,  // pass current version
};

match patch.update_by_id_returning(&client, article.id).await {
    Ok(updated) => println!("Updated to version {}", updated.version),
    Err(OrmError::StaleRecord) => println!("Conflict! Someone else modified this record"),
    Err(e) => return Err(e),
}
```

### Bulk Update & Delete

```rust
use pgorm::{SetExpr, Condition, sql};

// Bulk update with conditions
let affected = sql("users")
    .update_many([
        SetExpr::set("status", "inactive")?,
        SetExpr::raw("updated_at = NOW()"),
    ])
    .filter(Condition::lt("last_login", one_year_ago)?)
    .execute(&client)
    .await?;

// Bulk delete
let deleted = sql("sessions")
    .delete_many()
    .filter(Condition::lt("expires_at", now)?)
    .execute(&client)
    .await?;
```

### CTE (WITH) Queries

```rust
// Simple CTE
let results = sql("")
    .with("active_users", sql("SELECT id, name FROM users WHERE status = ").push_bind_owned("active"))?
    .select(sql("SELECT * FROM active_users"))
    .fetch_all_as::<User>(&client)
    .await?;

// Recursive CTE (e.g., org chart)
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

### PostgreSQL ENUM Types

```rust
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

// Use directly in queries — automatic ToSql/FromSql
query("INSERT INTO orders (status) VALUES ($1)")
    .bind(OrderStatus::Pending)
    .execute(&client).await?;

let status: OrderStatus = row.try_get_column("status")?;
```

### PostgreSQL Composite Types

```rust
use pgorm::PgComposite;

#[derive(PgComposite, Debug, Clone)]
#[orm(pg_type = "address")]
pub struct Address {
    pub street: String,
    pub city: String,
    pub zip_code: String,
}

// Bind and read composite types directly
query("INSERT INTO contacts (name, home_address) VALUES ($1, $2)")
    .bind("Alice")
    .bind(addr)
    .execute(&client).await?;

let addr: Address = row.try_get_column("home_address")?;
```

### Range Types

```rust
use pgorm::types::Range;
use chrono::{DateTime, Utc, Duration};

let now: DateTime<Utc> = Utc::now();

// Insert a tstzrange
query("INSERT INTO events (name, during) VALUES ($1, $2)")
    .bind("Meeting")
    .bind(Range::lower_inc(now, now + Duration::hours(2)))
    .execute(&client).await?;

// Read it back
let during: Range<DateTime<Utc>> = row.try_get_column("during")?;

// Range constructors
Range::<i32>::inclusive(1, 10);   // [1, 10]
Range::<i32>::exclusive(1, 10);  // (1, 10)
Range::<i32>::lower_inc(1, 10);  // [1, 10)
Range::<i32>::empty();           // empty
Range::<i32>::unbounded();       // (-inf, +inf)

// Range condition operators
Condition::overlaps("during", range)?;      // &&
Condition::contains("during", timestamp)?;  // @>
Condition::range_left_of("r", range)?;      // <<
Condition::range_right_of("r", range)?;     // >>
Condition::range_adjacent("r", range)?;     // -|-
```

### Transactions & Savepoints

```rust
use pgorm::{OrmError, TransactionExt};

// Top-level transaction
pgorm::transaction!(&mut client, tx, {
    query("UPDATE accounts SET balance = balance - $1 WHERE id = $2")
        .bind(100_i64).bind(1_i64).execute(&tx).await?;

    // Named savepoint (manual control)
    let sp = tx.pgorm_savepoint("bonus").await?;
    query("UPDATE accounts SET balance = balance + $1 WHERE id = $2")
        .bind(100_i64).bind(2_i64).execute(&sp).await?;
    sp.release().await?;  // or sp.rollback().await?

    Ok::<(), OrmError>(())
})?;

// savepoint! macro — auto release on Ok, rollback on Err
pgorm::transaction!(&mut client, tx, {
    let result: Result<(), OrmError> = pgorm::savepoint!(tx, "bonus", sp, {
        query("UPDATE ...").execute(&sp).await?;
        Ok(())
    });
    Ok::<(), OrmError>(())
})?;

// nested_transaction! — anonymous savepoint for nesting
pgorm::transaction!(&mut client, tx, {
    pgorm::nested_transaction!(tx, inner, {
        query("UPDATE ...").execute(&inner).await?;
        Ok::<(), OrmError>(())
    })?;
    Ok::<(), OrmError>(())
})?;
```

### Keyset (Cursor) Pagination

```rust
use pgorm::{Keyset2, WhereExpr, Condition, sql};

let mut where_expr = WhereExpr::and(Vec::new());

// Stable order: created_at DESC, id DESC
let mut keyset = Keyset2::desc("created_at", "id")?.limit(20);

// For subsequent pages, pass the last row's values
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

### Streaming Queries

Process large result sets row-by-row without loading everything into memory:

```rust
use futures_util::StreamExt;

let mut stream = query("SELECT * FROM large_table")
    .stream_as::<MyRow>(&client)
    .await?;

while let Some(row) = stream.next().await {
    let row = row?;
    // process each row as it arrives
}
```

### Multi-Table Write Graph

Insert related records across multiple tables in one transaction:

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

    // Graph fields (auto-inserted into related tables)
    category: Option<NewCategory>,
    detail: Option<NewProductDetail>,
    tags: Option<Vec<NewProductTag>>,
}

let report = NewProductGraph {
    id: uuid::Uuid::new_v4(),
    name: "Product".into(),
    category_id: None,
    category: Some(NewCategory { name: "Electronics".into() }),
    detail: Some(NewProductDetail { product_id: None, description: "...".into() }),
    tags: Some(vec![
        NewProductTag { product_id: None, tag: "new".into() },
        NewProductTag { product_id: None, tag: "sale".into() },
    ]),
}.insert_graph_report(&client).await?;
```

### Query Monitoring

Monitor query performance with built-in metrics, logging, and custom hooks:

```rust
use pgorm::{
    CompositeMonitor, HookAction, InstrumentedClient, LoggingMonitor,
    MonitorConfig, QueryContext, QueryHook, StatsMonitor, query,
};
use std::sync::Arc;
use std::time::Duration;

// Custom hook to block dangerous DELETE without WHERE
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

// All queries are now monitored
let count: i64 = query("SELECT COUNT(*) FROM users")
    .tag("users.count")
    .fetch_scalar_one(&pg)
    .await?;
```

### Input Validation

Generate validated Input structs with `#[orm(input)]`:

```rust
use pgorm::{FromRow, InsertModel, Model, UpdateModel};

#[derive(Debug, InsertModel)]
#[orm(table = "users", returning = "User")]
#[orm(input)]  // Generates NewUserInput struct
struct NewUser {
    #[orm(len = "2..=100")]        // String length validation
    name: String,

    #[orm(email)]                   // Email format validation
    email: String,

    #[orm(range = "0..=150")]       // Numeric range validation
    age: Option<i32>,

    #[orm(uuid, input_as = "String")]  // Accept String, validate & parse as UUID
    external_id: uuid::Uuid,

    #[orm(url)]                     // URL format validation
    homepage: Option<String>,
}

// Deserialize from untrusted input (e.g., JSON API request)
let input: NewUserInput = serde_json::from_str(json_body)?;

// Validate all fields at once
let errors = input.validate();
if !errors.is_empty() {
    return Err(serde_json::to_string(&errors)?);
}

// Convert to model (validates + converts input_as types)
let new_user: NewUser = input.try_into_model()?;
let user: User = new_user.insert_returning(&client).await?;
```

**Validation attributes:**

| Attribute | Description |
|-----------|-------------|
| `#[orm(len = "min..=max")]` | String length validation |
| `#[orm(range = "min..=max")]` | Numeric range validation |
| `#[orm(email)]` | Email format validation |
| `#[orm(url)]` | URL format validation |
| `#[orm(uuid)]` | UUID format validation |
| `#[orm(regex = "pattern")]` | Custom regex pattern |
| `#[orm(one_of = "a\|b\|c")]` | Value must be one of listed options |
| `#[orm(custom = "path::to::fn")]` | Custom validator function |
| `#[orm(input_as = "Type")]` | Accept different type in Input struct |

## Feature Flags

| Flag | Default | Description |
|------|---------|-------------|
| `pool` | Yes | Connection pooling via `deadpool-postgres` |
| `derive` | Yes | Derive macros (`FromRow`, `Model`, `InsertModel`, etc.) |
| `check` | Yes | SQL schema checking and linting via `pgorm-check` |
| `validate` | Yes | Changeset-style validation helpers (email/url/regex) |
| `migrate` | No | SQL migrations via `refinery` |
| `tracing` | No | SQL debug logs via `tracing` crate |
| `rust_decimal` | No | `rust_decimal::Decimal` support |
| `time` | No | `time` crate date/time support |
| `cidr` | No | `cidr` network type support |
| `geo_types` | No | `geo-types` geometry support |
| `eui48` | No | MAC address support |
| `bit_vec` | No | Bit vector support |
| `extra_types` | No | Enable all optional type support above |

## Examples

The `crates/pgorm/examples/` directory contains runnable examples for every feature:

| Example | Description |
|---------|-------------|
| `pg_client` | PgClient with SQL checking and statement cache |
| `eager_loading` | Eager loading relations (has_many, belongs_to) |
| `insert_many` | Batch insert with UNNEST |
| `insert_many_array` | Batch insert with array columns |
| `upsert` | ON CONFLICT upsert (single & batch) |
| `update_model` | Partial updates with Option semantics |
| `write_graph` | Multi-table write graph |
| `sql_builder` | Dynamic SQL with conditions, ordering, pagination |
| `changeset` | Changeset validation |
| `monitoring` | Query monitoring, logging, hooks |
| `statement_cache` | Prepared statement cache with LRU eviction |
| `jsonb` | JSONB column support |
| `fetch_semantics` | fetch_one / fetch_optional / fetch_all / fetch_scalar |
| `query_params` | QueryParams derive for dynamic query building |
| `streaming` | Streaming large result sets row-by-row |
| `keyset_pagination` | Cursor-based keyset pagination |
| `optimistic_locking` | Optimistic locking with version column |
| `pg_enum` | PostgreSQL ENUM with PgEnum derive |
| `pg_range` | Range types (tstzrange, daterange, int4range) |
| `pg_composite` | PostgreSQL composite types with PgComposite derive |
| `savepoint` | Savepoints and nested transactions |
| `migrate` | SQL migrations with refinery |

Run any example:

```bash
DATABASE_URL=postgres://postgres:postgres@localhost/pgorm_example \
  cargo run --example <name> -p pgorm
```

## Documentation

See the [full documentation](https://docs.rs/pgorm) for detailed API reference.

## Acknowledgements

pgorm is built on top of these excellent crates:

- [tokio-postgres](https://github.com/sfackler/rust-postgres) - Asynchronous PostgreSQL client for Rust
- [deadpool-postgres](https://github.com/bikeshedder/deadpool) - Dead simple async pool for PostgreSQL
- [refinery](https://github.com/rust-db/refinery) - Powerful SQL migration toolkit
- [pg_query](https://github.com/pganalyze/pg_query) - PostgreSQL query parser based on libpg_query

## License

MIT
