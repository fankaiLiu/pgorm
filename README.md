# pgorm

A PostgreSQL ORM library for Rust.

> **Note:** This project is under active development and not yet ready for production use.

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
