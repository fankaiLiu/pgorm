# Migrations: `refinery`

pgorm integrates `refinery` behind the optional `migrate` feature, so you can run SQL migrations.

## 1) Enable the feature

```toml
[dependencies]
pgorm = { version = "0.1.1", features = ["migrate", "pool"] }
```

> If you don’t use the built-in pool helpers, you can manage `tokio_postgres::Client` yourself. Most examples use `pool` for brevity.

## 2) Embed migrations: `embed_migrations!`

Example structure:

```text
your_app/
  migrations/
    V1__init.sql
    V2__add_users.sql
```

Embed and run:

```rust
use pgorm::{create_pool, migrate};

mod embedded {
    use pgorm::embed_migrations;
    embed_migrations!("./migrations");
}

let pool = create_pool(&database_url)?;
let report = migrate::run_pool(&pool, embedded::migrations::runner()).await?;
println!("Applied {} migration(s)", report.applied_migrations().len());
```

> Runnable example: `crates/pgorm/examples/migrate` (requires `--features migrate,pool`).

## 3) Notes

1) **Path base**: `embed_migrations!("./migrations")` is relative to the crate’s `Cargo.toml`.  
2) **Production**: run migrations on startup or in a dedicated job; handle concurrency safely.  

## Next

- Next: [`Monitoring & Hooks`](/en/guide/monitoring)
