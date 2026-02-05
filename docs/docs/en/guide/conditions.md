# Dynamic Filters & Pagination

When writing dynamic SQL, there are two common footguns:

1. composing WHERE logic correctly (AND/OR/NOT + parentheses)
2. building ORDER BY / LIMIT / OFFSET safely (especially when column names come from user input)

pgorm provides structured builders for both:

- `Condition` / `Op`: atomic predicates (validates identifiers)
- `WhereExpr`: boolean expression tree (handles grouping/parentheses)
- `OrderBy`: safe ORDER BY builder (validates identifiers)
- `Pagination`: page-based LIMIT/OFFSET
- `Keyset1` / `Keyset2`: cursor-based keyset pagination

## 1. `Condition` -- atomic predicates

`Condition` represents a single comparison. All identifiers are validated -- SQL injection via column names is not possible.

### Comparison operators

```rust
use pgorm::{Condition, Op};

// Equality / inequality
let c1 = Condition::eq("status", "active")?;
let c2 = Condition::ne("role", "banned")?;

// Comparison
let c3 = Condition::gt("age", 18_i32)?;
let c4 = Condition::gte("score", 90_i32)?;
let c5 = Condition::lt("login_count", 10_i32)?;
let c6 = Condition::lte("price", 9999_i64)?;

// Pattern matching
let c7 = Condition::like("name", "Alice%")?;
let c8 = Condition::ilike("name", "%test%")?;      // case-insensitive
let c9 = Condition::not_like("name", "%spam%")?;

// Null checks
let c10 = Condition::is_null("deleted_at")?;
let c11 = Condition::is_not_null("email")?;

// Lists and ranges
let c12 = Condition::new("role", Op::in_list(vec!["admin", "owner"]))?;
let c13 = Condition::new("role", Op::not_in(vec!["banned", "suspended"]))?;
let c14 = Condition::new("id", Op::between(1_i64, 100_i64))?;

// Array matching: column = ANY($1)
let c15 = Condition::eq_any("id", vec![1_i64, 2, 3])?;
```

### Range operators (for PostgreSQL range types)

```rust
use pgorm::Condition;
use pgorm::types::Range;

let range = Range::<i32>::inclusive(1, 10);

Condition::overlaps("during", range)?;           // &&
Condition::contains("during", timestamp)?;       // @>
Condition::range_left_of("r", range)?;           // <<
Condition::range_right_of("r", range)?;          // >>
Condition::range_adjacent("r", range)?;          // -|-
```

## 2. `Op<T>` -- operator enum for programmatic use

When you need to select the operator dynamically:

```rust
use pgorm::{Condition, Op};

let c = Condition::new("id", Op::between(1_i64, 100_i64))?;
let c = Condition::new("role", Op::in_list(vec!["admin", "owner"]))?;
```

## 3. `WhereExpr` -- boolean expression tree

`WhereExpr` composes `Condition` values into AND/OR/NOT trees with correct parenthesization.

### Building optional WHERE clauses

Start from `WhereExpr::and(Vec::new())` -- this is a trivially true expression. Add filters conditionally:

```rust
use pgorm::{Condition, WhereExpr, sql};

let mut where_expr = WhereExpr::and(Vec::new());

if let Some(status) = &filters.status {
    where_expr = where_expr.and_with(
        WhereExpr::atom(Condition::eq("status", status.clone())?)
    );
}

if let Some(search) = &filters.search {
    where_expr = where_expr.and_with(
        WhereExpr::atom(Condition::ilike("name", format!("%{search}%"))?)
    );
}

if !filters.include_deleted {
    where_expr = where_expr.and_with(
        WhereExpr::atom(Condition::is_null("deleted_at")?)
    );
}

let mut q = sql("SELECT * FROM users");
if !where_expr.is_trivially_true() {
    q.push(" WHERE ");
    where_expr.append_to_sql(&mut q);
}
```

### `is_trivially_true()`

`WhereExpr::and(Vec::new())` has no children, so `is_trivially_true()` returns `true`. This is useful for conditionally appending `WHERE` -- if no filters were added, you skip the clause entirely.

### AND/OR/NOT composition

```rust
use pgorm::{Condition, Op, WhereExpr};

let expr = WhereExpr::and(vec![
    Condition::eq("status", "active")?.into(),
    WhereExpr::or(vec![
        Condition::eq("role", "admin")?.into(),
        Condition::eq("role", "owner")?.into(),
    ]),
    Condition::new("id", Op::between(1_i64, 100_i64))?.into(),
]);
```

This produces: `(status = $1 AND (role = $2 OR role = $3) AND id BETWEEN $4 AND $5)`.

### Raw escape hatch

For conditions that don't fit the structured API:

```rust
let expr = WhereExpr::raw("expires_at < NOW()");
```

### `append_to_sql(&mut sql)`

Appends the full expression (with correct parentheses and bound parameters) to an `Sql` builder:

```rust
let mut q = sql("SELECT COUNT(*) FROM users WHERE ");
expr.append_to_sql(&mut q);
let count: i64 = q.fetch_scalar_one(&client).await?;
```

## 4. `OrderBy` -- safe ORDER BY

Do not write `ORDER BY {user_input}` directly. Use `OrderBy` which validates identifiers:

```rust
use pgorm::{NullsOrder, OrderBy, SortDir, sql};

let mut order = OrderBy::new()
    .with_nulls("created_at", SortDir::Desc, NullsOrder::Last)?;

// Dynamic column from user input (validated)
if let Some(sort_by) = &filters.sort_by {
    order = order.asc(sort_by.as_str())?;
}

let mut q = sql("SELECT * FROM users");
order.append_to_sql(&mut q);
```

Available methods:

- `asc("column")?` -- ascending order
- `desc("column")?` -- descending order
- `with_nulls("column", SortDir, NullsOrder)?` -- explicit NULLS FIRST/LAST

## 5. Page-based pagination: `Pagination::page(page, per_page)`

Appends `LIMIT $n OFFSET $m` with bound parameters:

```rust
use pgorm::{Pagination, sql};

let mut q = sql("SELECT * FROM users ORDER BY id");
Pagination::page(1, 20)?.append_to_sql(&mut q);
```

`page` starts at 1. Clamp `per_page` in your application (e.g., 1..=200).

## 6. Keyset pagination: `Keyset1` and `Keyset2`

Keyset (cursor) pagination is more efficient than OFFSET for large datasets because it uses index-friendly `WHERE` clauses instead of skipping rows.

### Single-column ordering with `Keyset1`

```rust
use pgorm::Keyset1;

let keyset = Keyset1::desc("id")?.limit(20);
// First page: no cursor
// Subsequent pages: pass the last row's value
let keyset = keyset.after(last_id);
```

### Two-column ordering with `Keyset2`

Use when you need a tie-breaker (e.g., `created_at DESC, id DESC`):

```rust
use pgorm::{Condition, Keyset2, WhereExpr, sql};

let mut where_expr = WhereExpr::and(Vec::new());

if let Some(status) = &filters.status {
    where_expr = where_expr.and_with(Condition::eq("status", status.clone())?.into());
}

// Stable order: created_at DESC, id DESC
let mut keyset = Keyset2::desc("created_at", "id")?.limit(20);

// For subsequent pages, pass the last row's values as cursor
if let (Some(last_ts), Some(last_id)) = (after_created_at, after_id) {
    keyset = keyset.after(last_ts, last_id);
    where_expr = where_expr.and_with(keyset.into_where_expr()?);
}

let mut q = sql("SELECT id, name, status, created_at FROM users");
if !where_expr.is_trivially_true() {
    q.push(" WHERE ");
    where_expr.append_to_sql(&mut q);
}
keyset.append_order_by_limit_to_sql(&mut q)?;
```

### Page-based vs. keyset pagination

| | Page-based (`Pagination`) | Keyset (`Keyset1`/`Keyset2`) |
|---|---|---|
| **How it works** | LIMIT + OFFSET | WHERE + LIMIT (index-friendly) |
| **Performance** | Degrades with large offsets | Constant regardless of page depth |
| **Jumping to page N** | Supported | Not supported (forward/backward only) |
| **Stable with inserts** | No (rows can shift) | Yes (cursor-based) |
| **Best for** | Admin UIs, small datasets | Infinite scroll, APIs, large datasets |

## Next

- Next: [Fetch Semantics & Streaming](/en/guide/fetch-semantics)
