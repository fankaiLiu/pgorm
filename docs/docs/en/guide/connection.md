# Connection & Pooling

pgorm provides connection pooling via `deadpool-postgres` and a recommended `PgClient` wrapper that bundles monitoring, SQL checking, and a prepared statement cache.

## Quick Start with `create_pool`

The simplest way to connect is `create_pool`, which parses a `DATABASE_URL` and returns a pool with sensible defaults (NoTls):

```rust
use pgorm::create_pool;

let pool = create_pool(&std::env::var("DATABASE_URL")?)?;
let client = pool.get().await?;
```

## PgClient (Recommended)

`PgClient` wraps any `GenericClient` (pool connection, raw client, or transaction) and adds:

- **SQL schema checking** -- validates queries against registered `#[derive(Model)]` schemas
- **Query statistics** -- tracks counts, durations, and slow queries
- **Logging** -- optional query logging with configurable thresholds
- **Statement cache** -- LRU cache for prepared statements
- **Safety policies** -- block dangerous DML patterns at runtime

### Basic Usage

```rust
use pgorm::{PgClient, PgClientConfig, create_pool};

let pool = create_pool(&database_url)?;
let client = pool.get().await?;

// Default config: WarnOnly checking, stats enabled, no cache
let pg = PgClient::new(&client);
```

### Configured Usage

```rust
use pgorm::{PgClient, PgClientConfig, CheckMode, create_pool};
use std::time::Duration;

let pool = create_pool(&database_url)?;
let client = pool.get().await?;

let pg = PgClient::with_config(&client, PgClientConfig::new()
    .timeout(Duration::from_secs(30))
    .slow_threshold(Duration::from_secs(1))
    .with_logging()
    .with_stats()
    .statement_cache(64)
    .strict());
```

## PgClientConfig Options

| Method | Description |
|--------|-------------|
| `.timeout(Duration)` | Set query timeout |
| `.slow_threshold(Duration)` | Set slow query threshold for alerting |
| `.with_logging()` | Enable query logging |
| `.log_slow_queries(Duration)` | Log only queries exceeding the given duration |
| `.with_stats()` | Enable query statistics collection |
| `.statement_cache(cap)` | Enable LRU statement cache with given capacity |
| `.strict()` | Set SQL checking to `Strict` mode (block invalid queries) |
| `.no_check()` | Disable SQL checking entirely |
| `.check_mode(CheckMode)` | Set check mode: `Disabled`, `WarnOnly`, or `Strict` |

### SQL Check Modes

- **`Disabled`** -- no checking
- **`WarnOnly`** (default) -- logs warnings for unknown tables/columns but executes the query
- **`Strict`** -- returns an error for unknown tables/columns before executing

```rust
use pgorm::{PgClient, PgClientConfig};

// Strict: block queries referencing unknown tables/columns
let pg = PgClient::with_config(&client, PgClientConfig::new().strict());

// Disabled: skip all SQL checking
let pg = PgClient::with_config(&client, PgClientConfig::new().no_check());
```

## Statement Cache

PgClient includes an LRU prepared statement cache. Prepared statements are per-connection, so the cache also lives per-connection. When the cache is full, the least recently used statement is evicted.

```rust
use pgorm::{PgClient, PgClientConfig, query};
use tokio_postgres::NoTls;

// Connect without pool (raw tokio_postgres::Client)
let (client, connection) = tokio_postgres::connect(&database_url, NoTls)
    .await
    .map_err(pgorm::OrmError::from_db_error)?;
tokio::spawn(async move { let _ = connection.await; });

// Enable cache with capacity 64
let pg = PgClient::with_config(client, PgClientConfig::new()
    .no_check()
    .statement_cache(64));

// First execution: prepares the statement (cache miss)
let _v: i64 = query("SELECT $1::bigint + $2::bigint")
    .tag("add")
    .bind(1_i64)
    .bind(2_i64)
    .fetch_scalar_one(&pg)
    .await?;

// Subsequent executions: hits the cache (no re-prepare)
let _v: i64 = query("SELECT $1::bigint + $2::bigint")
    .tag("add")
    .bind(3_i64)
    .bind(4_i64)
    .fetch_scalar_one(&pg)
    .await?;

// Check cache stats
let stats = pg.stats();
println!(
    "hits={}, misses={}, prepares={}, prepare_time={:?}",
    stats.stmt_cache_hits,
    stats.stmt_cache_misses,
    stats.stmt_prepare_count,
    stats.stmt_prepare_duration
);
```

## Query Statistics

When statistics are enabled, `PgClient` tracks query counts and durations:

```rust
let pg = PgClient::with_config(&client, PgClientConfig::new()
    .with_stats()
    .log_slow_queries(Duration::from_millis(50)));

// Run queries...
let users = User::select_all(&pg).await?;

// Read stats
let stats = pg.stats();
println!("total queries: {}", stats.total_queries);
println!("SELECT count: {}", stats.select_count);
println!("max duration: {:?}", stats.max_duration);

// Reset stats for a new measurement window
pg.reset_stats();
```

## Custom Pool Configuration

For production, configure pool size, recycling, and TLS:

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

Use `create_pool_with_tls` to connect with a TLS connector:

```rust
use pgorm::create_pool_with_tls;

let tls = /* e.g. tokio_postgres_rustls::MakeRustlsConnect */;
let pool = create_pool_with_tls(&database_url, tls)?;
```

## Using Without Pool

You can use pgorm without `deadpool-postgres` by connecting directly with `tokio_postgres::connect`:

```rust
use tokio_postgres::NoTls;

let (client, connection) = tokio_postgres::connect(&database_url, NoTls)
    .await
    .map_err(pgorm::OrmError::from_db_error)?;
tokio::spawn(async move { let _ = connection.await; });

// Use the raw client directly
let users = User::select_all(&client).await?;

// Or wrap with PgClient for monitoring
let pg = PgClient::with_config(client, PgClientConfig::new()
    .statement_cache(64)
    .with_stats());
```

---

Next: [Models & Derive Macros](/en/guide/models)
