# Quick Start

pgorm is a **model-definition-first, AI-friendly PostgreSQL ORM** for Rust. It generates queries from your model definitions, provides runtime SQL checking for AI-generated queries, and bundles monitoring, connection pooling, and statement caching into a single `PgClient` wrapper.

- **Version:** 0.2.0
- **MSRV:** Rust 1.88+
- **Edition:** 2024

## Installation

```toml
[dependencies]
pgorm = "0.2.0"
```

## Define a Model

Use `#[derive(FromRow, Model)]` to define a model. pgorm generates typed constants, CRUD methods, and a query builder for you.

```rust
use pgorm::prelude::*;

#[derive(Debug, Clone, FromRow, Model)]
#[orm(table = "users")]
struct User {
    #[orm(id)]
    id: i64,
    name: String,
    email: String,
}
```

## Connect

Create a connection pool with `create_pool`, then wrap it with `PgClient` for monitoring, SQL checking, and statement caching.

```rust
use pgorm::{PgClient, PgClientConfig, create_pool};
use std::time::Duration;

let pool = create_pool(&std::env::var("DATABASE_URL")?)?;
let client = pool.get().await?;
let pg = PgClient::with_config(&client, PgClientConfig::new()
    .timeout(Duration::from_secs(30))
    .slow_threshold(Duration::from_secs(1))
    .with_logging());
```

## Query

Use model-generated methods or raw SQL -- both are monitored through `PgClient`.

```rust
// Model-based query
let users = User::select_all(&pg).await?;

// Raw SQL query mapped to the model
let active: Vec<User> = pg.sql_query_as(
    "SELECT * FROM users WHERE status = $1",
    &[&"active"],
).await?;

// Query builder with filters
let admins = User::query()
    .eq(UserQuery::COL_EMAIL, "admin@example.com")?
    .find(&pg)
    .await?;

// Check query statistics
let stats = pg.stats();
println!("total queries: {}, max: {:?}", stats.total_queries, stats.max_duration);
```

## Insert

Define an insert model with `#[derive(InsertModel)]` and use it for single or batch inserts.

```rust
use pgorm::InsertModel;

#[derive(InsertModel)]
#[orm(table = "users", returning = "User")]
struct NewUser {
    name: String,
    email: String,
}

// Single insert with RETURNING
let user = NewUser {
    name: "Alice".into(),
    email: "alice@example.com".into(),
}.insert_returning(&pg).await?;

// Batch insert with UNNEST
let users = NewUser::insert_many_returning(&pg, vec![
    NewUser { name: "Bob".into(), email: "bob@example.com".into() },
    NewUser { name: "Carol".into(), email: "carol@example.com".into() },
]).await?;
```

## What's Next

Follow these guides to learn more:

1. [Installation & Feature Flags](/en/guide/installation) -- feature flags, MSRV, and minimal builds
2. [Connection & Pooling](/en/guide/connection) -- `PgClient`, statement cache, TLS
3. [Models & Derive Macros](/en/guide/models) -- `FromRow`, `Model`, `QueryParams`, `ViewModel`
4. [Relations & Eager Loading](/en/guide/relations) -- `has_many`, `belongs_to`, `has_one`, `many_to_many`
5. [PostgreSQL Types](/en/guide/pg-types) -- `PgEnum`, `PgComposite`, `Range<T>`, JSONB

---

Next: [Installation & Feature Flags](/en/guide/installation)
