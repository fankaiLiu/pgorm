# 写入：`InsertModel`

`InsertModel` 是一组“写入辅助方法”的派生宏，目标是让你：

- **仍然显式写 SQL 模型**（写模型和读模型可以分开）
- 但插入时不用手写 INSERT 语句

常见用法是：一个 `Model` 负责读，一个 `InsertModel` 负责写。

## 1) 最小示例：插入并返回（RETURNING）

```rust
use pgorm::{FromRow, InsertModel, Model};

#[derive(Debug, FromRow, Model)]
#[orm(table = "products")]
struct Product {
    #[orm(id)]
    id: i64,
    sku: String,
    name: String,
}

#[derive(Debug, InsertModel)]
#[orm(table = "products", returning = "Product")]
struct NewProduct {
    sku: String,
    name: String,
}

let p: Product = NewProduct {
    sku: "SKU-001".into(),
    name: "Keyboard".into(),
}
.insert_returning(&client)
.await?;
```

要点：

- `#[orm(table = "...")]` 必填
- `#[orm(returning = "Product")]` 才会生成 `insert_returning`（否则只会生成不带 RETURNING 的版本）

## 2) 批量插入：`insert_many(_returning)`（UNNEST）

当你要插入很多行，`insert_many` 走 UNNEST 路径，性能更好：

```rust
let inserted: Vec<Product> = NewProduct::insert_many_returning(
    &client,
    vec![
        NewProduct { sku: "SKU-001".into(), name: "Keyboard".into() },
        NewProduct { sku: "SKU-002".into(), name: "Mouse".into() },
    ],
)
.await?;
```

> 可运行示例见 `crates/pgorm/examples/insert_many`。

## 3) 常用字段属性

### `#[orm(skip_insert)]`：永远不参与 INSERT

适合：只读字段、派生字段、由数据库触发器维护的字段。

### `#[orm(default)]`：使用 SQL `DEFAULT`

适合：数据库默认值（例如 `created_at DEFAULT now()`）。

### `#[orm(auto_now_add)]`：插入时自动填充“当前时间”

适合：你想用 Rust 侧的时间（而不是 DB `now()`）：

```rust
use chrono::{DateTime, Utc};
use pgorm::InsertModel;

#[derive(InsertModel)]
#[orm(table = "products")]
struct NewProduct {
    name: String,

    // 当为 None 时，pgorm 会在插入时用 `Utc::now()` 填充
    #[orm(auto_now_add)]
    created_at: Option<DateTime<Utc>>,
}
```

> `auto_now_add` 目前要求字段类型为 `Option<DateTime<Utc>>` 或 `Option<NaiveDateTime>`。

## 4) 什么时候不该用 InsertModel？

如果你的写入包含复杂的 CTE、INSERT...SELECT、多表 JOIN、或需要精细控制锁/并发，建议直接写 SQL（`query()` / `sql()`），而不是强行塞进派生宏。

## 下一步

- 下一章：[`写入：UpdateModel`](/zh/guide/update-model)
