# Fetch Semantics: which `fetch_*` should you use?

In pgorm, “running a query” is split into two choices:

1) how you build SQL (`query()` or `sql()`)  
2) what row-count semantics you expect (`fetch_one` / `fetch_one_strict` / `fetch_opt` …)  

This page focuses on (2).

## 1) The 3 most common choices

### `fetch_one*`: first row (non-strict)

- 0 rows: `Err(OrmError::NotFound(..))`
- 1 row: OK
- multiple rows: returns the first row (no error)

Use when: you intentionally want “the first row” (ideally with `ORDER BY ... LIMIT 1`).

### `fetch_one_strict*`: exactly one row

- 0 rows: `Err(OrmError::NotFound(..))`
- 1 row: OK
- multiple rows: `Err(OrmError::TooManyRows { .. })`

Use when: querying by a unique key / primary key, and you want inconsistencies to fail loudly.

### `fetch_opt*`: optional row

- 0 rows: `Ok(None)`
- 1 row: `Ok(Some(..))`
- multiple rows: returns the first row (non-strict)

Use when: the record may or may not exist.

## 2) Example: the same SQL with different semantics

```rust
use pgorm::{OrmError, RowExt, query};

let row = query("SELECT id, name FROM items WHERE name = $1 ORDER BY id")
    .bind("dup")
    .fetch_one(&client)
    .await?;
let id: i64 = row.try_get_column("id")?;
println!("fetch_one => id={id}");

match query("SELECT id FROM items WHERE name = $1 ORDER BY id")
    .bind("dup")
    .fetch_one_strict(&client)
    .await
{
    Ok(_) => println!("unexpected: strict succeeded"),
    Err(OrmError::TooManyRows { expected, got }) => {
        println!("strict => TooManyRows (expected {expected}, got {got})")
    }
    Err(e) => return Err(e),
}

let maybe = query("SELECT id FROM items WHERE id = $1")
    .bind(9999_i64)
    .fetch_opt(&client)
    .await?;
println!("fetch_opt => {}", if maybe.is_some() { "Some" } else { "None" });
```

> Runnable example: `crates/pgorm/examples/fetch_semantics`.

## 3) `*_as`: map directly into a struct

If your type implements `FromRow` (usually via `#[derive(FromRow)]`), use `*_as`:

```rust
let user: User = query("SELECT id, username FROM users WHERE id = $1")
    .bind(1_i64)
    .fetch_one_as(&client)
    .await?;

let users: Vec<User> = query("SELECT id, username FROM users ORDER BY id")
    .fetch_all_as(&client)
    .await?;
```

Mapping details: [`Row Mapping: FromRow`](/en/guide/from-row).

## 4) Scalars: `fetch_scalar_*`

For single-column results:

```rust
let count: i64 = query("SELECT COUNT(*) FROM users").fetch_scalar_one(&client).await?;
let maybe: Option<i64> = query("SELECT MAX(id) FROM users").fetch_scalar_opt(&client).await?;
let all_ids: Vec<i64> = query("SELECT id FROM users ORDER BY id").fetch_scalar_all(&client).await?;
```

## 5) Quick rule of thumb

- “I want the first row”: `fetch_one`  
- “It must be unique”: `fetch_one_strict`  
- “It might not exist”: `fetch_opt`  
- “I only need a number”: `fetch_scalar_*`  

## Next

- Next: [`Row Mapping: FromRow`](/en/guide/from-row)
