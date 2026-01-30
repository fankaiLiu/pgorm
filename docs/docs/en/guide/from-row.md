# Row Mapping: `FromRow` / `RowExt` / JSONB

pgorm is SQL-first, so it keeps “querying” and “mapping” loosely coupled:

- you write SQL (`query()` / `sql()`)
- you map `tokio_postgres::Row` into Rust types

This page covers two common approaches:

1) `#[derive(FromRow)]` for struct mapping (recommended)  
2) `RowExt` for manual extraction (escape hatch)  

## 1) `#[derive(FromRow)]`: the common case

```rust
use pgorm::FromRow;

#[derive(Debug, FromRow)]
struct User {
    id: i64,
    username: String,
    email: Option<String>, // NULL -> None
}
```

Then use `fetch_*_as`:

```rust
use pgorm::query;

let users: Vec<User> = query("SELECT id, username, email FROM users ORDER BY id")
    .fetch_all_as(&client)
    .await?;
```

### Column aliasing: `#[orm(column = \"...\")]`

If your SQL column name differs from your Rust field name (or you used an alias), map explicitly:

```rust
use pgorm::FromRow;

#[derive(Debug, FromRow)]
struct User {
    id: i64,
    #[orm(column = "user_name")]
    username: String,
}
```

## 2) `RowExt`: manual typed access

```rust
use pgorm::{RowExt, query};

let row = query("SELECT id, username FROM users WHERE id = $1")
    .bind(1_i64)
    .fetch_one(&client)
    .await?;

let id: i64 = row.try_get_column("id")?;
let username: String = row.try_get_column("username")?;
```

`try_get_column` converts tokio-postgres decode errors into `OrmError::Decode`.

## 3) JSONB: strongly-typed and dynamic

### Strongly-typed: `Json<T>`

```rust
use pgorm::Json;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Meta {
    tags: Vec<String>,
    active: bool,
}

let meta: Json<Meta> = row.try_get_column("meta")?;
println!("{:?}", meta.0);
```

### Dynamic: `serde_json::Value`

```rust
let v: serde_json::Value = row.try_get_column("meta")?;
println!("{v}");
```

> Runnable example: `crates/pgorm/examples/jsonb`.

## 4) A small style tip

- Prefer explicit column lists over `SELECT *` for more stable mappings.
- For joins/aggregations, give columns clear aliases and map with `#[orm(column = "...")]`.

## Next

- Next: [`Models & Derive`](/en/guide/models)
