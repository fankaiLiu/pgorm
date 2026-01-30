# Connection Pooling

pgorm provides helpers for connection pooling using deadpool-postgres.

## Quick Start

`create_pool` is a quick-start helper that uses `NoTls` and a small set of defaults (good for local/dev):

```rust
use pgorm::create_pool;

let pool = create_pool(&database_url)?;
let client = pool.get().await?;
```

## Custom Pool Configuration

For production, inject TLS and pool settings from your application config:

```rust
use deadpool_postgres::{ManagerConfig, RecyclingMethod};
use pgorm::create_pool_with_manager_config;
use tokio_postgres::NoTls;

let mgr_cfg = ManagerConfig {
    recycling_method: RecyclingMethod::Fast,
};

let pool = create_pool_with_manager_config(
    &database_url,
    NoTls,
    mgr_cfg,
    |b| b.max_size(32)
)?;
```

## TLS Support

TLS connectors are passed through as-is:

```rust
use pgorm::create_pool_with_tls;

let tls = /* e.g. tokio_postgres_rustls::MakeRustlsConnect */;
let pool = create_pool_with_tls(&database_url, tls)?;
```

## Recommended Client (Monitoring + SQL Checking)

If you're generating SQL (especially with AI), wrap your client to get guardrails:

```rust
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

## Migrations

Enable the `migrate` feature and embed migrations from your application:

```rust
use pgorm::{create_pool, migrate};

mod embedded {
    use pgorm::embed_migrations;
    embed_migrations!("./migrations");
}

let pool = create_pool(&database_url)?;
migrate::run_pool(&pool, embedded::migrations::runner()).await?;
```
