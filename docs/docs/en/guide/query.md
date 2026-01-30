# Hand-written SQL: `query()`

Use `pgorm::query()` when you already have a complete SQL string and you’re happy to write `$1, $2, ...` placeholders explicitly:

- you fully control the SQL (SQL-first)
- values are bound via `.bind()` (no string interpolation)
- you still get consistent `fetch_*` / `execute` APIs

> If you need to *compose* SQL dynamically without tracking `$n` yourself, see: [`sql()`](/en/guide/sql-builder).

## 1) Minimal example: fetch and map into a struct

```rust
use pgorm::{FromRow, query};

#[derive(Debug, FromRow)]
struct User {
    id: i64,
    username: String,
}

let user: User = query("SELECT id, username FROM users WHERE id = $1")
    .tag("users.by_id") // optional observability tag
    .bind(1_i64)        // matches $1
    .fetch_one_as(&client)
    .await?;
```

## 2) Binding parameters: always use `bind()`

`query()` is intentionally simple: write `$1..$n` in SQL, then call `.bind(v1).bind(v2)...` in the same order.

```rust
let affected = query("UPDATE users SET username = $1 WHERE id = $2")
    .tag("users.rename")
    .bind("alice_new")
    .bind(42_i64)
    .execute(&client)
    .await?;
```

## 3) The 3 most-used fetch choices

- `fetch_one*`: returns the **first** row; 0 rows => NotFound; multiple rows => first row (no error)  
- `fetch_one_strict*`: requires **exactly one** row; 0 rows => NotFound; multiple rows => TooManyRows  
- `fetch_opt*`: 0 rows => `Ok(None)`; otherwise `Ok(Some(...))` (non-strict on multiple rows)

With `*_as`, rows are mapped into `T: FromRow`:

```rust
let u1: User = query("SELECT id, username FROM users WHERE id = $1")
    .bind(1_i64)
    .fetch_one_as(&client)
    .await?;

let u2: Option<User> = query("SELECT id, username FROM users WHERE id = $1")
    .bind(2_i64)
    .fetch_opt_as(&client)
    .await?;
```

More details: [`Fetch Semantics`](/en/guide/fetch-semantics).

## 4) Scalars: `fetch_scalar_*` / `exists()`

If you only care about the first column (COUNT / MAX / …), use scalar helpers:

```rust
let count: i64 = query("SELECT COUNT(*) FROM users WHERE status = $1")
    .bind("active")
    .fetch_scalar_one(&client)
    .await?;

let maybe_max_id: Option<i64> = query("SELECT MAX(id) FROM users")
    .fetch_scalar_opt(&client)
    .await?;

let has_any: bool = query("SELECT 1 FROM users WHERE status = $1")
    .bind("active")
    .exists(&client)
    .await?;
```

## 5) Observability: `tag()`

`tag()` is just a string label that becomes useful with wrappers like `PgClient` or `InstrumentedClient`:

```rust
let user: User = query("SELECT id, username FROM users WHERE id = $1")
    .tag("users.by_id")
    .bind(1_i64)
    .fetch_one_as(&pg)
    .await?;
```

## Common pitfalls

1) `query()` does **not** generate placeholders — use `sql()` for dynamic composition.  
2) Never interpolate untrusted input into SQL — bind values.  
3) Identifiers (table/column names) cannot be parameterized — for dynamic identifiers, use `sql().push_ident(...)` or `OrderBy`.  

## Next

- Next: [`Dynamic SQL: sql()`](/en/guide/sql-builder)
