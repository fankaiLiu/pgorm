# SQL Queries: query() & sql()

pgorm provides two ways to write SQL: `query()` for hand-written SQL with numbered placeholders, and `sql()` for dynamically composed SQL with auto-numbered parameters.

## 1. `query()` -- hand-written SQL with numbered placeholders

Use `pgorm::query()` when you have a complete SQL string with explicit `$1, $2, ...` placeholders. Values are bound via `.bind()` -- never use string interpolation.

### `.bind()` for parameters

Call `.bind()` once per placeholder, in order:

```rust
use pgorm::query;

let affected = query("UPDATE users SET username = $1 WHERE id = $2")
    .bind("alice_new")
    .bind(42_i64)
    .execute(&client)
    .await?;
```

### `.tag()` for observability

Attach a label for monitoring and logging. The tag is used by `PgClient` and `InstrumentedClient`:

```rust
let user: User = query("SELECT id, username FROM users WHERE id = $1")
    .tag("users.by_id")
    .bind(1_i64)
    .fetch_one_as(&pg)
    .await?;
```

### Fetch methods

| Method | Behavior |
|--------|----------|
| `fetch_all(&client)` | Returns all rows as `Vec<Row>` |
| `fetch_all_as::<T>(&client)` | Returns all rows mapped to `Vec<T>` |
| `fetch_one(&client)` | First row; 0 rows = `NotFound` error |
| `fetch_one_as::<T>(&client)` | First row mapped to `T` |
| `fetch_one_strict(&client)` | Exactly 1 row; 0 = `NotFound`, 2+ = `TooManyRows` |
| `fetch_opt(&client)` | 0 rows = `None`, 1+ = `Some(first)` |
| `fetch_opt_as::<T>(&client)` | Optional row mapped to `T` |
| `fetch_scalar_one(&client)` | First column of first row |
| `fetch_scalar_opt(&client)` | First column, optional |
| `fetch_scalar_all(&client)` | First column of all rows |
| `exists(&client)` | Returns `true` if any row matches |
| `execute(&client)` | Execute without returning rows (returns affected count) |

### `*_as::<T>` for struct mapping

Any type implementing `FromRow` (usually via `#[derive(FromRow)]`) can be used:

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

let users: Vec<User> = query("SELECT id, username FROM users ORDER BY id")
    .fetch_all_as(&client)
    .await?;
```

### Scalar helpers: `fetch_scalar_one`, `fetch_scalar_opt`, `exists()`

When you only care about the first column:

```rust
let count: i64 = query("SELECT COUNT(*) FROM users WHERE status = $1")
    .bind("active")
    .fetch_scalar_one(&client)
    .await?;

let maybe_max: Option<i64> = query("SELECT MAX(id) FROM users")
    .fetch_scalar_opt(&client)
    .await?;

let has_any: bool = query("SELECT 1 FROM users WHERE status = $1")
    .bind("active")
    .exists(&client)
    .await?;
```

### Common pitfalls

1. `query()` does **not** auto-number placeholders -- you must write `$1`, `$2`, etc. yourself. For auto-numbering, use `sql()`.
2. Never interpolate untrusted input into SQL. Always use `.bind()`.
3. Identifiers (table/column names) cannot be parameterized in PostgreSQL -- for dynamic identifiers, use `sql().push_ident(...)` or `OrderBy`.

## 2. `sql()` -- dynamic SQL composition

Use `pgorm::sql()` when your SQL needs to be composed at runtime (optional WHERE clauses, dynamic JOINs, etc.). It handles parameter numbering (`$1, $2, ...`) automatically.

### `push()` for SQL fragments

```rust
use pgorm::sql;

let mut q = sql("SELECT id, username FROM users WHERE 1=1");

if let Some(status) = status {
    q.push(" AND status = ");
    q.push_bind(status);
}

q.push(" ORDER BY id DESC");
let users: Vec<User> = q.fetch_all_as(&client).await?;
```

### `push_bind()` for parameters (auto-numbered)

Each call to `push_bind()` appends a `$N` placeholder and binds the value:

```rust
let mut q = sql("SELECT * FROM users WHERE id = ");
q.push_bind(1_i64);
// Generates: SELECT * FROM users WHERE id = $1
```

You can also chain with `push()`:

```rust
q.push(" AND status = ").push_bind("active");
```

### `push_bind_list()` for IN clauses

Appends a comma-separated list of placeholders:

```rust
let mut q = sql("SELECT * FROM users WHERE id IN (");
q.push_bind_list([1_i64, 2, 3]);
q.push(")");
// Generates: SELECT * FROM users WHERE id IN ($1, $2, $3)
```

If the list is empty, `push_bind_list([])` appends `NULL` (so `IN (NULL)` is valid SQL that matches nothing).

### `push_ident()` for safe dynamic identifiers

PostgreSQL does not allow parameterizing identifiers (table/column names). Use `push_ident()` which validates the identifier against injection:

```rust
let mut q = sql("SELECT * FROM users ORDER BY ");
q.push_ident("created_at")?;
q.push(" DESC");
```

Only `[a-zA-Z0-9_]` and qualified names like `schema.table.column` are accepted. Invalid identifiers return an `OrmError::Validation`.

### Owning `.bind()` variant

For one-liner construction (commonly used in CTE sub-queries), use `.bind()` which takes and returns ownership:

```rust
let sub = sql("SELECT id FROM users WHERE status = ").bind("active");
```

### Debugging: `.to_sql()` and `.params_ref()`

Inspect the generated SQL and parameter count:

```rust
let q = build_list_users_sql(&filters)?;
println!("SQL: {}", q.to_sql());
println!("params: {}", q.params_ref().len());
```

## 3. When to use which

| Scenario | Use |
|----------|-----|
| Static SQL with known placeholders | `query("SELECT ... WHERE id = $1").bind(id)` |
| Dynamic SQL (optional filters, conditional JOINs) | `sql("SELECT ...").push(...).push_bind(...)` |
| Simple CRUD on a model | Model methods (`select_all`, `insert_returning`, etc.) |
| Type-safe WHERE / ORDER BY / pagination | `Condition`, `WhereExpr`, `OrderBy`, `Pagination` with `sql()` |

## Next

- Next: [Dynamic Filters & Pagination](/en/guide/conditions)
