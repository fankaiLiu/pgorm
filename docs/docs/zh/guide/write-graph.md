# 多表写入图（Write Graph）

Write Graph 允许你在一次操作中原子性地向多张关联表插入记录。无需编写多个单独的插入调用并手动传递外键，你只需在 `InsertModel` 上声明关联关系，pgorm 就会自动生成多步写入逻辑。

典型使用场景：

- 创建商品，同时写入分类、详情、标签和审计日志
- 创建订单，同时写入订单明细和事件记录
- 任何需要一次性写入多张表的业务操作

> 这是一个高级功能。如果你的场景更适合使用 CTE 或手写 SQL，建议直接编写 SQL。

## 定义写入图

写入图通过派生宏属性声明在 `InsertModel` 结构体上。字段分为两类：

- **Root 字段** -- 写入根表的列（例如 `products`）
- **Graph 字段** -- 关联表的数据，由 pgorm 自动关联

### Graph Root ID

`graph_root_id_field` 属性告诉 pgorm 根结构体上哪个字段持有主键。该值用于填充子记录的外键。

```rust
#[orm(graph_root_id_field = "id")]
```

### 边类型

#### `belongs_to` -- 先插入父记录

先插入父记录（使用 `insert_returning` 获取其生成的 ID），然后在插入根记录之前将外键设置到根记录上。

```rust
#[orm(belongs_to(
    NewCategory,
    field = "category",         // graph 结构体上的字段名
    set_fk_field = "category_id", // 根表上的外键列
    mode = "insert_returning",  // 插入父记录并通过 RETURNING 获取其 ID
    required = false            // 该字段是 Option<NewCategory>
))]
```

#### `has_one` -- 在根记录之后插入子记录

根记录插入后，插入一条子记录，并将根记录的 ID 填充到子记录的外键列中。

```rust
#[orm(has_one(
    NewProductDetail,
    field = "detail",           // graph 结构体上的字段名
    fk_field = "product_id",   // 子表上的外键列
    mode = "insert"
))]
```

#### `has_many` -- 在根记录之后插入多条子记录

与 `has_one` 相同，但针对 `Vec` 类型的子记录。每条子记录的外键列都会被填充为根记录的 ID。

```rust
#[orm(has_many(
    NewProductTag,
    field = "tags",             // graph 结构体上的字段名
    fk_field = "product_id",   // 子表上的外键列
    mode = "insert"
))]
```

#### `after_insert` -- 在根记录插入后执行额外插入

用于审计日志或事件记录等需要在根记录插入后写入的场景。与 `has_one`/`has_many` 不同，外键不会自动关联 -- 你需要自行设置（适用于根记录 ID 是预先已知的情况，例如 UUID）。

```rust
#[orm(after_insert(NewAuditLog, field = "audit", mode = "insert"))]
```

## 完整示例

以下是 `write_graph` 示例中的完整写入图结构体：

```rust
use pgorm::{FromRow, InsertModel, Model, WriteReport};

// --- 关联表的插入模型 ---

#[derive(Debug, Clone, InsertModel)]
#[orm(table = "categories", returning = "Category")]
struct NewCategory {
    name: String,
}

#[derive(Debug, Clone, InsertModel)]
#[orm(table = "product_details")]
struct NewProductDetail {
    product_id: Option<uuid::Uuid>, // 由 graph 通过 has_one 填充
    description: String,
}

#[derive(Debug, Clone, InsertModel)]
#[orm(table = "product_tags")]
struct NewProductTag {
    product_id: Option<uuid::Uuid>, // 由 graph 通过 has_many 填充
    tag: String,
}

#[derive(Debug, Clone, InsertModel)]
#[orm(table = "audit_logs")]
struct NewAuditLog {
    product_id: uuid::Uuid,
    action: String,
}

// --- 写入图根 ---

#[derive(Debug, Clone, InsertModel)]
#[orm(table = "products", returning = "Product")]
#[orm(graph_root_id_field = "id")]
#[orm(belongs_to(
    NewCategory,
    field = "category",
    set_fk_field = "category_id",
    mode = "insert_returning",
    required = false
))]
#[orm(has_one(
    NewProductDetail,
    field = "detail",
    fk_field = "product_id",
    mode = "insert"
))]
#[orm(has_many(
    NewProductTag,
    field = "tags",
    fk_field = "product_id",
    mode = "insert"
))]
#[orm(after_insert(NewAuditLog, field = "audit", mode = "insert"))]
struct NewProductGraph {
    // Root 字段（插入到 products 表）
    id: uuid::Uuid,
    name: String,
    category_id: Option<i64>,

    // Graph 字段（插入到关联表）
    category: Option<NewCategory>,
    detail: Option<NewProductDetail>,
    tags: Option<Vec<NewProductTag>>,
    audit: Option<NewAuditLog>,
}
```

## 使用 `insert_graph_report` 执行

在写入图结构体上调用 `insert_graph_report()` 执行所有步骤：

```rust
let product_id = uuid::Uuid::new_v4();

let new_product = NewProductGraph {
    id: product_id,
    name: "Graph Product".into(),
    category_id: None,
    category: Some(NewCategory {
        name: "Category A".into(),
    }),
    detail: Some(NewProductDetail {
        product_id: None, // 由 graph 自动填充
        description: "Inserted via has_one".into(),
    }),
    tags: Some(vec![
        NewProductTag { product_id: None, tag: "tag-1".into() },
        NewProductTag { product_id: None, tag: "tag-2".into() },
    ]),
    audit: Some(NewAuditLog {
        product_id,
        action: "CREATE_PRODUCT".into(),
    }),
};

let report = new_product.insert_graph_report(&tx).await?;
```

## 事务包装

写入图**不会**自动包装在事务中。为了保证原子性，你应当使用 `pgorm::transaction!` 包装调用：

```rust
let report = pgorm::transaction!(&mut client, tx, {
    let report = new_product.insert_graph_report(&tx).await?;
    Ok(report)
})?;
```

如果任何步骤失败，整个事务将回滚，使数据库保持一致状态。

## `WriteReport` 和 `WriteStepReport`

`insert_graph_report()` 返回 `WriteReport<T>`，其中 `T` 是根模型的返回类型（例如 `Product`）。它包含：

- **`affected`** -- 所有步骤影响的总行数
- **`root`** -- 返回的根记录（如果根表使用了 `returning`）
- **`steps`** -- `Vec<WriteStepReport>`，每个执行的图边对应一个

每个 `WriteStepReport` 包含：

- **`tag`** -- 标识该步骤的字符串（例如 `"belongs_to:category"`、`"has_many:tags"`）
- **`affected`** -- 该步骤影响的行数

```rust
fn print_report(report: &WriteReport<Product>) {
    println!("affected = {}", report.affected);
    println!("steps:");
    for s in &report.steps {
        println!("- {} affected={}", s.tag, s.affected);
    }
    println!("root: {:?}", report.root.as_ref().map(|p| (&p.id, &p.name)));
}
```

## 可运行示例

参见 `crates/pgorm/examples/write_graph/main.rs`，包含完整的 schema 创建和验证查询。

## 下一步

- 下一章：[监控与 Hooks](/zh/guide/monitoring)
