# Row Mapping: `FromRow` / `RowExt` / JSONB / INET

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

## 4) INET: `std::net::IpAddr`

PostgreSQL `inet` columns map directly to Rust `std::net::IpAddr` (nullable: `Option<IpAddr>`):

```rust
use pgorm::{FromRow, query};
use std::net::IpAddr;

#[derive(Debug, FromRow)]
struct AuditLog {
    id: i64,
    ip_address: Option<IpAddr>, // PG: inet
}

let ip: IpAddr = "1.2.3.4".parse()?;
let logs: Vec<AuditLog> = query("SELECT id, ip_address FROM audit_logs WHERE ip_address = $1")
    .bind(ip)
    .fetch_all_as(&client)
    .await?;
```

If you prefer manual extraction via `RowExt`:

```rust
use pgorm::RowExt;

let ip: Option<std::net::IpAddr> = row.try_get_column("ip_address")?;
```

> Note: don’t map/bind `inet` as `String`; parse into `IpAddr` first. If your input layer is string-based, `#[orm(ip, input_as = "String")]` can validate+parse and return consistent `ValidationErrors`.

## 5) A small style tip

- Prefer explicit column lists over `SELECT *` for more stable mappings.
- For joins/aggregations, give columns clear aliases and map with `#[orm(column = "...")]`.

## Next

- Next: [`Models & Derive`](/en/guide/models)
