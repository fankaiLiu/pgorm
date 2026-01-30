# 高级写入：Write Graph（多表写入图）

Write Graph 用于“一个业务对象写入多张表”的场景，例如：

- 创建商品：products + categories（可选）+ product_details + product_tags + audit_logs
- 创建订单：orders + order_items + audit_logs

它的特点是：

- 仍然是 SQL-first 思路：你明确声明写入路径
- pgorm 根据声明生成多步写入逻辑
- 你通常会用事务把整张图包起来（原子性）

> 这是一个高级能力：如果你的写入更适合用 CTE 或者需要复杂逻辑，建议直接写 SQL。

## 1) 最小结构：Root + Graph 字段

下面是一个“插入 products，同时插入/关联其他表”的示意（节选自 `crates/pgorm/examples/write_graph`）：

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
    // root 字段（写入 products）
    id: uuid::Uuid,
    name: String,
    category_id: Option<i64>,

    // graph 字段（写入/关联其他表，不直接写入 products）
    category: Option<NewCategory>,
    detail: Option<NewProductDetail>,
    tags: Option<Vec<NewProductTag>>,
    audit: Option<NewAuditLog>,
}
```

关键概念：

- `graph_root_id_field`：Root 的主键字段名（后续用于填充子表外键）
- `belongs_to / has_one / has_many`：声明写入路径与外键填充规则
- `after_insert`：在 root 插入完成后再写入（常用于审计/事件表）

## 2) 执行：`insert_graph_report`

```rust
let report = new_product_graph.insert_graph_report(&tx).await?;
println!("affected = {}", report.affected);
for step in &report.steps {
    println!("- {} affected={}", step.tag, step.affected);
}
```

`report` 会告诉你每一步写入影响了多少行，便于排查。

## 3) 强烈建议：把写入图放进事务

```rust
let report = pgorm::transaction!(&mut client, tx, {
    let report = new_product_graph.insert_graph_report(&tx).await?;
    Ok(report)
})?;
```

## 4) 下一步：看可运行示例

- `crates/pgorm/examples/write_graph`：完整 schema + 写入图 + report 输出

## 下一步

- 下一章：[`事务`](/zh/guide/transactions)
