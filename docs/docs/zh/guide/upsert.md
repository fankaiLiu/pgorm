# 写入：Upsert（`ON CONFLICT`）

pgorm 的 Upsert 是 `InsertModel` 的一部分：你在 `#[orm(...)]` 上声明冲突策略，然后调用 `upsert_*` 系列方法。

## 1) 最小示例：按唯一列 Upsert

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

含义：

- `conflict_target = "name"`：等价于 `ON CONFLICT (name)`
- `conflict_update = "color"`：冲突时更新哪些列

## 2) 用 constraint 做冲突目标

当你的唯一约束/索引名字更稳定，推荐用 constraint：

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

## 3) 批量 Upsert：`upsert_many(_returning)`

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

> 可运行示例见 `crates/pgorm/examples/upsert`。

## 4) 常见坑

1) `conflict_update` 只描述“冲突时更新的列集合”，更复杂的更新表达式建议手写 SQL。  
2) Upsert 仍然是写入操作：如果你需要事务一致性，记得用 [`事务`](/zh/guide/transactions) 把多步写入包起来。  

## 下一步

- 下一章：[`高级写入：Write Graph`](/zh/guide/write-graph)
