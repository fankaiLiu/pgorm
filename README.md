# pgorm

A PostgreSQL ORM library for Rust.

> **Note:** This project is under active development and not yet ready for production use.

## Installation

```toml
# Cargo.toml
pgorm = "0.1.0"
```

If you only want the SQL builder (no pool / derive macros / runtime SQL checking):

```toml
pgorm = { version = "0.1.0", default-features = false }
```

## Rust toolchain

- Edition: 2024
- MSRV: Rust 1.88+

## Feature flags

Default features: `pool`, `derive`, `check`, `validate`.

- `pool`: deadpool-postgres pool helpers (`create_pool`)
- `derive`: proc-macros (`FromRow`, `Model`, `InsertModel`, `UpdateModel`, `ViewModel`)
- `check`: runtime SQL checking + recommended wrappers (`CheckedClient`, `PgClient`)
- `validate`: changeset-style validation helpers (email/url/regex/etc)
- `migrate`: SQL migrations via `refinery`

## Quick start (SQL-first)

`pgorm` is Postgres-only and keeps SQL explicit:

- Use `query()` when you already have a full SQL string with `$1, $2, ...`
- Use `sql()` when you want to compose SQL dynamically without manually tracking `$n`

```rust
use pgorm::{query, sql, FromRow};

#[derive(FromRow)]
struct User {
    id: i64,
    username: String,
}

// Hand-written SQL
let user: User = query("SELECT id, username FROM users WHERE id = $1")
    .bind(1_i64)
    .fetch_one_as(&client)
    .await?;

// Dynamic SQL composition (placeholders are generated automatically)
let mut q = sql("SELECT id, username FROM users WHERE 1=1");
q.push(" AND username ILIKE ").push_bind("%admin%");
let users: Vec<User> = q.fetch_all_as(&client).await?;
```

Note: `fetch_one*` returns the **first** row (it does not error if multiple rows match). If you need
strict uniqueness, use `fetch_one_strict*`.

You can also attach an observability tag (if the underlying client supports it):

```rust
let user: User = query("SELECT id, username FROM users WHERE id = $1")
    .tag("users.by_id")
    .bind(1_i64)
    .fetch_one_as(&client)
    .await?;

let users: Vec<User> = sql("SELECT id, username FROM users WHERE username ILIKE ")
    .tag("users.search")
    .push_bind("%admin%")
    .fetch_all_as(&client)
    .await?;
```

## Eager loading (batch preload)

`#[derive(Model)]` supports explicit eager-loading helpers for relations declared via:

- `#[orm(has_many(Child, foreign_key = "...", as = "..."))]`
- `#[orm(belongs_to(Parent, foreign_key = "...", as = "..."))]`

It never runs extra queries unless you call `load_*`.

```rust,ignore
use pgorm::{FromRow, GenericClient, Model, ModelPk as _};

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

async fn list(conn: &impl GenericClient) -> pgorm::OrmResult<()> {
    let users = User::select_all(conn).await?;

    // Map style (recommended default): one extra query per relation.
    let posts_by_user = User::load_posts_map_with(conn, &users, |q| {
        q.push(" ORDER BY id DESC");
    })
    .await?;

    for u in &users {
        let _posts = posts_by_user.get(u.pk()).map(Vec::as_slice).unwrap_or(&[]);
    }

    // Attach style: keep base order, attach relation payload.
    let posts = Post::select_all(conn).await?;
    let _posts_with_author = Post::load_author(conn, posts).await?;

    // Strict variant: require relation to exist for every base row.
    // let _posts_with_author = Post::load_author_strict(conn, posts).await?;

    Ok(())
}
```

## JSONB

`pgorm` enables `tokio-postgres`'s `with-serde_json-1` feature, so `jsonb` columns work out of the box.

- Dynamic JSON: `serde_json::Value`
- Strongly-typed JSON: `pgorm::Json<T>`

```rust
use pgorm::{FromRow, Json, query};
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

let meta = Json(Meta {
    tags: vec!["admin".to_string()],
});

query("INSERT INTO users (id, meta) VALUES ($1, $2)")
    .bind(1_i64)
    .bind(meta)
    .execute(&client)
    .await?;
```

## Transactions

All query execution in `pgorm` takes `&impl GenericClient`, so the same code works
with a plain client connection or inside a transaction:

```rust
use pgorm::{query, OrmResult};

// Works with `tokio_postgres::Client` and `deadpool_postgres::Client`.
pgorm::transaction!(&mut client, tx, {
    query("UPDATE users SET last_login = NOW() WHERE id = $1")
        .bind(1_i64)
        .execute(&tx)
        .await?;
    Ok(())
})?;
```

## Pooling

`create_pool` is a quick-start helper that uses `NoTls` and a small set of defaults (good for local/dev).
For production, inject TLS and pool settings from your application config:

```rust,ignore
use deadpool_postgres::{ManagerConfig, RecyclingMethod};
use pgorm::create_pool_with_manager_config;
use tokio_postgres::NoTls;

let mgr_cfg = ManagerConfig {
    recycling_method: RecyclingMethod::Fast,
};

let pool = create_pool_with_manager_config(&database_url, NoTls, mgr_cfg, |b| b.max_size(32))?;
```

TLS connectors are passed through as-is:

```rust,ignore
use pgorm::create_pool_with_tls;

let tls = /* e.g. tokio_postgres_rustls::MakeRustlsConnect */;
let pool = create_pool_with_tls(&database_url, tls)?;
```

## Recommended client (monitoring + SQL checking)

If you're generating SQL (especially with AI), wrap your client to get guardrails:

```rust,ignore
use pgorm::{create_pool, PgClient, PgClientConfig};

let pool = create_pool(&database_url)?;
let client = pool.get().await?;
let pg = PgClient::with_config(client, PgClientConfig::new().strict());

// Now all pgorm queries go through checking + monitoring.
let user: User = pgorm::query("SELECT id, username FROM users WHERE id = $1")
    .tag("users.by_id")
    .bind(1_i64)
    .fetch_one_as(&pg)
    .await?;
```

## Migrations (refinery)

Enable the `migrate` feature and embed migrations from your application (or a dedicated migrations crate):

```rust,ignore
use pgorm::{create_pool, migrate};

mod embedded {
    use pgorm::embed_migrations;
    embed_migrations!("./migrations");
}

let pool = create_pool(&database_url)?;
migrate::run_pool(&pool, embedded::migrations::runner()).await?;
```

## Crates

- `pgorm` - Core ORM with connection pooling and query builder
- `pgorm-derive` - Procedural macros (`FromRow`, `Model`)
- `pgorm-check` - SQL parsing/linting + schema checking utilities

## AI usage guide

See `AI_USAGE.md` for copy-paste templates, feature selection, and a derive-macro attribute cheat-sheet.

## Acknowledgements

pgorm is built on top of these excellent crates:

- [tokio-postgres](https://github.com/sfackler/rust-postgres) - Asynchronous PostgreSQL client for Rust
- [deadpool-postgres](https://github.com/bikeshedder/deadpool) - Dead simple async pool for PostgreSQL
- [refinery](https://github.com/rust-db/refinery) - Powerful SQL migration toolkit
- [pg_query](https://github.com/pganalyze/pg_query) - PostgreSQL query parser based on libpg_query
