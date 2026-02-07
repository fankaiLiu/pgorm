<p align="center">
  <img src="docs/docs/public/rspress-icon.png" alt="pgorm logo" width="200">
</p>

<h1 align="center">pgorm</h1>

<p align="center">
  <strong>A model-definition-first, AI-friendly PostgreSQL ORM for Rust</strong>
</p>

<p align="center">
  <a href="https://crates.io/crates/pgorm"><img src="https://img.shields.io/crates/v/pgorm.svg" alt="crates.io"></a>
  <a href="https://docs.rs/pgorm"><img src="https://docs.rs/pgorm/badge.svg" alt="docs.rs"></a>
  <img src="https://img.shields.io/badge/rust-1.88%2B-orange.svg" alt="MSRV">
  <img src="https://img.shields.io/crates/l/pgorm.svg" alt="license">
</p>

> **Note:** This project is pre-1.0 and iterating rapidly. APIs may change between minor versions.
> Deprecated items will have at least one minor version of transition period.
> MSRV: **1.88+** · Follows [semver](https://semver.org/) for 0.x releases.

---

## Features

- **Model-definition-first** — define your models with derive macros, pgorm generates queries for you
- **AI-friendly** — explicit queries with `query()` / `sql()`, runtime SQL checking for AI-generated queries
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
pgorm = "0.3.0"
```

Default features (`pool`, `derive`, `check`, `validate`) cover most use cases.
For a minimal build (SQL builder + row mapping only):

```toml
pgorm = { version = "0.3.0", default-features = false }
```

## Quick Start

**Recommended:** Use `PgClient` — it bundles monitoring, SQL checking, statement cache, and safety policies in one place.

```rust
use pgorm::prelude::*;
use pgorm::{PgClient, PgClientConfig, create_pool};
use std::time::Duration;

// 1. Define your models
#[derive(Debug, Clone, FromRow, Model)]
#[orm(table = "users")]
struct User {
    #[orm(id)]
    id: i64,
    name: String,
    email: String,
}

// 2. Connect via pool + PgClient
let pool = create_pool(&std::env::var("DATABASE_URL")?)?;
let client = pool.get().await?;
let pg = PgClient::with_config(&client, PgClientConfig::new()
    .timeout(Duration::from_secs(30))
    .slow_threshold(Duration::from_secs(1))
    .with_logging());

// 3. Use model-based or SQL-based queries — both are monitored
let users = User::select_all(&pg).await?;

let active: Vec<User> = pg.sql_query_as(
    "SELECT * FROM users WHERE status = $1",
    &[&"active"],
).await?;

// 4. Check query statistics
let stats = pg.stats();
println!("total queries: {}, max: {:?}", stats.total_queries, stats.max_duration);
```

> **Without `PgClient`:** you can also use `query()` / `sql()` directly with a `tokio_postgres::Client` or pool connection. See the [SQL Mode](#sql-mode) section below.

---

## Model Mode

Define models with relations and eager loading support:

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

### PostgreSQL Special Types

```rust
// ENUM types
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

// Composite types
use pgorm::PgComposite;

#[derive(PgComposite, Debug, Clone)]
#[orm(pg_type = "address")]
pub struct Address {
    pub street: String,
    pub city: String,
    pub zip_code: String,
}

// Range types
use pgorm::types::Range;

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

---

## SQL Mode

Build complex queries with type-safe condition helpers:

```rust
use pgorm::prelude::*;

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

### Bulk Update & Delete

```rust
use pgorm::prelude::*;

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
let mut cte = sql("SELECT id, name FROM users WHERE status = ");
cte.push_bind("active");
let results = sql("")
    .with("active_users", cte)?
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

### Keyset (Cursor) Pagination

```rust
use pgorm::prelude::*;

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

### Transactions & Savepoints

```rust
use pgorm::prelude::*;

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

---

## Monitoring & Checking

### Query Monitoring

Monitor query performance with built-in metrics, logging, and custom hooks:

```rust
use pgorm::monitor::{
    CompositeMonitor, HookAction, InstrumentedClient, LoggingMonitor,
    MonitorConfig, QueryContext, QueryHook, QueryType, StatsMonitor,
};
use pgorm::{query, OrmError, OrmResult};
use std::sync::Arc;
use std::time::Duration;

// Custom hook to block dangerous DELETE without WHERE
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

---

## Security Boundaries

pgorm provides several safety layers for handling dynamic and AI-generated SQL:

### Dynamic Identifiers (`Ident`)

Column and table names passed to `Condition`, `OrderBy`, `SetExpr`, etc. are validated through `Ident`. Only `[a-zA-Z0-9_]` and qualified names (`schema.table.column`) are accepted — **SQL injection via identifiers is not possible**.

```rust
// These are safe — identifiers are validated
Condition::eq("user_name", value)?;      // OK
OrderBy::new().asc("created_at")?;       // OK

// These will return Err (invalid characters rejected)
Condition::eq("name; DROP TABLE", v);    // Err
OrderBy::new().asc("col -- comment");    // Err
```

### Raw SQL

Functions like `query("...")`, `sql("...")`, and `SetExpr::raw("...")` accept **raw SQL strings**. These are passed directly to PostgreSQL — always use `$1` parameter placeholders for user input, never string interpolation.

```rust
// SAFE: parameterized
query("SELECT * FROM users WHERE id = $1").bind(user_id);

// UNSAFE: string interpolation — DO NOT do this
query(&format!("SELECT * FROM users WHERE id = {user_id}"));
```

### SQL Policies (PgClient)

`PgClient` can enforce runtime policies to catch dangerous patterns:

| Policy | Options | Default |
|--------|---------|---------|
| `select_without_limit` | `Allow`, `Warn`, `Error`, `AutoLimit(n)` | `Allow` |
| `delete_without_where` | `Allow`, `Warn`, `Error` | `Allow` |
| `update_without_where` | `Allow`, `Warn`, `Error` | `Allow` |
| `truncate` | `Allow`, `Warn`, `Error` | `Allow` |
| `drop_table` | `Allow`, `Warn`, `Error` | `Allow` |

```rust
let pg = PgClient::with_config(&client, PgClientConfig::new()
    .strict()
    .delete_without_where(DangerousDmlPolicy::Error)
    .update_without_where(DangerousDmlPolicy::Warn)
    .select_without_limit(SelectWithoutLimitPolicy::AutoLimit(1000)));
```

### SQL Schema Checking

With `check` feature (default on), `PgClient` validates SQL against registered `#[derive(Model)]` schemas at runtime. Three modes:

- **`Disabled`** — no checking
- **`WarnOnly`** (default) — logs warnings for unknown tables/columns but executes the query
- **`Strict`** — returns an error for unknown tables/columns before executing

---

## Feature Flags

| Flag | Default | Deps | Purpose | Recommended |
|------|---------|------|---------|-------------|
| `pool` | Yes | `deadpool-postgres` | Connection pooling | Yes for servers |
| `derive` | Yes | `pgorm-derive` (proc-macro) | `FromRow`, `Model`, `InsertModel`, etc. | Yes |
| `check` | Yes | `pgorm-check` + `libpg_query` | SQL schema checking, linting, `PgClient` | Yes for dev/staging |
| `validate` | Yes | `regex`, `url` | Input validation (email/url/regex) | Yes if accepting user input |
| `migrate` | No | `refinery` | SQL migrations | Only for migration runner binary |
| `tracing` | No | `tracing` | Emit SQL via `tracing` (target: `pgorm.sql`) | Yes if using tracing |
| `rust_decimal` | No | `rust_decimal` | `Decimal` type support | As needed |
| `time` | No | `time` | `time` crate date/time support | As needed |
| `cidr` | No | `cidr` | Network type support | As needed |
| `geo_types` | No | `geo-types` | Geometry support | As needed |
| `eui48` | No | `eui48` | MAC address support | As needed |
| `bit_vec` | No | `bit-vec` | Bit vector support | As needed |
| `extra_types` | No | all of the above | Enable all optional type support | Convenience alias |

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
| `bulk_operations` | Bulk update/delete with conditions |
| `changeset` | Changeset validation |
| `monitoring` | Query monitoring, logging, hooks |
| `statement_cache` | Prepared statement cache with LRU eviction |
| `jsonb` | JSONB column support |
| `fetch_semantics` | fetch_one / fetch_optional / fetch_all / fetch_scalar |
| `query_params` | QueryParams derive for dynamic query building |
| `streaming` | Streaming large result sets row-by-row |
| `keyset_pagination` | Cursor-based keyset pagination |
| `cte_queries` | CTE (WITH) queries including recursive CTEs |
| `optimistic_locking` | Optimistic locking with version column |
| `pg_enum` | PostgreSQL ENUM with PgEnum derive |
| `pg_range` | Range types (tstzrange, daterange, int4range) |
| `pg_composite` | PostgreSQL composite types with PgComposite derive |
| `composite_primary_key` | Composite primary key models (`select_by_pk`, `delete_by_pk`) |
| `savepoint` | Savepoints and nested transactions |
| `migrate` | SQL migrations with refinery |

Run any example:

```bash
# Most examples need a PostgreSQL connection
DATABASE_URL=postgres://postgres:postgres@localhost/pgorm_example \
  cargo run --example <name> -p pgorm

# Some examples can show SQL generation without a database
cargo run --example sql_builder -p pgorm
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
