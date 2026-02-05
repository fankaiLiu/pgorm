# Relations & Eager Loading

In pgorm, relations are **explicitly declared** on the model and **explicitly loaded** via generated helper methods. No extra queries run unless you call a `load_*` method -- there are no surprise N+1 queries.

## Overview

pgorm supports four relation types:

| Relation | Attribute | Parent has... | Generated map type |
|----------|-----------|---------------|-------------------|
| `has_many` | `#[orm(has_many(...))]` | Many children | `HasManyMap<PK, Vec<Child>>` |
| `has_one` | `#[orm(has_one(...))]` | One child (0..1) | `HasOneMap<PK, Child>` |
| `belongs_to` | `#[orm(belongs_to(...))]` | One parent | `HashMap<FK, Parent>` |
| `many_to_many` | `#[orm(many_to_many(...))]` | Many through join table | `HasManyMap<PK, Vec<Child>>` |

## `has_many`

Declares a one-to-many relationship. The foreign key lives on the child table.

### Definition

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
- `Post` -- the child model type
- `foreign_key = "user_id"` -- the FK column on the child table
- `as = "posts"` -- the relation name (determines generated method names)

### Generated Methods

For `as = "posts"`, these methods are generated on `User`:

| Method | Return type | Description |
|--------|-------------|-------------|
| `User::load_posts_map(conn, &users)` | `HasManyMap<i64, Post>` | Batch load into a map keyed by parent PK |
| `User::load_posts_map_with(conn, &users, \|q\| { ... })` | `HasManyMap<i64, Post>` | Same, but customize the preload query |
| `User::load_posts(conn, users)` | `Vec<Loaded<User, Vec<Post>>>` | Attach style: attach children to each parent |
| `User::load_posts_with(conn, users, \|q\| { ... })` | `Vec<Loaded<User, Vec<Post>>>` | Attach style with query customization |

### Map Style (Recommended)

One extra query per relation, returns a map indexed by primary key:

```rust
use pgorm::ModelPk as _;

let users = User::select_all(&client).await?;

// Load all posts for these users in one query
let posts_by_user = User::load_posts_map(&client, &users).await?;

for user in &users {
    let posts = posts_by_user.get(user.pk()).unwrap_or(&vec![]);
    println!("{} has {} posts", user.name, posts.len());
}
```

### Customizing the Preload Query

Use the `_with` variant to add ordering, filtering, or limits to the preload:

```rust
let posts_by_user = User::load_posts_map_with(&client, &users, |q| {
    q.push(" ORDER BY id DESC");
}).await?;
```

### Attach Style

Returns a `Vec<Loaded<Parent, Vec<Child>>>` where each parent has its children attached. The output order matches the input order.

```rust
let users = User::select_all(&client).await?;
let users_with_posts = User::load_posts(&client, users).await?;

for u in &users_with_posts {
    // u.base is the User, u.rel is Vec<Post>
    println!("user {} has {} posts", u.base.name, u.rel.len());
}
```

## `belongs_to`

Declares a many-to-one (or one-to-one) relationship. The foreign key lives on the current model.

### Definition

```rust
#[derive(Debug, Clone, FromRow, Model)]
#[orm(table = "posts")]
#[orm(belongs_to(User, foreign_key = "user_id", as = "author"))]
#[orm(belongs_to(User, foreign_key = "editor_id", as = "editor"))]
struct Post {
    #[orm(id)]
    id: i64,
    user_id: i64,
    editor_id: Option<i64>,
    title: String,
}
```

You can declare multiple `belongs_to` relations to the same model with different foreign keys and different `as` names.

### Generated Methods

For `as = "author"`:

| Method | Return type | Description |
|--------|-------------|-------------|
| `Post::load_author(conn, posts)` | `Vec<Loaded<Post, Option<User>>>` | Attach parent to each child |
| `Post::load_author_with(conn, posts, \|q\| { ... })` | `Vec<Loaded<Post, Option<User>>>` | Attach with query customization |
| `Post::load_author_strict(conn, posts)` | `Vec<Loaded<Post, User>>` | Strict: error if any parent is missing |
| `Post::load_author_strict_with(conn, posts, \|q\| { ... })` | `Vec<Loaded<Post, User>>` | Strict with query customization |
| `Post::load_author_map(conn, &posts)` | `HashMap<FK, User>` | Map of FK to parent |

### Usage

```rust
let posts = Post::select_all(&client).await?;

// Default: Option<User> (None if FK is NULL or parent is missing)
let posts_with_author = Post::load_author(&client, posts.clone()).await?;
for p in &posts_with_author {
    let author = p.rel.as_ref()
        .map(|u| u.name.as_str())
        .unwrap_or("(missing)");
    println!("post {} by {}", p.base.title, author);
}

// Nullable FK (editor_id is Option<i64>)
let posts_with_editor = Post::load_editor(&client, posts.clone()).await?;
for p in &posts_with_editor {
    let editor = p.rel.as_ref()
        .map(|u| u.name.as_str())
        .unwrap_or("(null)");
    println!("post {} editor: {}", p.base.title, editor);
}
```

### Strict Variant

The strict variant returns an error if any row is missing its parent (e.g., an FK references a deleted record):

```rust
match Post::load_editor_strict(&client, posts).await {
    Ok(loaded) => { /* every post has an editor */ }
    Err(e) => println!("missing editor: {e}"),
}
```

## `has_one`

Declares a one-to-one relationship where the foreign key lives on the child table. Similar to `has_many` but expects at most one child per parent.

### Definition

```rust
#[derive(Debug, Clone, FromRow, Model)]
#[orm(table = "users")]
#[orm(has_one(Profile, foreign_key = "user_id", as = "profile"))]
struct User {
    #[orm(id)]
    id: i64,
    name: String,
}

#[derive(Debug, Clone, FromRow, Model)]
#[orm(table = "profiles")]
struct Profile {
    #[orm(id)]
    id: i64,
    user_id: i64,
    bio: String,
}
```

### Generated Methods

For `as = "profile"`:

| Method | Return type | Description |
|--------|-------------|-------------|
| `User::load_profile_map(conn, &users)` | `HasOneMap<i64, Profile>` | Map of parent PK to child |
| `User::load_profile_map_with(conn, &users, \|q\| { ... })` | `HasOneMap<i64, Profile>` | With query customization |
| `User::load_profile_map_strict(conn, &users)` | `HasOneMap<i64, Profile>` | Error if duplicate children found |
| `User::load_profile_map_strict_with(conn, &users, \|q\| { ... })` | `HasOneMap<i64, Profile>` | Strict with query customization |
| `User::load_profile(conn, users)` | `Vec<Loaded<User, Option<Profile>>>` | Attach style |
| `User::load_profile_with(conn, users, \|q\| { ... })` | `Vec<Loaded<User, Option<Profile>>>` | Attach with customization |

## `many_to_many`

Declares a many-to-many relationship through a join table.

### Definition

```rust
#[derive(Debug, Clone, FromRow, Model)]
#[orm(table = "posts")]
#[orm(many_to_many(Tag,
    through = "post_tags",
    self_key = "post_id",
    other_key = "tag_id",
    as = "tags"
))]
struct Post {
    #[orm(id)]
    id: i64,
    title: String,
}

#[derive(Debug, Clone, FromRow, Model)]
#[orm(table = "tags")]
struct Tag {
    #[orm(id)]
    id: i64,
    name: String,
}
```

Parameters:
- `Tag` -- the related model type
- `through = "post_tags"` -- the join table name
- `self_key = "post_id"` -- join table column referencing the current model
- `other_key = "tag_id"` -- join table column referencing the related model
- `as = "tags"` -- the relation name

### Generated Methods

For `as = "tags"`:

| Method | Return type | Description |
|--------|-------------|-------------|
| `Post::load_tags_map(conn, &posts)` | `HasManyMap<i64, Tag>` | Map of parent PK to children |
| `Post::load_tags_map_with(conn, &posts, \|q\| { ... })` | `HasManyMap<i64, Tag>` | With query customization |
| `Post::load_tags(conn, posts)` | `Vec<Loaded<Post, Vec<Tag>>>` | Attach style |
| `Post::load_tags_with(conn, posts, \|q\| { ... })` | `Vec<Loaded<Post, Vec<Tag>>>` | Attach with customization |

## Naming Convention

Pick a business-friendly `as` name for each relation:

- `as = "posts"` generates `load_posts_map`, `load_posts`, etc.
- `as = "author"` generates `load_author`, `load_author_strict`, etc.

Keep `as` names stable -- renaming them changes all generated method names, which is a breaking change in your code.

---

Next: [PostgreSQL Types](/en/guide/pg-types)
