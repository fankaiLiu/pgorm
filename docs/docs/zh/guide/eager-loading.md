# 预加载

pgorm 支持显式的关系预加载辅助方法。除非调用 `load_*`，否则不会执行额外查询。

## Map 风格（推荐）

每个关系一个额外查询，返回按主键索引的映射：

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

    // 一次查询加载所有用户的文章
    let posts_by_user = User::load_posts_map_with(conn, &users, |q| {
        q.push(" ORDER BY id DESC");
    })
    .await?;

    for u in &users {
        let posts = posts_by_user.get(u.pk()).map(Vec::as_slice).unwrap_or(&[]);
        println!("用户 {} 有 {} 篇文章", u.name, posts.len());
    }

    Ok(())
}
```

## Attach 风格

保持基础顺序，将关系数据附加到每一行：

```rust
async fn list_posts(conn: &impl GenericClient) -> pgorm::OrmResult<()> {
    let posts = Post::select_all(conn).await?;

    // 为每篇文章附加作者
    let posts_with_author = Post::load_author(conn, posts).await?;

    Ok(())
}
```

## 严格变体

使用严格变体要求每个基础行都必须存在关系：

```rust
// 如果任何文章没有作者将会报错
let posts_with_author = Post::load_author_strict(conn, posts).await?;
```

## 下一步

- 下一章：[`写入：InsertModel`](/zh/guide/insert-model)
