# Writes: `InsertModel`

`InsertModel` generates “insert helpers” while keeping your project SQL-first:

- keep read models (`Model`) and write models (`InsertModel`) separate
- avoid hand-writing repetitive `INSERT ... RETURNING ...`

## 1) Minimal example: insert with RETURNING

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

Notes:

- `#[orm(table = "...")]` is required
- `#[orm(returning = "Product")]` enables `insert_returning` (otherwise you only get non-returning helpers)

## 2) Bulk insert: `insert_many(_returning)` (UNNEST)

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

Note: bulk insert helpers use `UNNEST` and require each inserted field type to implement `pgorm::PgType`
(for array type casts). For some types you may need to enable extra pgorm features, e.g.
`rust_decimal` / `time` / `cidr` / `geo_types` / `eui48` / `bit_vec` (or just `extra_types`).

> Runnable example: `crates/pgorm/examples/insert_many`.

## 3) Common field attributes

### `#[orm(skip_insert)]`

Never include the field in INSERT (good for read-only / trigger-managed columns).

### `#[orm(default)]`

Use SQL `DEFAULT` for this column.

### `#[orm(auto_now_add)]`

Fill “now” on insert (in Rust) when the field is `None`:

```rust
use chrono::{DateTime, Utc};
use pgorm::InsertModel;

#[derive(InsertModel)]
#[orm(table = "products")]
struct NewProduct {
    name: String,
    #[orm(auto_now_add)]
    created_at: Option<DateTime<Utc>>,
}
```

> `auto_now_add` currently requires `Option<DateTime<Utc>>` or `Option<NaiveDateTime>`.

## 4) When *not* to use InsertModel

For complex CTEs, `INSERT ... SELECT`, advanced locking, or heavily customized SQL, prefer writing SQL explicitly (`query()` / `sql()`).

## Next

- Next: [`Writes: UpdateModel`](/en/guide/update-model)
