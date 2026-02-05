# 写操作：插入、更新、Upsert

pgorm 提供了派生宏用于类型安全的插入、更新和 upsert 操作。所有写操作都使用参数化查询，并支持 `RETURNING` 来获取插入/更新后的行数据。

## 1. InsertModel

在结构体上派生 `InsertModel` 即可获得 `.insert_returning()`（单行）和 `::insert_many_returning()`（批量）方法。

### 带 RETURNING 的单行插入

```rust
use pgorm::InsertModel;

#[derive(InsertModel)]
#[orm(table = "products", returning = "Product")]
struct NewProduct {
    sku: String,
    name: String,
    price_cents: i64,
}

let product = NewProduct {
    sku: "SKU-001".into(),
    name: "Keyboard".into(),
    price_cents: 7999,
}.insert_returning(&client).await?;
```

### 批量插入：`insert_many` 和 `insert_many_returning`

批量插入使用 PostgreSQL 的 `UNNEST` 以获得最大吞吐量 -- 所有行通过单条查询发送，而非逐行 INSERT。

```rust
use pgorm::InsertModel;

#[derive(InsertModel)]
#[orm(table = "products", returning = "Product")]
struct NewProduct {
    sku: String,
    name: String,
    price_cents: i64,
    #[orm(auto_now_add)]
    created_at: Option<DateTime<Utc>>,
    #[orm(auto_now_add)]
    updated_at: Option<DateTime<Utc>>,
}

let rows = vec![
    NewProduct {
        sku: "SKU-001".into(),
        name: "Keyboard".into(),
        price_cents: 7999,
        created_at: None,
        updated_at: None,
    },
    NewProduct {
        sku: "SKU-002".into(),
        name: "Mouse".into(),
        price_cents: 2999,
        created_at: None,
        updated_at: None,
    },
    NewProduct {
        sku: "SKU-003".into(),
        name: "Monitor".into(),
        price_cents: 19999,
        created_at: None,
        updated_at: None,
    },
];

// 带 RETURNING 的批量插入
let inserted = NewProduct::insert_many_returning(&client, rows).await?;
println!("inserted {} product(s)", inserted.len());
```

### InsertModel 的字段属性

| 属性 | 效果 |
|------|------|
| `#[orm(skip_insert)]` | 该字段不会包含在 INSERT 语句中 |
| `#[orm(default)]` | 使用 PostgreSQL 的 `DEFAULT` 作为该列的值 |
| `#[orm(auto_now_add)]` | 如果值为 `None`，则在插入时自动填充 `Utc::now()` |

## 2. UpdateModel（补丁语义）

`UpdateModel` 生成部分更新方法。字段使用 `Option<T>` 来表达"跳过或更新"：

- `Option<T>`：`None` = 跳过（保留现有值），`Some(v)` = 更新为 `v`
- `Option<Option<T>>`：`None` = 跳过，`Some(None)` = 设为 NULL，`Some(Some(v))` = 更新为 `v`

```rust
use pgorm::UpdateModel;

#[derive(UpdateModel)]
#[orm(table = "products", model = "Product", returning = "Product")]
struct ProductPatch {
    name: Option<String>,              // None = 跳过, Some(v) = 更新
    description: Option<Option<String>>, // Some(None) = 设为 NULL
    price_cents: Option<i64>,
    in_stock: Option<bool>,
}
```

### `update_by_id` 和 `update_by_ids`

```rust
let patch = ProductPatch {
    name: Some("New Name".into()),
    description: Some(None),  // set to NULL
    price_cents: None,        // keep existing
    in_stock: None,           // keep existing
};

// 更新单行 -- 返回受影响的行数
let affected = patch.update_by_id(&client, 1_i64).await?;

// 更新多行
let affected = patch.update_by_ids(&client, vec![1_i64, 2, 3]).await?;
```

### `*_returning` 变体

无需额外 SELECT 即可获取更新后的行：

```rust
// 单行
let updated: Product = patch.update_by_id_returning(&client, 1_i64).await?;

// 多行
let updated: Vec<Product> = patch
    .update_by_ids_returning(&client, vec![1_i64, 2, 3])
    .await?;
```

### UpdateModel 的字段属性

| 属性 | 效果 |
|------|------|
| `#[orm(skip_update)]` | 该字段不会包含在 UPDATE 语句中 |
| `#[orm(default)]` | 将字段设为 PostgreSQL 的 `DEFAULT` |
| `#[orm(auto_now)]` | 每次更新时自动将字段设为 `Utc::now()` |

### 字段类型总结

| 字段类型 | 值 | 行为 |
|----------|-----|------|
| `Option<T>` | `None` | 跳过字段（不更新） |
| `Option<T>` | `Some(v)` | 将字段更新为 `v` |
| `Option<Option<T>>` | `None` | 跳过字段（不更新） |
| `Option<Option<T>>` | `Some(None)` | 将字段设为 NULL |
| `Option<Option<T>>` | `Some(Some(v))` | 将字段更新为 `v` |
| `T`（非 Option） | `value` | 始终更新字段 |

## 3. Upsert（ON CONFLICT）

在 `InsertModel` 上添加 `conflict_target` 或 `conflict_constraint` 以及 `conflict_update` 即可启用 upsert 行为。

### 使用 `conflict_target`（基于列）

```rust
#[derive(InsertModel)]
#[orm(
    table = "tags",
    returning = "Tag",
    conflict_target = "name",
    conflict_update = "color"
)]
struct TagUpsert {
    name: String,
    color: Option<String>,
}

// 带 RETURNING 的单行 upsert
let tag = TagUpsert {
    name: "rust".into(),
    color: Some("orange".into()),
}
.upsert_returning(&client)
.await?;
```

### 使用 `conflict_constraint`（命名约束）

```rust
#[derive(InsertModel)]
#[orm(
    table = "tags",
    returning = "Tag",
    conflict_constraint = "tags_name_unique",
    conflict_update = "color"
)]
struct TagUpsertByConstraint {
    name: String,
    color: Option<String>,
}
```

### 批量 upsert

```rust
let tags = TagUpsert::upsert_many_returning(
    &client,
    vec![
        TagUpsert { name: "rust".into(), color: Some("red".into()) },
        TagUpsert { name: "zig".into(), color: None },
    ],
)
.await?;
```

## 4. 乐观锁

使用 `#[orm(version)]` 防止并发场景下的更新丢失。更新时：

- 当前版本号在 `WHERE` 子句中被检查：`WHERE id = $1 AND version = $2`
- 版本号在 `SET` 子句中自动递增：`SET version = version + 1`
- 如果版本号不匹配（并发修改），返回 `OrmError::StaleRecord`

### 定义版本字段

```rust
#[derive(UpdateModel)]
#[orm(table = "articles", model = "Article", returning = "Article")]
struct ArticlePatch {
    title: Option<String>,
    body: Option<String>,
    #[orm(version)]        // 在 WHERE 中自动检查，在 SET 中自动递增
    version: i32,
}
```

### `StaleRecord` 错误处理

```rust
let patch = ArticlePatch {
    title: Some("Updated Title".into()),
    body: None,
    version: article.version,  // 传入当前版本号
};

match patch.update_by_id_returning(&client, article.id).await {
    Ok(updated) => println!("Updated to version {}", updated.version),
    Err(OrmError::StaleRecord { table, expected_version, .. }) => {
        println!("Conflict on '{}': expected version {}", table, expected_version);
    }
    Err(e) => return Err(e),
}
```

### `update_by_id_force` 管理员强制覆盖

强制更新会跳过版本检查，但仍会递增版本号：

```rust
let admin_patch = ArticlePatch {
    title: Some("Admin Override".into()),
    body: None,
    version: 0, // 强制更新时此值会被忽略
};

let affected = admin_patch.update_by_id_force(&client, article.id).await?;

// 带 RETURNING
let updated = admin_patch
    .update_by_id_force_returning(&client, article.id)
    .await?;
```

### 重试模式

处理乐观锁冲突的推荐模式：

```rust
let max_retries = 3;

for attempt in 1..=max_retries {
    // 重新获取最新版本
    let current: Article = query("SELECT * FROM articles WHERE id = $1")
        .bind(target_id)
        .fetch_one_as::<Article>(&client)
        .await?;

    let patch = ArticlePatch {
        title: Some(format!("Updated (attempt {attempt})")),
        body: None,
        version: current.version,
    };

    match patch.update_by_id_returning(&client, target_id).await {
        Ok(updated) => {
            println!("Success! Version: {} -> {}", current.version, updated.version);
            break;
        }
        Err(OrmError::StaleRecord { .. }) => {
            println!("Attempt {attempt} failed: version conflict, retrying...");
        }
        Err(e) => return Err(e),
    }
}
```

### 乐观锁 API 总结

| 方法 | 版本检查 | 说明 |
|------|----------|------|
| `update_by_id` | 是 | 带版本检查的更新，返回受影响行数 |
| `update_by_id_returning` | 是 | 带版本检查的更新，返回更新后的行 |
| `update_by_id_force` | 否 | 跳过版本检查（管理员强制覆盖） |
| `update_by_id_force_returning` | 否 | 跳过版本检查，返回更新后的行 |
| `update_by_ids` | 否 | 批量更新不支持版本检查 |

## 下一步

- 下一章：[SQL 查询：query() 与 sql()](/zh/guide/query)
