# CTE（WITH 子句）查询支持设计与计划

状态：Draft
相关代码：`crates/pgorm/src/sql.rs` / `crates/pgorm/src/builder.rs`
最后更新：2026-02-05

## 背景

CTE（Common Table Expression，公用表表达式）是 PostgreSQL 的强大特性，可用于：

1. **递归查询**：遍历树形/图形结构（如组织架构、分类树）
2. **复杂查询分解**：将复杂查询拆分为可读的子步骤
3. **数据修改 CTE**：在 CTE 中执行 INSERT/UPDATE/DELETE 并在主查询中使用结果

当前 `pgorm` 需要用户手写完整的 CTE SQL，缺乏类型安全的构建器支持。

## 目标 / 非目标

### 目标

1. 提供 `with()` 方法声明非递归 CTE。
2. 提供 `with_recursive()` 方法支持递归 CTE。
3. CTE 名称通过 `Ident` 校验，防止注入。
4. 支持在 CTE 中引用参数绑定。
5. 主查询可以引用 CTE 结果。

### 非目标

- CTE 的完全类型推断（由于 SQL 的动态性，保持显式）。
- 嵌套 CTE（WITH 内部再 WITH）。
- MATERIALIZED / NOT MATERIALIZED 提示（可后续扩展）。

## 方案

### 1) 基础 CTE 构建器

```rust,ignore
impl Sql {
    /// 添加 CTE（WITH 子句）
    pub fn with(self, name: impl Into<Ident>, query: Sql) -> WithBuilder;

    /// 添加递归 CTE
    pub fn with_recursive(self, name: impl Into<Ident>, query: Sql) -> WithBuilder;
}

pub struct WithBuilder {
    ctes: Vec<CteDefinition>,
    is_recursive: bool,
}

struct CteDefinition {
    name: Ident,
    columns: Option<Vec<Ident>>,  // 可选：指定列名
    query: Sql,
}

impl WithBuilder {
    /// 添加另一个 CTE
    pub fn with(self, name: impl Into<Ident>, query: Sql) -> Self;

    /// 添加带列名的 CTE
    pub fn with_columns(
        self,
        name: impl Into<Ident>,
        columns: impl IntoIterator<Item = impl Into<Ident>>,
        query: Sql,
    ) -> Self;

    /// 设置主查询
    pub fn select(self, query: Sql) -> Sql;

    /// 从 CTE 中 SELECT
    pub fn select_from(self, cte_name: impl Into<Ident>) -> Sql;
}
```

### 2) 递归 CTE 构建器

递归 CTE 有特定结构：基础情况 + UNION [ALL] + 递归情况

```rust,ignore
impl Sql {
    /// 构建递归 CTE
    pub fn with_recursive(
        self,
        name: impl Into<Ident>,
        base: Sql,           // 基础查询（锚点）
        recursive: Sql,      // 递归查询（引用 CTE 自身）
    ) -> WithBuilder;
}

// 或者使用专门的递归 CTE 构建器
pub struct RecursiveCteBuilder {
    name: Ident,
    base_query: Sql,
}

impl RecursiveCteBuilder {
    /// 设置递归部分（UNION ALL）
    pub fn union_all(self, recursive_query: Sql) -> CteDefinition;

    /// 设置递归部分（UNION，去重）
    pub fn union(self, recursive_query: Sql) -> CteDefinition;
}
```

## 使用示例

### A) 简单 CTE

```rust,ignore
use pgorm::prelude::*;

#[derive(FromRow)]
struct UserSummary {
    user_id: i64,
    total_orders: i64,
    total_amount: Decimal,
}

// WITH order_stats AS (
//     SELECT user_id, COUNT(*) as total_orders, SUM(amount) as total_amount
//     FROM orders
//     WHERE status = 'completed'
//     GROUP BY user_id
// )
// SELECT * FROM order_stats WHERE total_amount > 1000

let high_value_users: Vec<UserSummary> = pgorm::sql("")
    .with(
        "order_stats",
        pgorm::sql("SELECT user_id, COUNT(*) as total_orders, SUM(amount) as total_amount FROM orders WHERE status = $1 GROUP BY user_id")
            .bind("completed")
    )
    .select(pgorm::sql("SELECT * FROM order_stats WHERE total_amount > $1").bind(1000_i64))
    .fetch_all(&client)
    .await?;
```

### B) 多个 CTE

```rust,ignore
// WITH
//   active_users AS (SELECT id FROM users WHERE status = 'active'),
//   recent_orders AS (SELECT * FROM orders WHERE created_at > $1)
// SELECT u.id, COUNT(o.id) as order_count
// FROM active_users u
// JOIN recent_orders o ON o.user_id = u.id
// GROUP BY u.id

let results = pgorm::sql("")
    .with("active_users", pgorm::sql("SELECT id FROM users WHERE status = $1").bind("active"))
    .with("recent_orders", pgorm::sql("SELECT * FROM orders WHERE created_at > $1").bind(last_week))
    .select(pgorm::sql("
        SELECT u.id, COUNT(o.id) as order_count
        FROM active_users u
        JOIN recent_orders o ON o.user_id = u.id
        GROUP BY u.id
    "))
    .fetch_all::<UserOrderCount>(&client)
    .await?;
```

### C) 递归 CTE - 组织架构树

```rust,ignore
#[derive(FromRow)]
struct OrgNode {
    id: i64,
    name: String,
    parent_id: Option<i64>,
    level: i32,
    path: String,
}

// WITH RECURSIVE org_tree AS (
//   -- Base case: root nodes
//   SELECT id, name, parent_id, 0 as level, name::text as path
//   FROM employees
//   WHERE parent_id IS NULL
//
//   UNION ALL
//
//   -- Recursive case: children
//   SELECT e.id, e.name, e.parent_id, t.level + 1, t.path || ' > ' || e.name
//   FROM employees e
//   JOIN org_tree t ON e.parent_id = t.id
// )
// SELECT * FROM org_tree ORDER BY path

let org_tree: Vec<OrgNode> = pgorm::sql("")
    .with_recursive(
        "org_tree",
        // Base case
        pgorm::sql("SELECT id, name, parent_id, 0 as level, name::text as path FROM employees WHERE parent_id IS NULL"),
        // Recursive case
        pgorm::sql("SELECT e.id, e.name, e.parent_id, t.level + 1, t.path || ' > ' || e.name FROM employees e JOIN org_tree t ON e.parent_id = t.id"),
    )
    .select(pgorm::sql("SELECT * FROM org_tree ORDER BY path"))
    .fetch_all(&client)
    .await?;

for node in org_tree {
    let indent = "  ".repeat(node.level as usize);
    println!("{}{} (ID: {})", indent, node.name, node.id);
}
```

### D) 递归 CTE - 分类树（带深度限制）

```rust,ignore
#[derive(FromRow)]
struct Category {
    id: i64,
    name: String,
    parent_id: Option<i64>,
    depth: i32,
}

// WITH RECURSIVE category_tree AS (
//   SELECT id, name, parent_id, 0 as depth
//   FROM categories WHERE id = $1
//
//   UNION ALL
//
//   SELECT c.id, c.name, c.parent_id, ct.depth + 1
//   FROM categories c
//   JOIN category_tree ct ON c.parent_id = ct.id
//   WHERE ct.depth < 5  -- 限制递归深度
// )
// SELECT * FROM category_tree

let root_category_id = 1_i64;
let max_depth = 5_i32;

let tree: Vec<Category> = pgorm::sql("")
    .with_recursive(
        "category_tree",
        pgorm::sql("SELECT id, name, parent_id, 0 as depth FROM categories WHERE id = $1")
            .bind(root_category_id),
        pgorm::sql("SELECT c.id, c.name, c.parent_id, ct.depth + 1 FROM categories c JOIN category_tree ct ON c.parent_id = ct.id WHERE ct.depth < $1")
            .bind(max_depth),
    )
    .select_from("category_tree")
    .fetch_all(&client)
    .await?;
```

### E) CTE 用于数据修改

```rust,ignore
// WITH deleted_orders AS (
//   DELETE FROM orders
//   WHERE status = 'cancelled' AND created_at < $1
//   RETURNING *
// )
// INSERT INTO deleted_orders_archive
// SELECT * FROM deleted_orders

let archived_count: u64 = pgorm::sql("")
    .with(
        "deleted_orders",
        pgorm::sql("DELETE FROM orders WHERE status = 'cancelled' AND created_at < $1 RETURNING *")
            .bind(one_year_ago)
    )
    .select(pgorm::sql("INSERT INTO deleted_orders_archive SELECT * FROM deleted_orders"))
    .execute(&client)
    .await?;

println!("Archived {} cancelled orders", archived_count);
```

### F) 带列名的 CTE

```rust,ignore
// WITH monthly_sales(month, total) AS (
//   SELECT DATE_TRUNC('month', created_at), SUM(amount)
//   FROM orders
//   GROUP BY DATE_TRUNC('month', created_at)
// )
// SELECT * FROM monthly_sales WHERE total > 10000

let results = pgorm::sql("")
    .with_columns(
        "monthly_sales",
        ["month", "total"],
        pgorm::sql("SELECT DATE_TRUNC('month', created_at), SUM(amount) FROM orders GROUP BY DATE_TRUNC('month', created_at)")
    )
    .select(pgorm::sql("SELECT * FROM monthly_sales WHERE total > $1").bind(10000_i64))
    .fetch_all::<MonthlySales>(&client)
    .await?;
```

## 生成的 SQL

### 简单 CTE

```sql
WITH order_stats AS (
    SELECT user_id, COUNT(*) as total_orders, SUM(amount) as total_amount
    FROM orders
    WHERE status = $1
    GROUP BY user_id
)
SELECT * FROM order_stats WHERE total_amount > $2
```

### 递归 CTE

```sql
WITH RECURSIVE org_tree AS (
    SELECT id, name, parent_id, 0 as level, name::text as path
    FROM employees
    WHERE parent_id IS NULL

    UNION ALL

    SELECT e.id, e.name, e.parent_id, t.level + 1, t.path || ' > ' || e.name
    FROM employees e
    JOIN org_tree t ON e.parent_id = t.id
)
SELECT * FROM org_tree ORDER BY path
```

## API 设计

### 链式调用流程

```
pgorm::sql("")
    .with("cte1", query1)           // 第一个 CTE
    .with("cte2", query2)           // 第二个 CTE（可选）
    .with_recursive("tree", base, recursive)  // 递归 CTE（可选）
    .select(main_query)             // 主查询
    .fetch_all::<T>(&client)        // 执行
```

### 参数绑定的处理

CTE 和主查询共享参数序列：

```rust,ignore
// CTE 中的 $1，主查询中的 $2
pgorm::sql("")
    .with("stats", pgorm::sql("SELECT ... WHERE status = $1").bind("active"))
    .select(pgorm::sql("SELECT ... WHERE amount > $1").bind(1000))
```

内部会重新编号参数：CTE 的 `$1` -> `$1`，主查询的 `$1` -> `$2`。

## 关键取舍

| 选项 | 优点 | 缺点 | 决策 |
|------|------|------|------|
| 完全类型化 vs SQL 字符串 | 更安全 | 实现复杂，不灵活 | **SQL 字符串 + 名称校验** |
| 自动参数重编号 vs 用户手动 | 更易用 | 实现复杂 | **自动重编号** |
| 专用递归构建器 vs 通用方法 | 更清晰 | API 增多 | **专用方法** |

## 与现有功能的关系

- **Sql builder**：CTE 返回的仍是 `Sql` 类型，可以继续链式调用。
- **监控**：CTE 查询通过 `QueryMonitor` 记录完整 SQL。
- **检查**：`pgorm-check` 可以解析和验证 CTE 语法。

## 兼容性与迁移

- 纯新增 API，不影响现有 `Sql` 的行为。
- 用户可以继续使用 `pgorm::query()` 写原生 CTE SQL。

## 里程碑 / TODO

### M1（基础 CTE）

- [ ] `with()` 方法
- [ ] `WithBuilder` 类型
- [ ] 多 CTE 支持
- [ ] 参数重编号逻辑
- [ ] 单元测试

### M2（递归 CTE）

- [ ] `with_recursive()` 方法
- [ ] UNION / UNION ALL 支持
- [ ] 递归深度限制的文档说明
- [ ] 集成测试

### M3（扩展功能）

- [ ] `with_columns()` 带列名的 CTE
- [ ] MATERIALIZED / NOT MATERIALIZED 提示
- [ ] `examples/cte_queries`
- [ ] 中英文文档

## Open Questions

1. 参数重编号是在 `build()` 时还是 `with()` 时处理？（建议 `build()` 时统一处理）
2. 是否支持 CTE 的类型推断（如 `with::<MyStruct>("name", query)`）？（建议不支持，保持简单）
3. 递归 CTE 是否需要提供深度限制的便捷方法？（建议文档说明，由用户在 SQL 中添加 WHERE 条件）
4. 是否支持 `INSERT ... WITH` / `UPDATE ... WITH`？（建议 M2 后扩展）
