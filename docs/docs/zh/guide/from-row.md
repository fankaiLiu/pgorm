# 行映射：`FromRow` / `RowExt` / JSONB

pgorm 的核心理念是“SQL-first”，因此它把“查询”与“映射”解耦：

- 你可以手写 SQL（`query()` / `sql()`）
- 再把 `tokio_postgres::Row` 映射成 Rust 类型

这一页讲两种常用方式：

1) `#[derive(FromRow)]`：把一行映射成结构体（推荐）  
2) `RowExt`：手动按列名取值（用于快速脚本/调试/特殊场景）  

## 1) `#[derive(FromRow)]`：最常用方式

```rust
use pgorm::FromRow;

#[derive(Debug, FromRow)]
struct User {
    id: i64,
    username: String,
    email: Option<String>, // NULL -> None
}
```

然后你就可以用 `fetch_*_as`：

```rust
use pgorm::query;

let users: Vec<User> = query("SELECT id, username, email FROM users ORDER BY id")
    .fetch_all_as(&client)
    .await?;
```

### 字段名与列名不一致：`#[orm(column = \"...\")]`

当 SQL 的列名和 Rust 字段名不同（或你做了别名），可以显式指定：

```rust
use pgorm::FromRow;

#[derive(Debug, FromRow)]
struct User {
    id: i64,
    #[orm(column = "user_name")]
    username: String,
}
```

## 2) `RowExt`：按列名取值（“逃生舱”）

```rust
use pgorm::{RowExt, query};

let row = query("SELECT id, username FROM users WHERE id = $1")
    .bind(1_i64)
    .fetch_one(&client)
    .await?;

let id: i64 = row.try_get_column("id")?;
let username: String = row.try_get_column("username")?;
```

`try_get_column` 会把 tokio-postgres 的错误转成 `OrmError::Decode`，便于统一处理。

## 3) JSONB：强类型与弱类型两种写法

### 强类型：`Json<T>`

```rust
use pgorm::Json;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Meta {
    tags: Vec<String>,
    active: bool,
}

// 查询时：列类型是 jsonb
let meta: Json<Meta> = row.try_get_column("meta")?;
println!("{:?}", meta.0);
```

### 弱类型：`serde_json::Value`

```rust
let v: serde_json::Value = row.try_get_column("meta")?;
println!("{v}");
```

> 可运行示例见 `crates/pgorm/examples/jsonb`。

## 4) 小建议：SQL 写法配合映射更稳

1) 尽量 **显式列出字段**（不要随手 `SELECT *`），避免 schema 改动导致映射不稳定。  
2) JOIN/聚合时，给列起清晰别名（例如 `SELECT u.id AS user_id ...`），再用 `#[orm(column="user_id")]` 映射。  

## 下一步

- 下一章：[`模型与派生宏`](/zh/guide/models)
