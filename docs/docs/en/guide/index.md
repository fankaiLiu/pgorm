# Quick Start

pgorm is a PostgreSQL-only ORM library for Rust that keeps SQL explicit.

## Installation

Add pgorm to your `Cargo.toml`:

```toml
[dependencies]
pgorm = "0.1.1"
```

If you only want the SQL builder (no pool / derive macros / runtime SQL checking):

```toml
[dependencies]
pgorm = { version = "0.1.1", default-features = false }
```

## Rust Toolchain

- Edition: 2024
- MSRV: Rust 1.88+

## Feature Flags

Default features: `pool`, `derive`, `check`, `validate`.

| Feature    | Description                                                        |
| ---------- | ------------------------------------------------------------------ |
| `pool`     | deadpool-postgres pool helpers (`create_pool`)                     |
| `derive`   | proc-macros (`FromRow`, `Model`, `InsertModel`, `UpdateModel`, `ViewModel`) |
| `check`    | runtime SQL checking + recommended wrappers (`CheckedClient`, `PgClient`)   |
| `validate` | changeset-style validation helpers (email/url/regex/etc)           |
| `migrate`  | SQL migrations via `refinery`                                      |

## Basic Usage

pgorm provides two main query APIs:

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

## Observability Tags

You can attach an observability tag for tracing:

```rust
let user: User = query("SELECT id, username FROM users WHERE id = $1")
    .tag("users.by_id")
    .bind(1_i64)
    .fetch_one_as(&client)
    .await?;
```

## Suggested Reading Order

If this is your first time using pgorm, this is a good path:

1) Pooling & clients: [`create_pool` / TLS / recommended clients](/en/guide/pooling)  
2) Hand-written SQL: [`query()`](/en/guide/query)  
3) Dynamic SQL: [`sql()`](/en/guide/sql-builder)  
4) Dynamic filters: [`Condition/WhereExpr/OrderBy/Pagination`](/en/guide/conditions)  
5) Fetch semantics: [`fetch_one` vs `fetch_one_strict` vs `fetch_opt`](/en/guide/fetch-semantics)  
6) Row mapping: [`FromRow` / `RowExt` / JSONB / INET](/en/guide/from-row)  
7) Models & derives: [`Model / InsertModel / UpdateModel`](/en/guide/models)  
8) Relations: [`has_many` / `belongs_to`](/en/guide/relations)  
9) Eager loading: [`load_*` helpers (avoid N+1)](/en/guide/eager-loading)  
10) Writes: [`InsertModel`](/en/guide/insert-model) / [`UpdateModel`](/en/guide/update-model) / [`Upsert`](/en/guide/upsert)  
11) Advanced writes: [`Write Graph`](/en/guide/write-graph)  
12) Transactions: [`transaction!` / savepoints](/en/guide/transactions)  
13) Migrations: [`refinery`](/en/guide/migrations)  
14) Monitoring & hooks: [`InstrumentedClient`](/en/guide/monitoring)  
15) Runtime SQL checks: [`PgClient / CheckedClient`](/en/guide/runtime-sql-check)  
16) Input validation: [`#[orm(input)]` changeset style](/en/guide/validation-and-input)  
