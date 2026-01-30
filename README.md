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

- SQL-first design with explicit queries
- Derive macros: `FromRow`, `Model`, `InsertModel`, `UpdateModel`, `ViewModel`
- Connection pooling via `deadpool-postgres`
- Eager loading for relations (`has_many`, `belongs_to`)
- JSONB support out of the box
- SQL migrations via `refinery`
- Runtime SQL checking for AI-generated queries
- Query monitoring with metrics, hooks, and slow query detection
- Input validation macros with automatic Input struct generation

## Installation

```toml
[dependencies]
pgorm = "0.1.1"
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

// Create monitors
let stats = Arc::new(StatsMonitor::new());
let monitor = CompositeMonitor::new()
    .add(LoggingMonitor::new()
        .prefix("[pgorm]")
        .min_duration(Duration::from_millis(10)))  // Log queries > 10ms
    .add_arc(stats.clone());

// Configure monitoring
let config = MonitorConfig::new()
    .with_query_timeout(Duration::from_secs(30))
    .with_slow_query_threshold(Duration::from_millis(100))
    .enable_monitoring();

// Wrap client with instrumentation
let pg = InstrumentedClient::new(client)
    .with_config(config)
    .with_monitor(monitor)
    .with_hook(BlockDangerousDeleteHook);

// If you use `tracing`, enable feature `tracing` and add `TracingSqlHook::new()`
// to emit the actual executed SQL (target: `pgorm.sql`).

// Use normally - all queries are monitored
let count: i64 = query("SELECT COUNT(*) FROM users")
    .tag("users.count")  // Optional tag for metrics grouping
    .fetch_scalar_one(&pg)
    .await?;

// Access collected metrics
let metrics = stats.stats();
println!("Total queries: {}", metrics.total_queries);
println!("Failed queries: {}", metrics.failed_queries);
println!("Max duration: {:?}", metrics.max_duration);
```

### Input Validation

Generate validated Input structs with `#[orm(input)]`:

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
    // Return validation errors as JSON
    return Err(serde_json::to_string(&errors)?);
}

// Convert to model (validates + converts input_as types)
let new_user: NewUser = input.try_into_model()?;

// Insert into database
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

**Update validation with tri-state semantics:**

```rust
#[derive(Debug, UpdateModel)]
#[orm(table = "users", model = "User", returning = "User")]
#[orm(input)]  // Generates UserPatchInput struct
struct UserPatch {
    #[orm(len = "2..=100")]
    name: Option<String>,              // None = skip, Some(v) = update

    #[orm(email)]
    email: Option<String>,

    #[orm(url)]
    homepage: Option<Option<String>>,  // None = skip, Some(None) = NULL, Some(Some(v)) = value
}

// Patch from JSON (missing fields are skipped)
let patch_input: UserPatchInput = serde_json::from_str(r#"{"email": "new@example.com"}"#)?;
let patch = patch_input.try_into_patch()?;

// Update only the email field
let updated: User = patch.update_by_id_returning(&client, user_id).await?;
```

## Documentation

See the [full documentation](https://docs.rs/pgorm) for detailed usage.

## Acknowledgements

pgorm is built on top of these excellent crates:

- [tokio-postgres](https://github.com/sfackler/rust-postgres) - Asynchronous PostgreSQL client for Rust
- [deadpool-postgres](https://github.com/bikeshedder/deadpool) - Dead simple async pool for PostgreSQL
- [refinery](https://github.com/rust-db/refinery) - Powerful SQL migration toolkit
- [pg_query](https://github.com/pganalyze/pg_query) - PostgreSQL query parser based on libpg_query

## License

MIT
