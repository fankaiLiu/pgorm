# Advanced Writes: Write Graph (Multi-table Writes)

Write Graph is for “one business operation writes multiple tables”, for example:

- creating a product: `products` + (optional) `categories` + `product_details` + `product_tags` + `audit_logs`
- creating an order: `orders` + `order_items` + audit/event tables

Key ideas:

- model-definition-first: you declare the write path via derive macros
- pgorm generates multi-step write logic
- you usually wrap the whole write in a transaction (atomicity)

> This is an advanced feature. For complex flows where CTEs are a better fit, prefer writing SQL directly.

## 1) Minimal structure: root fields + graph fields

This is a trimmed version of `crates/pgorm/examples/write_graph`:

```rust
use pgorm::InsertModel;

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
    // root (inserted into products)
    id: uuid::Uuid,
    name: String,
    category_id: Option<i64>,

    // graph fields (inserted into other tables)
    category: Option<NewCategory>,
    detail: Option<NewProductDetail>,
    tags: Option<Vec<NewProductTag>>,
    audit: Option<NewAuditLog>,
}
```

Concepts:

- `graph_root_id_field`: the root id field name (used to fill child FKs)
- `belongs_to / has_one / has_many`: write edges + FK wiring
- `after_insert`: run an extra insert after the root insert (e.g. audit/events)

## 2) Execute: `insert_graph_report`

```rust
let report = new_product_graph.insert_graph_report(&tx).await?;
println!("affected = {}", report.affected);
for step in &report.steps {
    println!("- {} affected={}", step.tag, step.affected);
}
```

## 3) Strongly recommended: wrap in a transaction

```rust
let report = pgorm::transaction!(&mut client, tx, {
    let report = new_product_graph.insert_graph_report(&tx).await?;
    Ok(report)
})?;
```

## 4) Runnable example

- `crates/pgorm/examples/write_graph`

## Next

- Next: [`Transactions`](/en/guide/transactions)
