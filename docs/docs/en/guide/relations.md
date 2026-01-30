# Relations: `has_many` / `belongs_to`

In pgorm, relations are **explicitly declared** and **explicitly loaded**:

- declare relations on the model via `#[orm(has_many(...))]` / `#[orm(belongs_to(...))]`
- no extra queries happen unless you call `load_*` helpers (so no “surprise N+1”)

This page covers *declarations* and what helpers are generated. For usage, see: [`Eager Loading`](/en/guide/eager-loading).

## 1) `has_many`: one-to-many

```rust
use pgorm::{FromRow, Model};

#[derive(Debug, Clone, FromRow, Model)]
#[orm(table = "users")]
#[orm(has_many(Post, foreign_key = "user_id", as = "posts"))]
struct User {
    #[orm(id)]
    id: i64,
    name: String,
}

#[derive(Debug, Clone, FromRow, Model)]
#[orm(table = "posts")]
struct Post {
    #[orm(id)]
    id: i64,
    user_id: i64,
    title: String,
}
```

Parameters:

- `Post`: child model type
- `foreign_key = "user_id"`: FK column on the child table
- `as = "posts"`: relation name (affects generated method names)

Common generated helpers (for `as = "posts"`):

- `User::load_posts_map(conn, &users)` → `HashMap<PK, Vec<Post>>`
- `User::load_posts_map_with(conn, &users, |q| { ... })` → customize the preload query (e.g. ORDER BY)

## 2) `belongs_to`: many-to-one

```rust
use pgorm::{FromRow, Model};

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

Common generated helpers (for `as = "author"`):

- `Post::load_author(conn, posts)` → attach authors
- `Post::load_author_strict(conn, posts)` → strict variant (missing relation is an error)

## 3) Picking a good `as` name

- pick a business-friendly name (`author`, `posts`, …)
- keep it stable (renaming changes generated method names → breaking change)

## Next

- Next: [`Eager Loading`](/en/guide/eager-loading)
