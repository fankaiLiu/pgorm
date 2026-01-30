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

---

## Features

- SQL-first design with explicit queries
- Derive macros: `FromRow`, `Model`, `InsertModel`, `UpdateModel`, `ViewModel`
- Connection pooling via `deadpool-postgres`
- Eager loading for relations (`has_many`, `belongs_to`)
- JSONB support out of the box
- SQL migrations via `refinery`
- Runtime SQL checking for AI-generated queries

## Installation

```toml
[dependencies]
pgorm = "0.1.1"
```

## Quick Start

```rust
use pgorm::{query, FromRow};

#[derive(FromRow)]
struct User {
    id: i64,
    username: String,
}

let user: User = query("SELECT id, username FROM users WHERE id = $1")
    .bind(1_i64)
    .fetch_one_as(&client)
    .await?;
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
