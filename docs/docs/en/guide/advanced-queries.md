# Advanced: CTE & Bulk Ops

This page covers pgorm's support for Common Table Expressions (WITH queries), bulk updates, and bulk deletes.

## 1. CTE (WITH) Queries

CTEs let you define named sub-queries that can be referenced in the main SELECT. pgorm's `sql()` builder supports simple, multiple, columned, recursive, and data-modification CTEs.

### Simple CTE

```rust
use pgorm::sql;

let q = sql("")
    .with(
        "active_users",
        sql("SELECT id, name FROM users WHERE status = ").bind("active"),
    )?
    .select(sql("SELECT * FROM active_users"));

let users: Vec<User> = q.fetch_all_as(&client).await?;
```

This generates:

```sql
WITH active_users AS (
  SELECT id, name FROM users WHERE status = $1
)
SELECT * FROM active_users
```

### Multiple CTEs

Chain `.with()` calls to define multiple CTEs:

```rust
let q = sql("")
    .with(
        "active_users",
        sql("SELECT id, name FROM users WHERE status = ").bind("active"),
    )?
    .with(
        "recent_orders",
        sql("SELECT user_id, amount FROM orders WHERE amount > ").bind(100_i64),
    )?
    .select(sql(
        "SELECT u.id, u.name, SUM(o.amount) as total \
         FROM active_users u \
         JOIN recent_orders o ON o.user_id = u.id \
         GROUP BY u.id, u.name"
    ));
```

### CTE with column names: `with_columns`

When the CTE needs explicit column aliases:

```rust
let q = sql("")
    .with_columns(
        "monthly_sales",
        ["month", "total"],
        sql("SELECT DATE_TRUNC('month', created_at), SUM(amount) FROM orders GROUP BY 1"),
    )?
    .select(sql("SELECT * FROM monthly_sales WHERE total > ").bind(10000_i64));
```

This generates:

```sql
WITH monthly_sales(month, total) AS (
  SELECT DATE_TRUNC('month', created_at), SUM(amount) FROM orders GROUP BY 1
)
SELECT * FROM monthly_sales WHERE total > $1
```

### Recursive CTE: `with_recursive`

For hierarchical data (org charts, category trees, etc.):

```rust
use pgorm::{FromRow, sql};

#[derive(Debug, FromRow)]
struct OrgNode {
    id: i64,
    name: String,
    parent_id: Option<i64>,
    level: i32,
    path: String,
}

let org_tree: Vec<OrgNode> = sql("")
    .with_recursive(
        "org_tree",
        // Base case: root nodes
        sql(
            "SELECT id, name, parent_id, 0 as level, name::text as path \
             FROM employees WHERE parent_id IS NULL",
        ),
        // Recursive step
        sql(
            "SELECT e.id, e.name, e.parent_id, t.level + 1, t.path || ' > ' || e.name \
             FROM employees e JOIN org_tree t ON e.parent_id = t.id",
        ),
    )?
    .select(sql("SELECT * FROM org_tree ORDER BY path"))
    .fetch_all_as(&client)
    .await?;
```

Recursive CTEs with parameters (e.g., depth limit):

```rust
let tree: Vec<CategoryNode> = sql("")
    .with_recursive(
        "category_tree",
        sql("SELECT id, name, 0 as depth FROM categories WHERE id = ").bind(1_i64),
        sql("SELECT c.id, c.name, ct.depth + 1 \
             FROM categories c JOIN category_tree ct ON c.parent_id = ct.id \
             WHERE ct.depth < ")
        .bind(5_i32),
    )?
    .select_from("category_tree")?
    .fetch_all_as(&client)
    .await?;
```

### `select_from` shorthand

Instead of `.select(sql("SELECT * FROM cte_name"))`, you can use `.select_from("cte_name")?` which validates the identifier:

```rust
let q = sql("")
    .with_recursive("reachable", base, recursive)?
    .select_from("reachable")?;
```

### Recursive CTE with UNION (dedup)

By default, `with_recursive` uses `UNION ALL`. To deduplicate rows (useful for graph traversal), use `with_recursive_union`:

```rust
let q = sql("")
    .with_recursive_union(
        "reachable",
        sql("SELECT end_node FROM edges WHERE start_node = ").bind(1_i64),
        sql("SELECT e.end_node FROM edges e JOIN reachable r ON e.start_node = r.end_node"),
    )?
    .select_from("reachable")?;
```

### Data modification CTEs

CTEs can contain INSERT, UPDATE, or DELETE with RETURNING, piping results into the main query:

```rust
let mut delete_sql = sql("DELETE FROM orders WHERE status = 'cancelled' AND created_at < ");
delete_sql.push_bind("2024-01-01");
delete_sql.push(" RETURNING *");

let q = sql("")
    .with("deleted_orders", delete_sql)?
    .select(sql("INSERT INTO orders_archive SELECT * FROM deleted_orders"));

q.execute(&client).await?;
```

## 2. Bulk Update

Use `sql("table").update_many([...])` to update multiple rows matching a condition.

### Basic bulk update

```rust
use pgorm::{Condition, SetExpr, sql};

let affected = sql("users")
    .update_many([SetExpr::set("status", "inactive")?])?
    .filter(Condition::lt("login_count", 10_i32)?)
    .execute(&client)
    .await?;

println!("Deactivated {affected} users");
```

### `SetExpr` variants

| Method | Effect |
|--------|--------|
| `SetExpr::set("col", value)?` | `SET col = $N` |
| `SetExpr::raw("updated_at = NOW()")` | Raw SQL expression |
| `SetExpr::increment("col", 1)?` | `SET col = col + 1` |
| `SetExpr::increment("col", -3)?` | `SET col = col + (-3)` (decrement) |

### Multiple SET clauses

```rust
let affected = sql("orders")
    .update_many([
        SetExpr::set("status", "shipped")?,
        SetExpr::raw("shipped_at = NOW()"),
    ])?
    .filter(Condition::eq_any("id", vec![1_i64, 2, 3])?)
    .execute(&client)
    .await?;
```

### Bulk update with RETURNING

```rust
use pgorm::{Condition, SetExpr, sql};

let updated: Vec<User> = sql("users")
    .update_many([SetExpr::increment("login_count", 1)?])?
    .filter(Condition::eq("status", "active")?)
    .returning(&client)
    .await?;
```

### `.filter()` for WHERE

Chain `.filter()` calls to add AND conditions:

```rust
let builder = sql("audit_logs")
    .delete_many()?
    .filter(Condition::eq("level", "debug")?)
    .filter(Condition::eq("archived", true)?);
```

## 3. Bulk Delete

Use `sql("table").delete_many()` to delete rows matching conditions.

### Basic bulk delete

```rust
use pgorm::{WhereExpr, sql};

let deleted = sql("sessions")
    .delete_many()?
    .filter(WhereExpr::raw("expires_at < NOW()"))
    .execute(&client)
    .await?;
```

### Bulk delete with RETURNING

```rust
use pgorm::{Condition, sql};

let deleted_users: Vec<User> = sql("users")
    .delete_many()?
    .filter(Condition::eq("status", "banned")?)
    .returning(&client)
    .await?;
```

### Safety: must have filter or explicit `.all_rows()`

Bulk update and delete operations require either a `.filter()` or an explicit `.all_rows()` call. Without either, `build_sql()` (and therefore `execute()`) returns an error:

```rust
// This will return an error -- no filter provided
let builder = sql("users")
    .update_many([SetExpr::set("status", "x")?])?;
builder.build_sql(); // Err: "update without WHERE clause"

// Explicit opt-in to update all rows
let builder = sql("temp_data")
    .update_many([SetExpr::set("status", "archived")?])?
    .all_rows();
builder.execute(&client).await?; // OK

// Same for delete
let builder = sql("temp_data")
    .delete_many()?
    .all_rows();
builder.execute(&client).await?; // OK
```

This safety check prevents accidental full-table updates or deletes.

## Next

- Next: [Transactions & Savepoints](/en/guide/transactions)
