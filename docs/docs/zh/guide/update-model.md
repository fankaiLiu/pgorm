# 写入：`UpdateModel`（Patch 语义）

`UpdateModel` 的定位是“补丁式更新（patch-style）”：

- `Option<T>`：`None` 表示 **跳过该字段**，`Some(v)` 表示 **更新为 v**
- `Option<Option<T>>`：三态语义（跳过 / 设为 NULL / 设为值）

这让你可以自然地表达“部分更新”，尤其适合 HTTP PATCH / 管理后台表单等场景。

## 1) 最小示例：按主键更新

```rust
use pgorm::{FromRow, Model, UpdateModel};

#[derive(Debug, FromRow, Model)]
#[orm(table = "products")]
struct Product {
    #[orm(id)]
    id: i64,
    name: String,
    description: Option<String>,
    price_cents: i64,
}

#[derive(Debug, UpdateModel)]
#[orm(table = "products", model = "Product", returning = "Product")]
struct ProductPatch {
    name: Option<String>,
    description: Option<Option<String>>,
    price_cents: Option<i64>,
}

let patch = ProductPatch {
    name: Some("New Name".into()),
    description: Some(None), // 显式设为 NULL
    price_cents: None,       // 跳过，不更新
};

let updated: Product = patch.update_by_id_returning(&client, 1_i64).await?;
```

要点：

- `#[orm(model = "Product")]`：从 `Model` 推断主键列
- `#[orm(returning = "Product")]`：生成 `*_returning` 系列方法

## 2) 批量更新：`update_by_ids(_returning)`

```rust
let patch = ProductPatch {
    name: None,
    description: None,
    price_cents: Some(7999),
};

let affected = patch.update_by_ids(&client, vec![1_i64, 2, 3]).await?;
```

如果你需要返回更新后的行：

```rust
let updated: Vec<Product> = patch
    .update_by_ids_returning(&client, vec![1_i64, 2, 3])
    .await?;
```

> 可运行示例见 `crates/pgorm/examples/update_model`。

## 3) 常用字段属性

### `#[orm(skip_update)]`：永远不参与 UPDATE

适合：只读字段、不可变字段。

### `#[orm(default)]`：把字段设为 `DEFAULT`

适合：你希望“回到数据库默认值”的场景。

### `#[orm(auto_now)]`：更新时自动填充“当前时间”

常用于 `updated_at`：

```rust
use chrono::{DateTime, Utc};
use pgorm::UpdateModel;

#[derive(UpdateModel)]
#[orm(table = "products", id_column = "id")]
struct TouchPatch {
    #[orm(auto_now)]
    updated_at: Option<DateTime<Utc>>,
}
```

> `auto_now` 目前要求字段类型为 `Option<DateTime<Utc>>` 或 `Option<NaiveDateTime>`。

## 4) 什么时候要用 `fetch_one_strict` 而不是 UpdateModel？

`UpdateModel` 关注的是“生成 UPDATE 语句”。它不会改变“查询的行数语义”。  
如果你需要在读的时候强制“必须恰好一行”，请看：[`Fetch 语义`](/zh/guide/fetch-semantics)。

## 下一步

- 下一章：[`写入：Upsert`](/zh/guide/upsert)
