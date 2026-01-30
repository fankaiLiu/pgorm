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
