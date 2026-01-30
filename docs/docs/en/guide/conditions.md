# Dynamic Filters & Pagination

When writing dynamic SQL, there are two common footguns:

1) composing WHERE logic correctly (AND/OR/NOT + parentheses)  
2) building ORDER BY / LIMIT / OFFSET safely (especially when column names come from user input)  

pgorm provides structured builders for these:

- `Condition` / `Op`: atomic predicates (validates identifiers)
- `WhereExpr`: boolean expression tree (handles grouping/parentheses)
- `OrderBy`: safe ORDER BY builder (validates identifiers)
- `Pagination`: LIMIT/OFFSET (uses bound params)

## 1) Build optional WHERE with `WhereExpr`

Start from `WhereExpr::and(Vec::new())` (identity `TRUE`), then add optional filters:

```rust
use pgorm::{Condition, WhereExpr, sql};

let mut where_expr = WhereExpr::and(Vec::new());

if let Some(status) = status {
    where_expr = where_expr.and_with(WhereExpr::atom(Condition::eq("status", status)?));
}

if let Some(keyword) = keyword {
    where_expr = where_expr.and_with(WhereExpr::atom(Condition::ilike(
        "username",
        format!("%{keyword}%"),
    )?));
}

let mut q = sql("SELECT id, username FROM users");
if !where_expr.is_trivially_true() {
    q.push(" WHERE ");
    where_expr.append_to_sql(&mut q);
}
```

## 2) Common `Condition` helpers

```rust
use pgorm::{Condition, Op};

let c1 = Condition::eq("status", "active")?;
let c2 = Condition::ne("role", "banned")?;
let c3 = Condition::ilike("name", "%alice%")?;
let c4 = Condition::is_null("deleted_at")?;
let c5 = Condition::new("id", Op::between(1_i64, 100_i64))?;
let c6 = Condition::new("role", Op::in_list(vec!["admin", "owner"]))?;
```

Important: **column names are validated identifiers**. If you pass an invalid identifier, you’ll get `OrmError::Validation` instead of unsafe SQL.

## 3) OR groups: express (A AND (B OR C))

```rust
use pgorm::{Condition, WhereExpr};

let expr = WhereExpr::and(vec![
    Condition::eq("status", "active")?.into(),
    WhereExpr::or(vec![
        Condition::eq("role", "admin")?.into(),
        Condition::eq("role", "owner")?.into(),
    ]),
]);
```

## 4) Safe ORDER BY: `OrderBy`

Don’t do `ORDER BY {user_input}`. Use `OrderBy` (validates identifiers):

```rust
use pgorm::{NullsOrder, OrderBy, Pagination, SortDir, sql};

let mut q = sql("SELECT id, username, created_at FROM users");

let mut order = OrderBy::new()
    .with_nulls("created_at", SortDir::Desc, NullsOrder::Last)?;

if let Some(sort_by) = sort_by {
    order = order.asc(sort_by.as_str())?;
}

order.append_to_sql(&mut q);
Pagination::page(1, 20)?.append_to_sql(&mut q);
```

## 5) Pagination: `Pagination`

`Pagination` appends `LIMIT $n OFFSET $m` with bound params:

```rust
use pgorm::{Pagination, sql};

let mut q = sql("SELECT * FROM users");
Pagination::page(page, per_page)?.append_to_sql(&mut q);
```

> `page` starts at 1. `per_page` is not auto-validated; clamp it in your app (e.g. 1..=200).

## Next

- Next: [`Fetch Semantics`](/en/guide/fetch-semantics)
