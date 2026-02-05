# Writes: Insert, Update, Upsert

pgorm provides derive macros for type-safe inserts, updates, and upserts. All write operations use parameterized queries and support `RETURNING` to get inserted/updated rows back.

## 1. InsertModel

Derive `InsertModel` on a struct to get `.insert_returning()` (single row) and `::insert_many_returning()` (bulk).

### Single insert with RETURNING

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

### Bulk insert: `insert_many` and `insert_many_returning`

Bulk inserts use PostgreSQL's `UNNEST` for maximum throughput -- all rows are sent in a single query instead of one INSERT per row.

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

// Bulk insert with RETURNING
let inserted = NewProduct::insert_many_returning(&client, rows).await?;
println!("inserted {} product(s)", inserted.len());
```

### Field attributes for InsertModel

| Attribute | Effect |
|-----------|--------|
| `#[orm(skip_insert)]` | Field is never included in the INSERT statement |
| `#[orm(default)]` | Uses PostgreSQL `DEFAULT` for this column |
| `#[orm(auto_now_add)]` | If the value is `None`, fills it with `Utc::now()` at insert time |

## 2. UpdateModel (patch semantics)

`UpdateModel` generates partial-update methods. Fields use `Option<T>` to express "skip or update":

- `Option<T>`: `None` = skip (keep existing value), `Some(v)` = update to `v`
- `Option<Option<T>>`: `None` = skip, `Some(None)` = set to NULL, `Some(Some(v))` = update to `v`

```rust
use pgorm::UpdateModel;

#[derive(UpdateModel)]
#[orm(table = "products", model = "Product", returning = "Product")]
struct ProductPatch {
    name: Option<String>,              // None = skip, Some(v) = update
    description: Option<Option<String>>, // Some(None) = set NULL
    price_cents: Option<i64>,
    in_stock: Option<bool>,
}
```

### `update_by_id` and `update_by_ids`

```rust
let patch = ProductPatch {
    name: Some("New Name".into()),
    description: Some(None),  // set to NULL
    price_cents: None,        // keep existing
    in_stock: None,           // keep existing
};

// Update single row -- returns number of affected rows
let affected = patch.update_by_id(&client, 1_i64).await?;

// Update multiple rows
let affected = patch.update_by_ids(&client, vec![1_i64, 2, 3]).await?;
```

### `*_returning` variants

Get the updated row(s) back without a separate SELECT:

```rust
// Single row
let updated: Product = patch.update_by_id_returning(&client, 1_i64).await?;

// Multiple rows
let updated: Vec<Product> = patch
    .update_by_ids_returning(&client, vec![1_i64, 2, 3])
    .await?;
```

### Field attributes for UpdateModel

| Attribute | Effect |
|-----------|--------|
| `#[orm(skip_update)]` | Field is never included in the UPDATE statement |
| `#[orm(default)]` | Sets the field to PostgreSQL `DEFAULT` |
| `#[orm(auto_now)]` | Automatically sets the field to `Utc::now()` on every update |

### Field type summary

| Field Type | Value | Behavior |
|-----------|-------|----------|
| `Option<T>` | `None` | Skip field (no update) |
| `Option<T>` | `Some(v)` | Update field to `v` |
| `Option<Option<T>>` | `None` | Skip field (no update) |
| `Option<Option<T>>` | `Some(None)` | Set field to NULL |
| `Option<Option<T>>` | `Some(Some(v))` | Update field to `v` |
| `T` (non-optional) | `value` | Always update field |

## 3. Upsert (ON CONFLICT)

Add `conflict_target` or `conflict_constraint` plus `conflict_update` to an `InsertModel` to enable upsert behavior.

### Using `conflict_target` (column-based)

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

// Single upsert with RETURNING
let tag = TagUpsert {
    name: "rust".into(),
    color: Some("orange".into()),
}
.upsert_returning(&client)
.await?;
```

### Using `conflict_constraint` (named constraint)

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

### Batch upsert

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

## 4. Optimistic Locking

Prevent lost updates in concurrent scenarios with `#[orm(version)]`. When updating:

- The current version is checked in the `WHERE` clause: `WHERE id = $1 AND version = $2`
- The version is auto-incremented in the `SET` clause: `SET version = version + 1`
- If the version doesn't match (concurrent modification), `OrmError::StaleRecord` is returned

### Defining the version field

```rust
#[derive(UpdateModel)]
#[orm(table = "articles", model = "Article", returning = "Article")]
struct ArticlePatch {
    title: Option<String>,
    body: Option<String>,
    #[orm(version)]        // auto-checked in WHERE, auto-incremented in SET
    version: i32,
}
```

### `StaleRecord` error handling

```rust
let patch = ArticlePatch {
    title: Some("Updated Title".into()),
    body: None,
    version: article.version,  // pass current version
};

match patch.update_by_id_returning(&client, article.id).await {
    Ok(updated) => println!("Updated to version {}", updated.version),
    Err(OrmError::StaleRecord { table, expected_version, .. }) => {
        println!("Conflict on '{}': expected version {}", table, expected_version);
    }
    Err(e) => return Err(e),
}
```

### `update_by_id_force` for admin override

Force updates skip the version check but still increment the version:

```rust
let admin_patch = ArticlePatch {
    title: Some("Admin Override".into()),
    body: None,
    version: 0, // this value is ignored for force updates
};

let affected = admin_patch.update_by_id_force(&client, article.id).await?;

// With RETURNING
let updated = admin_patch
    .update_by_id_force_returning(&client, article.id)
    .await?;
```

### Retry pattern

The recommended pattern for handling optimistic locking conflicts:

```rust
let max_retries = 3;

for attempt in 1..=max_retries {
    // Re-fetch the latest version
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

### Optimistic locking API summary

| Method | Version Check | Description |
|--------|--------------|-------------|
| `update_by_id` | Yes | Update with version check, returns affected rows |
| `update_by_id_returning` | Yes | Update with version check, returns updated row |
| `update_by_id_force` | No | Skip version check (admin override) |
| `update_by_id_force_returning` | No | Skip version check, returns updated row |
| `update_by_ids` | No | Bulk updates do not support version checking |

## Next

- Next: [SQL Queries: query() & sql()](/en/guide/query)
