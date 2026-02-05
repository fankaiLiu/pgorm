# Multi-Table Write Graph

Write Graph lets you insert related records across multiple tables atomically in a single operation. Instead of writing separate insert calls and manually wiring foreign keys, you declare the relationships on your `InsertModel` and pgorm generates the multi-step write logic for you.

Typical use cases:

- Creating a product with its category, details, tags, and an audit log entry
- Creating an order with order items and event records
- Any business operation that writes to several tables at once

> This is an advanced feature. For complex flows where CTEs or hand-written SQL are a better fit, prefer writing SQL directly.

## Defining the Graph

A write graph is declared on an `InsertModel` struct using derive macro attributes. There are two kinds of fields:

- **Root fields** -- columns that go into the root table (e.g. `products`)
- **Graph fields** -- data for related tables, wired automatically by pgorm

### Graph Root ID

The `graph_root_id_field` attribute tells pgorm which field on the root struct holds the primary key. This value is used to fill foreign keys on child records.

```rust
#[orm(graph_root_id_field = "id")]
```

### Edge Types

#### `belongs_to` -- Insert a Parent First

Insert the parent record first (using `insert_returning` to get its generated ID), then set a foreign key on the root record before inserting it.

```rust
#[orm(belongs_to(
    NewCategory,
    field = "category",         // field name on the graph struct
    set_fk_field = "category_id", // FK column on the root table
    mode = "insert_returning",  // insert parent and use RETURNING to get its ID
    required = false            // the field is Option<NewCategory>
))]
```

#### `has_one` -- Insert a Child After Root

After the root record is inserted, insert one child record with the root's ID filled into the child's FK column.

```rust
#[orm(has_one(
    NewProductDetail,
    field = "detail",           // field name on the graph struct
    fk_field = "product_id",   // FK column on the child table
    mode = "insert"
))]
```

#### `has_many` -- Insert Multiple Children After Root

Same as `has_one`, but for a `Vec` of child records. Each child gets the root's ID filled into its FK column.

```rust
#[orm(has_many(
    NewProductTag,
    field = "tags",             // field name on the graph struct
    fk_field = "product_id",   // FK column on the child table
    mode = "insert"
))]
```

#### `after_insert` -- Run Extra Inserts After Root

For audit logs or event records that should be written after the root insert. Unlike `has_one`/`has_many`, the FK is not automatically wired -- you set it yourself (useful when the root ID is known upfront, e.g. a UUID).

```rust
#[orm(after_insert(NewAuditLog, field = "audit", mode = "insert"))]
```

## Full Example

Here is the complete graph struct from the `write_graph` example:

```rust
use pgorm::{FromRow, InsertModel, Model, WriteReport};

// --- Related insert models ---

#[derive(Debug, Clone, InsertModel)]
#[orm(table = "categories", returning = "Category")]
struct NewCategory {
    name: String,
}

#[derive(Debug, Clone, InsertModel)]
#[orm(table = "product_details")]
struct NewProductDetail {
    product_id: Option<uuid::Uuid>, // filled by graph via has_one
    description: String,
}

#[derive(Debug, Clone, InsertModel)]
#[orm(table = "product_tags")]
struct NewProductTag {
    product_id: Option<uuid::Uuid>, // filled by graph via has_many
    tag: String,
}

#[derive(Debug, Clone, InsertModel)]
#[orm(table = "audit_logs")]
struct NewAuditLog {
    product_id: uuid::Uuid,
    action: String,
}

// --- The graph root ---

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
    // Root fields (inserted into products)
    id: uuid::Uuid,
    name: String,
    category_id: Option<i64>,

    // Graph fields (inserted into related tables)
    category: Option<NewCategory>,
    detail: Option<NewProductDetail>,
    tags: Option<Vec<NewProductTag>>,
    audit: Option<NewAuditLog>,
}
```

## Executing with `insert_graph_report`

Call `insert_graph_report()` on the graph struct to execute all steps:

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
        product_id: None, // filled automatically by the graph
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

## Transaction Wrapping

Write graphs do **not** automatically wrap themselves in a transaction. For atomicity, you should wrap the call in `pgorm::transaction!`:

```rust
let report = pgorm::transaction!(&mut client, tx, {
    let report = new_product.insert_graph_report(&tx).await?;
    Ok(report)
})?;
```

If any step fails, the entire transaction rolls back, leaving the database in a consistent state.

## `WriteReport` and `WriteStepReport`

`insert_graph_report()` returns a `WriteReport<T>` where `T` is the root model's returning type (e.g. `Product`). It contains:

- **`affected`** -- total number of rows affected across all steps
- **`root`** -- the returned root record (if the root table uses `returning`)
- **`steps`** -- a `Vec<WriteStepReport>`, one per graph edge executed

Each `WriteStepReport` has:

- **`tag`** -- a string identifying the step (e.g. `"belongs_to:category"`, `"has_many:tags"`)
- **`affected`** -- number of rows affected by that step

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

## Runnable Example

See `crates/pgorm/examples/write_graph/main.rs` for a complete, runnable example with schema setup and verification queries.

## Next

- Next: [Monitoring & Hooks](/en/guide/monitoring)
