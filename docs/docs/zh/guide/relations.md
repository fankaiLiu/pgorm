# 关系声明：`has_many` / `belongs_to`

pgorm 的关系是**显式声明 + 显式预加载**：

- 在模型上用 `#[orm(has_many(...))]` / `#[orm(belongs_to(...))]` 声明关系
- 只有你调用 `load_*` 时才会额外执行查询（避免 ORM “暗中 N+1”）

这一页只讲“如何声明关系、会生成哪些方法”。预加载的用法见：[`预加载`](/zh/guide/eager-loading)。

## 1) `has_many`：一对多

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

参数解释：

- `Post`：子表模型类型
- `foreign_key = "user_id"`：子表里指向父表主键的外键列
- `as = "posts"`：关系名（会影响生成的方法名）

常见生成方法（以 `as = "posts"` 为例）：

- `User::load_posts_map(conn, &users)`：一次性加载所有 posts，按 user 主键分组返回 `HashMap<PK, Vec<Post>>`
- `User::load_posts_map_with(conn, &users, |q| { ... })`：允许你在预加载 SQL 上追加片段（例如 ORDER BY）

## 2) `belongs_to`：多对一

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

常见生成方法（以 `as = "author"` 为例）：

- `Post::load_author(conn, posts)`：把作者附加到每一行（返回新的 Vec）
- `Post::load_author_strict(conn, posts)`：严格版；缺任何作者直接报错

> 具体返回类型与使用方式见：[`预加载`](/zh/guide/eager-loading)。

## 3) 关系名（`as`）如何选？

建议遵循两个原则：

1) **面向业务语义**：`author`、`posts` 比 `user`、`children` 更可读  
2) **稳定**：因为它会影响方法名（重命名会是 breaking change）  

## 4) 常见坑

1) `foreign_key` 是 **列名**（字符串），会做标识符校验；不要带空格/表达式。  
2) 关系只影响“预加载辅助方法生成”，不会替你自动 JOIN。需要 JOIN 时，建议写显式 SQL。  

## 下一步

- 下一章：[`预加载`](/zh/guide/eager-loading)
