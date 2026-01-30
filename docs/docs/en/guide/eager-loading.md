# Eager Loading

pgorm supports explicit eager-loading helpers for relations. It never runs extra queries unless you call `load_*`.

## Map Style (Recommended)

One extra query per relation, returns a map indexed by primary key:

```rust
use pgorm::{FromRow, GenericClient, Model, ModelPk as _};

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

async fn list(conn: &impl GenericClient) -> pgorm::OrmResult<()> {
    let users = User::select_all(conn).await?;

    // Load posts for all users in one query
    let posts_by_user = User::load_posts_map_with(conn, &users, |q| {
        q.push(" ORDER BY id DESC");
    })
    .await?;

    for u in &users {
        let posts = posts_by_user.get(u.pk()).map(Vec::as_slice).unwrap_or(&[]);
        println!("User {} has {} posts", u.name, posts.len());
    }

    Ok(())
}
```

## Attach Style

Keep base order, attach relation payload to each row:

```rust
async fn list_posts(conn: &impl GenericClient) -> pgorm::OrmResult<()> {
    let posts = Post::select_all(conn).await?;

    // Attach author to each post
    let posts_with_author = Post::load_author(conn, posts).await?;

    Ok(())
}
```

## Strict Variant

Use the strict variant to require that a relation exists for every base row:

```rust
// Will error if any post doesn't have an author
let posts_with_author = Post::load_author_strict(conn, posts).await?;
```
