# 动态过滤与分页

编写动态 SQL 时，有两个常见的容易出错的地方：

1. 正确组装 WHERE 逻辑（AND/OR/NOT + 括号）
2. 安全构建 ORDER BY / LIMIT / OFFSET（特别是当列名来自用户输入时）

pgorm 为这两者提供了结构化的构建器：

- `Condition` / `Op`：原子谓词（会验证标识符）
- `WhereExpr`：布尔表达式树（处理分组/括号）
- `OrderBy`：安全的 ORDER BY 构建器（会验证标识符）
- `Pagination`：基于页码的 LIMIT/OFFSET
- `Keyset1` / `Keyset2`：基于游标的键集分页

## 1. `Condition` -- 原子谓词

`Condition` 表示单个比较条件。所有标识符都会被验证 -- 通过列名进行 SQL 注入是不可能的。

### 比较运算符

```rust
use pgorm::{Condition, Op};

// 相等 / 不等
let c1 = Condition::eq("status", "active")?;
let c2 = Condition::ne("role", "banned")?;

// 比较
let c3 = Condition::gt("age", 18_i32)?;
let c4 = Condition::gte("score", 90_i32)?;
let c5 = Condition::lt("login_count", 10_i32)?;
let c6 = Condition::lte("price", 9999_i64)?;

// 模式匹配
let c7 = Condition::like("name", "Alice%")?;
let c8 = Condition::ilike("name", "%test%")?;      // 不区分大小写
let c9 = Condition::not_like("name", "%spam%")?;

// 空值检查
let c10 = Condition::is_null("deleted_at")?;
let c11 = Condition::is_not_null("email")?;

// 列表和范围
let c12 = Condition::new("role", Op::in_list(vec!["admin", "owner"]))?;
let c13 = Condition::new("role", Op::not_in(vec!["banned", "suspended"]))?;
let c14 = Condition::new("id", Op::between(1_i64, 100_i64))?;

// 数组匹配：column = ANY($1)
let c15 = Condition::eq_any("id", vec![1_i64, 2, 3])?;
```

### 范围运算符（用于 PostgreSQL 范围类型）

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

## 2. `Op<T>` -- 用于编程式使用的运算符枚举

当你需要动态选择运算符时：

```rust
use pgorm::{Condition, Op};

let c = Condition::new("id", Op::between(1_i64, 100_i64))?;
let c = Condition::new("role", Op::in_list(vec!["admin", "owner"]))?;
```

## 3. `WhereExpr` -- 布尔表达式树

`WhereExpr` 将 `Condition` 值组合成带有正确括号化的 AND/OR/NOT 树。

### 构建可选的 WHERE 子句

从 `WhereExpr::and(Vec::new())` 开始 -- 这是一个恒真表达式。按条件添加过滤器：

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

`WhereExpr::and(Vec::new())` 没有子节点，因此 `is_trivially_true()` 返回 `true`。这在条件性追加 `WHERE` 时很有用 -- 如果没有添加任何过滤器，就完全跳过该子句。

### AND/OR/NOT 组合

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

这会生成：`(status = $1 AND (role = $2 OR role = $3) AND id BETWEEN $4 AND $5)`。

### Raw 逃生通道

对于结构化 API 无法覆盖的条件：

```rust
let expr = WhereExpr::raw("expires_at < NOW()");
```

### `append_to_sql(&mut sql)`

将完整的表达式（带有正确的括号和绑定参数）追加到 `Sql` 构建器中：

```rust
let mut q = sql("SELECT COUNT(*) FROM users WHERE ");
expr.append_to_sql(&mut q);
let count: i64 = q.fetch_scalar_one(&client).await?;
```

## 4. `OrderBy` -- 安全的 ORDER BY

不要直接编写 `ORDER BY {user_input}`。使用会验证标识符的 `OrderBy`：

```rust
use pgorm::{NullsOrder, OrderBy, SortDir, sql};

let mut order = OrderBy::new()
    .with_nulls("created_at", SortDir::Desc, NullsOrder::Last)?;

// 来自用户输入的动态列（经过验证）
if let Some(sort_by) = &filters.sort_by {
    order = order.asc(sort_by.as_str())?;
}

let mut q = sql("SELECT * FROM users");
order.append_to_sql(&mut q);
```

可用方法：

- `asc("column")?` -- 升序
- `desc("column")?` -- 降序
- `with_nulls("column", SortDir, NullsOrder)?` -- 显式 NULLS FIRST/LAST

## 5. 基于页码的分页：`Pagination::page(page, per_page)`

追加带有绑定参数的 `LIMIT $n OFFSET $m`：

```rust
use pgorm::{Pagination, sql};

let mut q = sql("SELECT * FROM users ORDER BY id");
Pagination::page(1, 20)?.append_to_sql(&mut q);
```

`page` 从 1 开始。在你的应用中限制 `per_page` 的范围（例如 1..=200）。

## 6. 键集分页：`Keyset1` 和 `Keyset2`

键集（游标）分页比 OFFSET 对大数据集更高效，因为它使用索引友好的 `WHERE` 子句，而不是跳过行。

### 单列排序：`Keyset1`

```rust
use pgorm::Keyset1;

let keyset = Keyset1::desc("id")?.limit(20);
// 第一页：没有游标
// 后续页：传入上一行的值
let keyset = keyset.after(last_id);
```

### 双列排序：`Keyset2`

当你需要一个平局决胜字段时使用（例如 `created_at DESC, id DESC`）：

```rust
use pgorm::{Condition, Keyset2, WhereExpr, sql};

let mut where_expr = WhereExpr::and(Vec::new());

if let Some(status) = &filters.status {
    where_expr = where_expr.and_with(Condition::eq("status", status.clone())?.into());
}

// 稳定排序：created_at DESC, id DESC
let mut keyset = Keyset2::desc("created_at", "id")?.limit(20);

// 后续页传入上一行的值作为游标
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

### 基于页码 vs. 键集分页

| | 基于页码（`Pagination`） | 键集（`Keyset1`/`Keyset2`） |
|---|---|---|
| **工作原理** | LIMIT + OFFSET | WHERE + LIMIT（索引友好） |
| **性能** | 随着偏移量增大而下降 | 无论翻到多深都保持恒定 |
| **跳转到第 N 页** | 支持 | 不支持（只能前进/后退） |
| **插入时的稳定性** | 否（行可能偏移） | 是（基于游标） |
| **最适用于** | 管理后台、小数据集 | 无限滚动、API、大数据集 |

## 下一步

- 下一章：[Fetch 语义与流式查询](/zh/guide/fetch-semantics)
