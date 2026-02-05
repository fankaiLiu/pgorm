# Migrations

pgorm integrates [refinery](https://github.com/rust-db/refinery) behind the optional `migrate` feature for running SQL migrations against your PostgreSQL database.

## Enable the Feature

Add pgorm with the `migrate` feature to your `Cargo.toml`:

```toml
[dependencies]
pgorm = { version = "0.2.0", features = ["migrate"] }
```

If you also use connection pooling (most applications do):

```toml
pgorm = { version = "0.2.0", features = ["migrate", "pool"] }
```

## Migration File Naming

Migration files follow the refinery naming convention: `V{number}__{description}.sql` (note the double underscore). Place them in a `migrations/` directory:

```text
your_app/
  migrations/
    V1__create_products.sql
    V2__create_categories.sql
    V3__add_users.sql
```

Example migration files:

```sql
-- V1__create_products.sql
CREATE TABLE IF NOT EXISTS products (
    id BIGSERIAL PRIMARY KEY,
    name TEXT NOT NULL,
    price_cents BIGINT NOT NULL,
    in_stock BOOLEAN NOT NULL DEFAULT TRUE
);
```

```sql
-- V2__create_categories.sql
CREATE TABLE IF NOT EXISTS categories (
    id BIGSERIAL PRIMARY KEY,
    name TEXT NOT NULL UNIQUE
);
```

## Embedding and Running Migrations

Use the `embed_migrations!` macro to embed your SQL files into the binary at compile time, then run them with `migrate::run_pool`:

```rust
use pgorm::{create_pool, migrate};

mod embedded {
    use pgorm::embed_migrations;
    embed_migrations!("./migrations");
}

#[tokio::main]
async fn main() -> pgorm::OrmResult<()> {
    let database_url = std::env::var("DATABASE_URL")
        .map_err(|_| pgorm::OrmError::Connection("DATABASE_URL is not set".to_string()))?;

    let pool = create_pool(&database_url)?;
    let report = migrate::run_pool(&pool, embedded::migrations::runner()).await?;

    println!("Applied {} migration(s)", report.applied_migrations().len());
    Ok(())
}
```

### Key Points

- **`embed_migrations!("./migrations")`** -- the path is relative to the crate's `Cargo.toml`
- **`embedded::migrations::runner()`** -- returns a refinery `Runner` that tracks which migrations have been applied
- **`migrate::run_pool(&pool, runner)`** -- runs pending migrations using a connection from the pool

## Production Considerations

1. **Run migrations on startup or in a dedicated job.** Most applications run migrations at the start of the server process. For larger deployments, consider running them as a separate step in your deployment pipeline.

2. **Handle concurrency.** Refinery uses a `refinery_schema_history` table with a lock to prevent multiple instances from running migrations simultaneously. However, for safety, ensure only one instance runs migrations at a time (e.g. via a Kubernetes init container or a deployment script).

3. **Keep migrations forward-only.** Each migration should be additive. Avoid modifying or deleting previously applied migration files -- refinery checks hashes to detect changes.

4. **Test migrations in CI.** Run your migration suite against a fresh database in your CI pipeline to catch issues early.

## Refinery Documentation

For advanced features (migration groups, async runners, different backends), see the [refinery documentation](https://docs.rs/refinery).

## Runnable Example

See `crates/pgorm/examples/migrate/main.rs` for a complete runnable example. Run it with:

```bash
DATABASE_URL=postgres://postgres:postgres@localhost/pgorm_example \
  cargo run --example migrate -p pgorm --features migrate
```

## Next

- Next: [CLI: pgorm-cli](/en/guide/cli)
