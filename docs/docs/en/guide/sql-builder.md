# Dynamic SQL: `sql()`

When your SQL needs to be *composed* (optional WHERE, optional JOINs, dynamic ordering/pagination), `pgorm::sql()` is a better fit than `query()`:

- you build SQL fragments and pgorm generates `$1, $2, ...` automatically
- values are still bound (safe)
- you can still execute via `fetch_*` / `execute`

> For structured WHERE/ORDER BY/Pagination builders, see: [`Dynamic Filters & Pagination`](/en/guide/conditions).

## 1) Minimal example: optional filters

```rust
use pgorm::sql;

let mut q = sql("SELECT id, username FROM users WHERE 1=1");

if let Some(status) = status {
    q.push(" AND status = ").push_bind(status);
}

if let Some(keyword) = keyword {
    q.push(" AND username ILIKE ").push_bind(format!("%{keyword}%"));
}

q.push(" ORDER BY id DESC");

let users: Vec<User> = q.fetch_all_as(&client).await?;
```

## 2) `push()` vs `push_bind()`

- `push("...")`: append raw SQL
- `push_bind(v)`: append one placeholder and bind `v`

```rust
let mut q = sql("SELECT * FROM users WHERE id = ");
q.push_bind(1_i64);
```

## 3) `push_bind_list()`: build `IN (...)`

```rust
let mut q = sql("SELECT * FROM users WHERE id IN (");
q.push_bind_list([1_i64, 2, 3]);
q.push(")");
```

If the list is empty, `push_bind_list([])` appends `NULL` (so you get `IN (NULL)`).

## 4) Safe dynamic identifiers: `push_ident()`

Postgres **does not** allow binding identifiers (table/column names) as parameters. For dynamic identifiers, use `push_ident()` which validates and escapes:

```rust
use pgorm::sql;

let mut q = sql("SELECT * FROM users ORDER BY ");
q.push_ident("created_at")?;
q.push(" DESC");
```

For ORDER BY, prefer `OrderBy` (also validates identifiers): [`Dynamic Filters & Pagination`](/en/guide/conditions).

## 5) Debugging: inspect final SQL and param count

```rust
let sql_text = q.to_sql();
let param_count = q.params_ref().len();
```

## Next

- Next: [`Dynamic Filters & Pagination`](/en/guide/conditions)
