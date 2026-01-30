# 模型与派生宏

pgorm 提供了多个派生宏来处理数据库模型。

## FromRow

`FromRow` 派生宏将数据库行映射到 Rust 结构体：

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

`Model` 派生宏提供 CRUD 操作和关系辅助方法：

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

### 表名

使用 `#[orm(table = "table_name")]` 指定数据库表名。

### 主键

使用 `#[orm(id)]` 标记主键字段。

## 关系

### has_many

定义一对多关系：

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

定义多对一关系：

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

## JSONB 支持

pgorm 支持 PostgreSQL JSONB 列：

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
    meta: Json<Meta>, // jsonb 列
}
```

## 下一步

- 下一章：[`关系声明：has_many / belongs_to`](/zh/guide/relations)
