# 高级查询：CTE 与批量操作

本页介绍 pgorm 对公用表表达式（WITH 查询）、批量更新和批量删除的支持。

## 1. CTE（WITH）查询

CTE 允许你定义命名的子查询，然后在主 SELECT 中引用。pgorm 的 `sql()` 构建器支持简单 CTE、多个 CTE、带列名的 CTE、递归 CTE 以及数据修改 CTE。

### 简单 CTE

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

这会生成：

```sql
WITH active_users AS (
  SELECT id, name FROM users WHERE status = $1
)
SELECT * FROM active_users
```

### 多个 CTE

链式调用 `.with()` 来定义多个 CTE：

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

### 带列名的 CTE：`with_columns`

当 CTE 需要显式的列别名时：

```rust
let q = sql("")
    .with_columns(
        "monthly_sales",
        ["month", "total"],
        sql("SELECT DATE_TRUNC('month', created_at), SUM(amount) FROM orders GROUP BY 1"),
    )?
    .select(sql("SELECT * FROM monthly_sales WHERE total > ").bind(10000_i64));
```

这会生成：

```sql
WITH monthly_sales(month, total) AS (
  SELECT DATE_TRUNC('month', created_at), SUM(amount) FROM orders GROUP BY 1
)
SELECT * FROM monthly_sales WHERE total > $1
```

### 递归 CTE：`with_recursive`

用于层级数据（组织架构、分类树等）：

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
        // 基础情况：根节点
        sql(
            "SELECT id, name, parent_id, 0 as level, name::text as path \
             FROM employees WHERE parent_id IS NULL",
        ),
        // 递归步骤
        sql(
            "SELECT e.id, e.name, e.parent_id, t.level + 1, t.path || ' > ' || e.name \
             FROM employees e JOIN org_tree t ON e.parent_id = t.id",
        ),
    )?
    .select(sql("SELECT * FROM org_tree ORDER BY path"))
    .fetch_all_as(&client)
    .await?;
```

带参数的递归 CTE（例如深度限制）：

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

### `select_from` 简写

可以用 `.select_from("cte_name")?` 代替 `.select(sql("SELECT * FROM cte_name"))`，它会验证标识符：

```rust
let q = sql("")
    .with_recursive("reachable", base, recursive)?
    .select_from("reachable")?;
```

### 带 UNION 的递归 CTE（去重）

默认情况下，`with_recursive` 使用 `UNION ALL`。要对行进行去重（在图遍历时很有用），使用 `with_recursive_union`：

```rust
let q = sql("")
    .with_recursive_union(
        "reachable",
        sql("SELECT end_node FROM edges WHERE start_node = ").bind(1_i64),
        sql("SELECT e.end_node FROM edges e JOIN reachable r ON e.start_node = r.end_node"),
    )?
    .select_from("reachable")?;
```

### 数据修改 CTE

CTE 可以包含带 RETURNING 的 INSERT、UPDATE 或 DELETE，将结果传递到主查询：

```rust
let mut delete_sql = sql("DELETE FROM orders WHERE status = 'cancelled' AND created_at < ");
delete_sql.push_bind("2024-01-01");
delete_sql.push(" RETURNING *");

let q = sql("")
    .with("deleted_orders", delete_sql)?
    .select(sql("INSERT INTO orders_archive SELECT * FROM deleted_orders"));

q.execute(&client).await?;
```

## 2. 批量更新

使用 `sql("table").update_many([...])` 来更新匹配条件的多行数据。

### 基本批量更新

```rust
use pgorm::{Condition, SetExpr, sql};

let affected = sql("users")
    .update_many([SetExpr::set("status", "inactive")?])?
    .filter(Condition::lt("login_count", 10_i32)?)
    .execute(&client)
    .await?;

println!("Deactivated {affected} users");
```

### `SetExpr` 变体

| 方法 | 效果 |
|------|------|
| `SetExpr::set("col", value)?` | `SET col = $N` |
| `SetExpr::raw("updated_at = NOW()")` | 原始 SQL 表达式 |
| `SetExpr::increment("col", 1)?` | `SET col = col + 1` |
| `SetExpr::increment("col", -3)?` | `SET col = col + (-3)`（递减） |

### 多个 SET 子句

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

### 带 RETURNING 的批量更新

```rust
use pgorm::{Condition, SetExpr, sql};

let updated: Vec<User> = sql("users")
    .update_many([SetExpr::increment("login_count", 1)?])?
    .filter(Condition::eq("status", "active")?)
    .returning(&client)
    .await?;
```

### `.filter()` 用于 WHERE

链式调用 `.filter()` 来添加 AND 条件：

```rust
let builder = sql("audit_logs")
    .delete_many()?
    .filter(Condition::eq("level", "debug")?)
    .filter(Condition::eq("archived", true)?);
```

## 3. 批量删除

使用 `sql("table").delete_many()` 来删除匹配条件的行。

### 基本批量删除

```rust
use pgorm::{WhereExpr, sql};

let deleted = sql("sessions")
    .delete_many()?
    .filter(WhereExpr::raw("expires_at < NOW()"))
    .execute(&client)
    .await?;
```

### 带 RETURNING 的批量删除

```rust
use pgorm::{Condition, sql};

let deleted_users: Vec<User> = sql("users")
    .delete_many()?
    .filter(Condition::eq("status", "banned")?)
    .returning(&client)
    .await?;
```

### 安全性：必须有过滤条件或显式 `.all_rows()`

批量更新和删除操作要求有 `.filter()` 或显式的 `.all_rows()` 调用。如果两者都没有，`build_sql()`（以及 `execute()`）会返回错误：

```rust
// 这会返回错误 -- 没有提供过滤条件
let builder = sql("users")
    .update_many([SetExpr::set("status", "x")?])?;
builder.build_sql(); // Err: "update without WHERE clause"

// 显式声明更新所有行
let builder = sql("temp_data")
    .update_many([SetExpr::set("status", "archived")?])?
    .all_rows();
builder.execute(&client).await?; // OK

// 删除同理
let builder = sql("temp_data")
    .delete_many()?
    .all_rows();
builder.execute(&client).await?; // OK
```

这个安全检查可以防止意外的全表更新或删除。

## 下一步

- 下一章：[事务与保存点](/zh/guide/transactions)
