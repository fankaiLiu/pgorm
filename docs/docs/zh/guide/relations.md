# 关系与预加载

在 pgorm 中，关系是**在模型上显式声明**并**通过生成的辅助方法显式加载**的。除非你调用 `load_*` 方法，否则不会执行额外查询 -- 不存在意外的 N+1 查询问题。

## 概览

pgorm 支持四种关系类型：

| 关系 | 属性 | 父模型拥有... | 生成的映射类型 |
|------|------|--------------|--------------|
| `has_many` | `#[orm(has_many(...))]` | 多个子记录 | `HasManyMap<PK, Vec<Child>>` |
| `has_one` | `#[orm(has_one(...))]` | 一个子记录（0..1） | `HasOneMap<PK, Child>` |
| `belongs_to` | `#[orm(belongs_to(...))]` | 一个父记录 | `HashMap<FK, Parent>` |
| `many_to_many` | `#[orm(many_to_many(...))]` | 多个（通过关联表） | `HasManyMap<PK, Vec<Child>>` |

## `has_many`

声明一对多关系。外键位于子表上。

### 定义

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

参数说明：
- `Post` -- 子模型类型
- `foreign_key = "user_id"` -- 子表上的外键列
- `as = "posts"` -- 关系名称（决定生成的方法名）

### 生成的方法

对于 `as = "posts"`，会在 `User` 上生成以下方法：

| 方法 | 返回类型 | 描述 |
|------|---------|------|
| `User::load_posts_map(conn, &users)` | `HasManyMap<i64, Post>` | 批量加载到以父主键为键的映射中 |
| `User::load_posts_map_with(conn, &users, \|q\| { ... })` | `HasManyMap<i64, Post>` | 同上，但可自定义预加载查询 |
| `User::load_posts(conn, users)` | `Vec<Loaded<User, Vec<Post>>>` | 附加风格：将子记录附加到每个父记录上 |
| `User::load_posts_with(conn, users, \|q\| { ... })` | `Vec<Loaded<User, Vec<Post>>>` | 附加风格，支持查询自定义 |

### 映射风格（推荐）

每个关系只需一次额外查询，返回以主键为索引的映射：

```rust
use pgorm::ModelPk as _;

let users = User::select_all(&client).await?;

// 一次查询加载这些用户的所有文章
let posts_by_user = User::load_posts_map(&client, &users).await?;

for user in &users {
    let posts = posts_by_user.get(user.pk()).unwrap_or(&vec![]);
    println!("{} has {} posts", user.name, posts.len());
}
```

### 自定义预加载查询

使用 `_with` 变体为预加载添加排序、过滤或限制条件：

```rust
let posts_by_user = User::load_posts_map_with(&client, &users, |q| {
    q.push(" ORDER BY id DESC");
}).await?;
```

### 附加风格

返回 `Vec<Loaded<Parent, Vec<Child>>>`，每个父记录都附加了其子记录。输出顺序与输入顺序一致。

```rust
let users = User::select_all(&client).await?;
let users_with_posts = User::load_posts(&client, users).await?;

for u in &users_with_posts {
    // u.base 是 User，u.rel 是 Vec<Post>
    println!("user {} has {} posts", u.base.name, u.rel.len());
}
```

## `belongs_to`

声明多对一（或一对一）关系。外键位于当前模型上。

### 定义

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

你可以对同一模型声明多个 `belongs_to` 关系，使用不同的外键和不同的 `as` 名称。

### 生成的方法

对于 `as = "author"`：

| 方法 | 返回类型 | 描述 |
|------|---------|------|
| `Post::load_author(conn, posts)` | `Vec<Loaded<Post, Option<User>>>` | 将父记录附加到每个子记录上 |
| `Post::load_author_with(conn, posts, \|q\| { ... })` | `Vec<Loaded<Post, Option<User>>>` | 附加并支持查询自定义 |
| `Post::load_author_strict(conn, posts)` | `Vec<Loaded<Post, User>>` | 严格模式：缺少父记录时返回错误 |
| `Post::load_author_strict_with(conn, posts, \|q\| { ... })` | `Vec<Loaded<Post, User>>` | 严格模式，支持查询自定义 |
| `Post::load_author_map(conn, &posts)` | `HashMap<FK, User>` | 外键到父记录的映射 |

### 用法

```rust
let posts = Post::select_all(&client).await?;

// 默认：Option<User>（FK 为 NULL 或父记录缺失时为 None）
let posts_with_author = Post::load_author(&client, posts.clone()).await?;
for p in &posts_with_author {
    let author = p.rel.as_ref()
        .map(|u| u.name.as_str())
        .unwrap_or("(missing)");
    println!("post {} by {}", p.base.title, author);
}

// 可空外键（editor_id 是 Option<i64>）
let posts_with_editor = Post::load_editor(&client, posts.clone()).await?;
for p in &posts_with_editor {
    let editor = p.rel.as_ref()
        .map(|u| u.name.as_str())
        .unwrap_or("(null)");
    println!("post {} editor: {}", p.base.title, editor);
}
```

### 严格变体

严格变体在任何行缺少父记录时返回错误（例如外键引用了已删除的记录）：

```rust
match Post::load_editor_strict(&client, posts).await {
    Ok(loaded) => { /* 每篇文章都有编辑者 */ }
    Err(e) => println!("missing editor: {e}"),
}
```

## `has_one`

声明一对一关系，外键位于子表上。类似于 `has_many`，但期望每个父记录最多有一个子记录。

### 定义

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

### 生成的方法

对于 `as = "profile"`：

| 方法 | 返回类型 | 描述 |
|------|---------|------|
| `User::load_profile_map(conn, &users)` | `HasOneMap<i64, Profile>` | 父主键到子记录的映射 |
| `User::load_profile_map_with(conn, &users, \|q\| { ... })` | `HasOneMap<i64, Profile>` | 支持查询自定义 |
| `User::load_profile_map_strict(conn, &users)` | `HasOneMap<i64, Profile>` | 发现重复子记录时返回错误 |
| `User::load_profile_map_strict_with(conn, &users, \|q\| { ... })` | `HasOneMap<i64, Profile>` | 严格模式，支持查询自定义 |
| `User::load_profile(conn, users)` | `Vec<Loaded<User, Option<Profile>>>` | 附加风格 |
| `User::load_profile_with(conn, users, \|q\| { ... })` | `Vec<Loaded<User, Option<Profile>>>` | 附加风格，支持自定义 |

## `many_to_many`

声明多对多关系，通过关联表实现。

### 定义

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

参数说明：
- `Tag` -- 关联模型类型
- `through = "post_tags"` -- 关联表名称
- `self_key = "post_id"` -- 关联表中引用当前模型的列
- `other_key = "tag_id"` -- 关联表中引用关联模型的列
- `as = "tags"` -- 关系名称

### 生成的方法

对于 `as = "tags"`：

| 方法 | 返回类型 | 描述 |
|------|---------|------|
| `Post::load_tags_map(conn, &posts)` | `HasManyMap<i64, Tag>` | 父主键到子记录列表的映射 |
| `Post::load_tags_map_with(conn, &posts, \|q\| { ... })` | `HasManyMap<i64, Tag>` | 支持查询自定义 |
| `Post::load_tags(conn, posts)` | `Vec<Loaded<Post, Vec<Tag>>>` | 附加风格 |
| `Post::load_tags_with(conn, posts, \|q\| { ... })` | `Vec<Loaded<Post, Vec<Tag>>>` | 附加风格，支持自定义 |

## 命名约定

为每个关系选择面向业务的 `as` 名称：

- `as = "posts"` 会生成 `load_posts_map`、`load_posts` 等方法
- `as = "author"` 会生成 `load_author`、`load_author_strict` 等方法

保持 `as` 名称稳定 -- 重命名会改变所有生成的方法名，这是代码中的破坏性变更。

---

下一步：[PostgreSQL 类型](/zh/guide/pg-types)
