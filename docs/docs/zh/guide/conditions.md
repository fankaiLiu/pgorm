# 动态条件与分页

写“动态 SQL”时，最容易踩坑的两类点：

1) **WHERE 条件组合**（AND/OR/NOT、可选条件、括号优先级）  
2) **ORDER BY / LIMIT / OFFSET**（排序字段来自用户输入时的注入风险）

pgorm 提供了一组结构化工具来解决这些问题：

- `Condition` / `Op`：条件原子（会校验列名标识符）
- `WhereExpr`：布尔表达式树（负责括号与优先级）
- `OrderBy`：安全的 ORDER BY 生成（会校验列名标识符）
- `Pagination`：LIMIT/OFFSET（参数绑定）

## 1) 用 `WhereExpr` 组装可选 WHERE

一个实用技巧：用 `WhereExpr::and(Vec::new())` 作为起点，它表示恒真（`TRUE`），方便不断 `and_with(...)` 追加条件。

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

`WhereExpr` 会在需要时自动加括号，保证 AND/OR 的优先级正确。

## 2) 常用 `Condition`（你大概率会用到）

```rust
use pgorm::{Condition, Op};

let c1 = Condition::eq("status", "active")?;
let c2 = Condition::ne("role", "banned")?;
let c3 = Condition::ilike("name", "%alice%")?;
let c4 = Condition::is_null("deleted_at")?;
let c5 = Condition::new("id", Op::between(1_i64, 100_i64))?;
let c6 = Condition::new("role", Op::in_list(vec!["admin", "owner"]))?;
```

这里最关键的是：**列名是标识符，会被校验**。如果你把不合法的字符串当列名传进去，会得到 `OrmError::Validation`，从而避免“用户输入变成 SQL 注入”。

## 3) OR 组：表达 (A AND (B OR C))

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

## 4) 安全的动态排序：`OrderBy`

不要自己拼 `ORDER BY {user_input}`。用 `OrderBy`（会校验标识符）：

```rust
use pgorm::{NullsOrder, OrderBy, Pagination, SortDir, sql};

let mut q = sql("SELECT id, username, created_at FROM users");

let mut order = OrderBy::new()
    .with_nulls("created_at", SortDir::Desc, NullsOrder::Last)?;

if let Some(sort_by) = sort_by {
    // sort_by 可能来自用户输入：这里会进行标识符校验
    order = order.asc(sort_by.as_str())?;
}

order.append_to_sql(&mut q);
Pagination::page(1, 20)?.append_to_sql(&mut q);
```

## 5) 分页：`Pagination`

`Pagination` 会生成 `LIMIT $n OFFSET $m` 并把值作为参数绑定：

```rust
use pgorm::{Pagination, sql};

let mut q = sql("SELECT * FROM users");
Pagination::page(page, per_page)?.append_to_sql(&mut q);
```

> `page` 从 1 开始；`per_page` 目前不会自动校验，你可以在业务侧限制范围（比如 1..=200）。

## 下一步

- 下一章：[`Fetch 语义`](/zh/guide/fetch-semantics)
