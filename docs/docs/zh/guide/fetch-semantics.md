# Fetch 语义与流式查询

在 pgorm 中，"执行查询"分为两个选择：

1. 如何构建 SQL（`query()` 或 `sql()`）
2. 你期望的行数语义（`fetch_one` / `fetch_one_strict` / `fetch_opt` ...）

本页详细介绍第 (2) 点，以及用于大结果集的流式查询。

## 1. 选择 Fetch 方法

### `fetch_one` / `fetch_one_as::<T>`：第一行（非严格）

- 0 行：`Err(OrmError::NotFound(..))`
- 1 行：OK
- 2+ 行：返回第一行（不报错）

适用于你明确想要"第一行"的场景（最好配合 `ORDER BY ... LIMIT 1`）。

```rust
use pgorm::{RowExt, query};

let row = query("SELECT id, name FROM items WHERE name = $1 ORDER BY id")
    .bind("dup")
    .fetch_one(&client)
    .await?;
let id: i64 = row.try_get_column("id")?;
```

### `fetch_one_strict` / `fetch_one_strict_as::<T>`：恰好一行

- 0 行：`Err(OrmError::NotFound(..))`
- 1 行：OK
- 2+ 行：`Err(OrmError::TooManyRows { expected, got })`

适用于按唯一键/主键查询时，你希望数据不一致时立即报错。

```rust
match query("SELECT id, name FROM items WHERE name = $1 ORDER BY id")
    .bind("dup")
    .fetch_one_strict(&client)
    .await
{
    Ok(row) => { /* 恰好一行 */ }
    Err(OrmError::TooManyRows { expected, got }) => {
        println!("Expected {expected} row, got {got}");
    }
    Err(e) => return Err(e),
}
```

### `fetch_opt` / `fetch_opt_as::<T>`：可选行

- 0 行：`Ok(None)`
- 1 行：`Ok(Some(..))`
- 2+ 行：返回第一行作为 `Some(..)`（非严格）

适用于记录可能存在也可能不存在的场景。

```rust
let maybe_row = query("SELECT id FROM items WHERE id = $1")
    .bind(9999_i64)
    .fetch_opt(&client)
    .await?;
// 如果没有匹配的行，maybe_row 为 None
```

### `fetch_all` / `fetch_all_as::<T>`：所有行

返回所有匹配的行。空结果为 `Ok(vec![])`，不是错误。

```rust
let users: Vec<User> = query("SELECT id, username FROM users ORDER BY id")
    .fetch_all_as(&client)
    .await?;
```

### `*_as::<T>` 结构体映射

任何实现了 `FromRow`（通常通过 `#[derive(FromRow)]`）的类型都可以用于 `*_as` 变体：

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

let maybe_user: Option<User> = query("SELECT id, username FROM users WHERE id = $1")
    .bind(2_i64)
    .fetch_opt_as(&client)
    .await?;
```

## 2. 标量辅助方法

当你只关心结果的第一列（COUNT、MAX 等）时，使用标量方法：

```rust
// 第一行的单个标量值（0 行时报错）
let count: i64 = query("SELECT COUNT(*) FROM users")
    .fetch_scalar_one(&client)
    .await?;

// 严格标量值：要求恰好一行。
let strict_count: i64 = query("SELECT COUNT(*) FROM users")
    .fetch_scalar_one_strict(&client)
    .await?;

// 可选标量（0 行 = None）
let maybe_max: Option<i64> = query("SELECT MAX(id) FROM users")
    .fetch_scalar_opt(&client)
    .await?;

// 所有标量作为 Vec
let all_ids: Vec<i64> = query("SELECT id FROM users ORDER BY id")
    .fetch_scalar_all(&client)
    .await?;
```

### `exists()`

一个便捷方法，用于检查是否有匹配的行：

```rust
let has_active: bool = query("SELECT 1 FROM users WHERE status = $1")
    .bind("active")
    .exists(&client)
    .await?;
```

## 3. 快速选择指南

| 我想要... | 使用 |
|-----------|------|
| 第一行 | `fetch_one` |
| 恰好一行（唯一键） | `fetch_one_strict` |
| 可能不存在的行 | `fetch_opt` |
| 所有匹配的行 | `fetch_all` |
| 只要一个数字（COUNT、MAX、...） | `fetch_scalar_one` |
| 恰好一行的数字 | `fetch_scalar_one_strict` |
| 可能为 NULL 的数字 | `fetch_scalar_opt` |
| 检查是否存在 | `exists()` |
| 将行映射为结构体 | 在任何 fetch 方法后加 `_as::<T>` |

## 4. 流式查询

对于大结果集，当你不想一次性将所有数据加载到内存时，使用 `stream_as::<T>()`。行会在 PostgreSQL 发送时逐条到达。

### 基本流式查询

```rust
use futures_util::StreamExt;
use pgorm::{FromRow, query};

#[derive(Debug, FromRow)]
struct Item {
    n: i64,
}

let mut stream = query("SELECT generate_series(1, $1) AS n")
    .bind(10_i64)
    .tag("examples.streaming")
    .stream_as::<Item>(&client)
    .await?;

while let Some(item) = stream.next().await {
    let item = item?;
    println!("{}", item.n);
}
```

### 背压

`stream_as::<T>()` 返回一个实现了 `futures::Stream` 的 `FromRowStream<T>`。行会在 PostgreSQL 发送时到达。如果你的消费者处理较慢，PostgreSQL 的 TCP 流控会提供自然的背压 -- 数据库会暂停发送行，直到你消费它们。

### 何时使用流式查询

- 处理数百万行的数据导出或 ETL
- 聚合数据但不想将所有行保留在内存中
- 任何 `fetch_all` 会占用过多内存的场景

对于大多数返回有限行数的查询（例如带 `LIMIT` 的查询），`fetch_all_as` 更简单且足够用。

## 下一步

- 下一章：[高级查询：CTE 与批量操作](/zh/guide/advanced-queries)
