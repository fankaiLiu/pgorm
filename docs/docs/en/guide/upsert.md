# Writes: Upsert (`ON CONFLICT`)

Upsert in pgorm is part of `InsertModel`: you declare the conflict strategy in `#[orm(...)]`, then call `upsert_*`.

## 1) Minimal example: upsert by unique column

```rust
use pgorm::{FromRow, InsertModel, Model};

#[derive(Debug, Clone, FromRow, Model)]
#[orm(table = "tags")]
struct Tag {
    #[orm(id)]
    id: i64,
    name: String,
    color: Option<String>,
}

#[derive(Debug, Clone, InsertModel)]
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

let tag: Tag = TagUpsert {
    name: "rust".into(),
    color: Some("orange".into()),
}
.upsert_returning(&client)
.await?;
```

Meaning:

- `conflict_target = "name"` → `ON CONFLICT (name)`
- `conflict_update = "color"` → update these columns on conflict

## 2) Use a named constraint as the conflict target

If your constraint name is the stable contract:

```rust
#[derive(Debug, Clone, InsertModel)]
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

## 3) Bulk upsert: `upsert_many(_returning)`

```rust
let tags: Vec<Tag> = TagUpsert::upsert_many_returning(
    &client,
    vec![
        TagUpsert { name: "rust".into(), color: Some("red".into()) },
        TagUpsert { name: "zig".into(), color: None },
    ],
)
.await?;
```

> Runnable example: `crates/pgorm/examples/upsert`.

## 4) Notes

- `conflict_update` only lists columns; for more complex update expressions, write SQL explicitly.
- If you need multi-step consistency, wrap upserts in a [`transaction`](/en/guide/transactions).

## Next

- Next: [`Advanced Writes: Write Graph`](/en/guide/write-graph)
