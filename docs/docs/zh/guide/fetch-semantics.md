# Fetch 语义：该用哪个 `fetch_*`？

pgorm 把“执行 SQL”拆成两件事：

1) 你写 SQL（`query()` 或 `sql()`）  
2) 你选择“期望返回的行数语义”（`fetch_one` / `fetch_one_strict` / `fetch_opt` …）

这一页只讲第二件事：**什么时候用哪个 `fetch_*`**。

## 1) 三个最常用的选择

### `fetch_one*`：取第一行（多行不报错）

- 0 行：`Err(OrmError::NotFound(..))`
- 1 行：OK
- 多行：**返回第一行**（不报错）

适合：你明确 `ORDER BY ... LIMIT 1`，或者你就是想要“第一条”。

### `fetch_one_strict*`：要求恰好 1 行

- 0 行：`Err(OrmError::NotFound(..))`
- 1 行：OK
- 多行：`Err(OrmError::TooManyRows { .. })`

适合：按唯一键查询、按主键查询、你希望“数据不一致时立刻失败”。

### `fetch_opt*`：可选返回

- 0 行：`Ok(None)`
- 1 行：`Ok(Some(..))`
- 多行：返回第一行（同 `fetch_one` 的“非严格”语义）

适合：查询“可能存在也可能不存在”的资源。

## 2) 例子：同一条 SQL 的三种读法

```rust
use pgorm::{OrmError, RowExt, query};

// 预期：可能会返回多行（比如 name 不唯一）
let row = query("SELECT id, name FROM items WHERE name = $1 ORDER BY id")
    .bind("dup")
    .fetch_one(&client)
    .await?;
let id: i64 = row.try_get_column("id")?;
println!("fetch_one => id={id}");

// 预期：必须恰好 1 行（否则报错）
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

// 预期：可能不存在
let maybe = query("SELECT id FROM items WHERE id = $1")
    .bind(9999_i64)
    .fetch_opt(&client)
    .await?;
println!("fetch_opt => {}", if maybe.is_some() { "Some" } else { "None" });
```

> 上面示例来自 `crates/pgorm/examples/fetch_semantics`，你可以直接运行它验证行为。

## 3) `*_as`：直接映射成结构体

只要你的类型实现了 `FromRow`（通常用 `#[derive(FromRow)]`），就可以用 `*_as`：

```rust
let user: User = query("SELECT id, username FROM users WHERE id = $1")
    .bind(1_i64)
    .fetch_one_as(&client)
    .await?;

let users: Vec<User> = query("SELECT id, username FROM users ORDER BY id")
    .fetch_all_as(&client)
    .await?;
```

映射细节见：[`行映射：FromRow`](/zh/guide/from-row)。

## 4) 标量：`fetch_scalar_*`

当你只关心第一列（COUNT / MAX / EXISTS 等），用标量 API：

```rust
let count: i64 = query("SELECT COUNT(*) FROM users").fetch_scalar_one(&client).await?;
let maybe: Option<i64> = query("SELECT MAX(id) FROM users").fetch_scalar_opt(&client).await?;
let all_ids: Vec<i64> = query("SELECT id FROM users ORDER BY id").fetch_scalar_all(&client).await?;
```

## 5) 一句话选型

- “我要第一条”：`fetch_one`（最好配合 `ORDER BY ... LIMIT 1`）  
- “必须唯一，否则就是 bug”：`fetch_one_strict`  
- “可能不存在”：`fetch_opt`  
- “只要一个数”：`fetch_scalar_*`  

## 下一步

- 下一章：[`行映射：FromRow`](/zh/guide/from-row)
