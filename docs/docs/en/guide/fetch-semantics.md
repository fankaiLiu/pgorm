# Fetch Semantics & Streaming

In pgorm, "running a query" is split into two choices:

1. how you build SQL (`query()` or `sql()`)
2. what row-count semantics you expect (`fetch_one` / `fetch_one_strict` / `fetch_opt` ...)

This page covers (2) in detail, plus streaming for large result sets.

## 1. Choosing a fetch method

### `fetch_one` / `fetch_one_as::<T>`: first row (non-strict)

- 0 rows: `Err(OrmError::NotFound(..))`
- 1 row: OK
- 2+ rows: returns the first row (no error)

Use when you intentionally want "the first row" (ideally with `ORDER BY ... LIMIT 1`).

```rust
use pgorm::{RowExt, query};

let row = query("SELECT id, name FROM items WHERE name = $1 ORDER BY id")
    .bind("dup")
    .fetch_one(&client)
    .await?;
let id: i64 = row.try_get_column("id")?;
```

### `fetch_one_strict` / `fetch_one_strict_as::<T>`: exactly one row

- 0 rows: `Err(OrmError::NotFound(..))`
- 1 row: OK
- 2+ rows: `Err(OrmError::TooManyRows { expected, got })`

Use when querying by a unique key / primary key, and you want inconsistencies to fail loudly.

```rust
match query("SELECT id, name FROM items WHERE name = $1 ORDER BY id")
    .bind("dup")
    .fetch_one_strict(&client)
    .await
{
    Ok(row) => { /* exactly one row */ }
    Err(OrmError::TooManyRows { expected, got }) => {
        println!("Expected {expected} row, got {got}");
    }
    Err(e) => return Err(e),
}
```

### `fetch_opt` / `fetch_opt_as::<T>`: optional row

- 0 rows: `Ok(None)`
- 1 row: `Ok(Some(..))`
- 2+ rows: returns the first row as `Some(..)` (non-strict)

Use when the record may or may not exist.

```rust
let maybe_row = query("SELECT id FROM items WHERE id = $1")
    .bind(9999_i64)
    .fetch_opt(&client)
    .await?;
// maybe_row is None if no row matches
```

### `fetch_all` / `fetch_all_as::<T>`: all rows

Returns all matching rows. An empty result is `Ok(vec![])`, not an error.

```rust
let users: Vec<User> = query("SELECT id, username FROM users ORDER BY id")
    .fetch_all_as(&client)
    .await?;
```

### `*_as::<T>` for struct mapping

Any type implementing `FromRow` (usually via `#[derive(FromRow)]`) can be used with the `*_as` variants:

```rust
use pgorm::{FromRow, query};

#[derive(Debug, FromRow)]
struct User {
    id: i64,
    username: String,
}

let user: User = query("SELECT id, username FROM users WHERE id = $1")
    .bind(1_i64)
    .fetch_one_as(&client)
    .await?;

let maybe_user: Option<User> = query("SELECT id, username FROM users WHERE id = $1")
    .bind(2_i64)
    .fetch_opt_as(&client)
    .await?;
```

## 2. Scalar helpers

When you only care about the first column of the result (COUNT, MAX, etc.), use scalar methods:

```rust
// Single scalar value (errors if 0 rows)
let count: i64 = query("SELECT COUNT(*) FROM users")
    .fetch_scalar_one(&client)
    .await?;

// Optional scalar (0 rows = None)
let maybe_max: Option<i64> = query("SELECT MAX(id) FROM users")
    .fetch_scalar_opt(&client)
    .await?;

// All scalars as a Vec
let all_ids: Vec<i64> = query("SELECT id FROM users ORDER BY id")
    .fetch_scalar_all(&client)
    .await?;
```

### `exists()`

A convenience for checking if any rows match:

```rust
let has_active: bool = query("SELECT 1 FROM users WHERE status = $1")
    .bind("active")
    .exists(&client)
    .await?;
```

## 3. Quick rule of thumb

| I want... | Use |
|-----------|-----|
| The first row | `fetch_one` |
| Exactly one row (unique key) | `fetch_one_strict` |
| A row that might not exist | `fetch_opt` |
| All matching rows | `fetch_all` |
| Just a number (COUNT, MAX, ...) | `fetch_scalar_one` |
| A number that might be NULL | `fetch_scalar_opt` |
| To check existence | `exists()` |
| To map rows into a struct | Add `_as::<T>` to any fetch method |

## 4. Streaming queries

For large result sets where you do not want to load everything into memory at once, use `stream_as::<T>()`. Rows arrive one at a time as PostgreSQL sends them.

### Basic streaming

```rust
use futures_util::StreamExt;
use pgorm::{FromRow, query};

#[derive(Debug, FromRow)]
struct Item {
    n: i64,
}

let mut stream = query("SELECT generate_series(1, $1) AS n")
    .bind(10_i64)
    .tag("examples.streaming")
    .stream_as::<Item>(&client)
    .await?;

while let Some(item) = stream.next().await {
    let item = item?;
    println!("{}", item.n);
}
```

### Backpressure

`stream_as::<T>()` returns a `FromRowStream<T>` that implements `futures::Stream`. Rows arrive as PostgreSQL sends them. If your consumer is slow, PostgreSQL's TCP flow control provides natural backpressure -- the database will pause sending rows until you consume them.

### When to use streaming

- Processing millions of rows for data export or ETL
- Aggregating data without holding all rows in memory
- Any situation where `fetch_all` would use too much memory

For most queries that return a bounded number of rows (e.g., with `LIMIT`), `fetch_all_as` is simpler and sufficient.

## Next

- Next: [Advanced: CTE & Bulk Ops](/en/guide/advanced-queries)
