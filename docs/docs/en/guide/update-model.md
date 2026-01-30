# Writes: `UpdateModel` (Patch Semantics)

`UpdateModel` is designed for patch-style updates:

- `Option<T>`: `None` means **skip**, `Some(v)` means **set to v**
- `Option<Option<T>>`: tri-state (skip / set NULL / set value)

This maps naturally to HTTP PATCH and “partial update” forms.

## 1) Minimal example: update by id

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
    description: Some(None), // explicitly set NULL
    price_cents: None,       // skip
};

let updated: Product = patch.update_by_id_returning(&client, 1_i64).await?;
```

Notes:

- `#[orm(model = "Product")]` derives the primary key column from the `Model`
- `#[orm(returning = "Product")]` enables `*_returning` helpers

## 2) Bulk update: `update_by_ids(_returning)`

```rust
let patch = ProductPatch {
    name: None,
    description: None,
    price_cents: Some(7999),
};

let affected = patch.update_by_ids(&client, vec![1_i64, 2, 3]).await?;
```

If you want updated rows back:

```rust
let updated: Vec<Product> = patch
    .update_by_ids_returning(&client, vec![1_i64, 2, 3])
    .await?;
```

> Runnable example: `crates/pgorm/examples/update_model`.

## 3) Common field attributes

### `#[orm(skip_update)]`

Never include the field in UPDATE.

### `#[orm(default)]`

Set the column to SQL `DEFAULT`.

### `#[orm(auto_now)]`

Auto-fill “now” on update (in Rust) when the field is `None`:

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

> `auto_now` currently requires `Option<DateTime<Utc>>` or `Option<NaiveDateTime>`.

## 4) Strictness note

`UpdateModel` is about generating UPDATE SQL. It does not change read-side row-count semantics. For that, see: [`Fetch Semantics`](/en/guide/fetch-semantics).

## Next

- Next: [`Writes: Upsert`](/en/guide/upsert)
