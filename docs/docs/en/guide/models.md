# Models & Derive Macros

pgorm provides several derive macros for working with database models.

## FromRow

The `FromRow` derive macro maps database rows to Rust structs:

```rust
use pgorm::FromRow;

#[derive(FromRow)]
struct User {
    id: i64,
    username: String,
    email: Option<String>,
}
```

## Model

The `Model` derive macro provides CRUD operations and relation helpers:

```rust
use pgorm::{FromRow, Model};

#[derive(Debug, Clone, FromRow, Model)]
#[orm(table = "users")]
struct User {
    #[orm(id)]
    id: i64,
    name: String,
    email: String,
}
```

### Table Name

Use `#[orm(table = "table_name")]` to specify the database table name.

### Primary Key

Mark the primary key field with `#[orm(id)]`.

## Relations

### has_many

Define a one-to-many relationship:

```rust
#[derive(Debug, Clone, FromRow, Model)]
#[orm(table = "users")]
#[orm(has_many(Post, foreign_key = "user_id", as = "posts"))]
struct User {
    #[orm(id)]
    id: i64,
    name: String,
}
```

### belongs_to

Define a many-to-one relationship:

```rust
#[derive(Debug, Clone, FromRow, Model)]
#[orm(table = "posts")]
#[orm(belongs_to(User, foreign_key = "user_id", as = "author"))]
struct Post {
    #[orm(id)]
    id: i64,
    user_id: i64,
    title: String,
}
```

## JSONB Support

pgorm supports PostgreSQL JSONB columns:

```rust
use pgorm::{FromRow, Json};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct Meta {
    tags: Vec<String>,
}

#[derive(FromRow)]
struct User {
    id: i64,
    meta: Json<Meta>, // jsonb column
}
```

## Next

- Next: [`Relations: has_many / belongs_to`](/en/guide/relations)
