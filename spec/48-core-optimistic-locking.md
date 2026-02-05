# 乐观锁（Optimistic Locking）设计与计划

状态：Implemented (M1-M2)
相关代码：`crates/pgorm` / `crates/pgorm-derive`
最后更新：2026-02-05

## 背景

在高并发场景下，多个事务同时修改同一条记录可能导致"后写覆盖前写"的问题（Lost Update）。乐观锁通过版本号机制，在更新时检查版本是否变化，从而防止并发冲突。

当前 `pgorm` 的 `UpdateModel` 不支持自动版本检查，用户需要手写 `WHERE version = $old_version` 并手动处理冲突。

## 目标 / 非目标

### 目标

1. 提供 `#[orm(version)]` 字段属性，标记版本号字段（支持 `i32/i64/i16`）。
2. `update_by_id*` 方法自动在 WHERE 子句中加入版本检查：`WHERE id = $1 AND version = $2`。
3. 更新时自动递增版本号：`SET version = version + 1`。
4. 当版本冲突（affected_rows = 0）时，返回明确的错误类型 `OrmError::StaleRecord`。
5. 提供 `update_by_id_force*` 方法跳过版本检查（用于管理员覆盖等场景）。

### 非目标

- 悲观锁（`SELECT ... FOR UPDATE`）的封装（可作为独立 RFC）。
- 分布式锁或跨数据库的版本控制。
- 自动重试机制（应由业务层决定重试策略）。

## 方案

### 1) 字段属性 `#[orm(version)]`

```rust,ignore
#[derive(Model, UpdateModel)]
#[orm(table = "posts")]
pub struct Post {
    pub id: i64,
    pub title: String,
    pub content: String,
    #[orm(version)]
    pub version: i32,
}
```

约束：
- 一个 Model 最多只能有一个 `#[orm(version)]` 字段。
- 类型必须是整数类型（`i16/i32/i64`）。
- 字段名可以自定义（默认推荐 `version`）。

### 2) 生成的 SQL

原始（无乐观锁）：

```sql
UPDATE posts SET title = $1, content = $2 WHERE id = $3
```

启用乐观锁后：

```sql
UPDATE posts
SET title = $1, content = $2, version = version + 1
WHERE id = $3 AND version = $4
```

### 3) 错误处理

```rust,ignore
pub enum OrmError {
    // ...existing variants...

    /// 乐观锁冲突：记录已被其他事务修改
    StaleRecord {
        table: &'static str,
        id: String,
        expected_version: i64,
    },
}
```

使用示例：

```rust,ignore
let patch = PostPatch {
    id: 1,
    title: Some("New Title".into()),
    version: 5, // 当前版本
};

match patch.update_by_id(&client).await {
    Ok(()) => println!("Updated successfully"),
    Err(OrmError::StaleRecord { .. }) => {
        // 重新获取最新数据，提示用户或自动重试
        println!("Record was modified by another user");
    }
    Err(e) => return Err(e),
}
```

### 4) 强制更新（跳过版本检查）

```rust,ignore
impl PostPatch {
    /// 跳过版本检查的更新（用于管理员覆盖）
    pub async fn update_by_id_force<C: GenericClient>(&self, client: &C) -> OrmResult<()>;

    /// 带 RETURNING 的强制更新
    pub async fn update_by_id_force_returning<C: GenericClient>(&self, client: &C) -> OrmResult<Post>;
}
```

### 5) 与 `update_by_id_returning` 的集成

带 RETURNING 的更新会返回新版本号：

```rust,ignore
let updated_post = patch.update_by_id_returning(&client).await?;
assert_eq!(updated_post.version, 6); // 版本已递增
```

## API 设计

### derive 属性

```rust,ignore
#[derive(UpdateModel)]
#[orm(table = "posts")]
pub struct PostPatch {
    pub id: i64,

    #[orm(skip_update)] // 可选：不更新 title
    pub title: Option<String>,

    pub content: Option<String>,

    #[orm(version)] // 标记版本字段
    pub version: i32,
}
```

### 生成的方法

```rust,ignore
impl PostPatch {
    // 带版本检查的更新
    pub async fn update_by_id<C: GenericClient>(&self, client: &C) -> OrmResult<()>;
    pub async fn update_by_id_returning<C: GenericClient>(&self, client: &C) -> OrmResult<Post>;

    // 跳过版本检查（force）
    pub async fn update_by_id_force<C: GenericClient>(&self, client: &C) -> OrmResult<()>;
    pub async fn update_by_id_force_returning<C: GenericClient>(&self, client: &C) -> OrmResult<Post>;
}
```

## 使用示例

### A) 基本用法

```rust,ignore
use pgorm::prelude::*;

#[derive(Model, UpdateModel, FromRow)]
#[orm(table = "articles")]
pub struct Article {
    pub id: i64,
    pub title: String,
    pub body: String,
    #[orm(version)]
    pub version: i32,
}

// 获取文章
let article: Article = pgorm::query("SELECT * FROM articles WHERE id = $1")
    .bind(1_i64)
    .fetch_one(&client)
    .await?;

// 准备更新
let patch = ArticlePatch {
    id: article.id,
    title: Some("Updated Title".into()),
    body: None, // 不更新
    version: article.version, // 传入当前版本
};

// 更新（自动检查版本）
match patch.update_by_id(&client).await {
    Ok(()) => println!("Success!"),
    Err(OrmError::StaleRecord { expected_version, .. }) => {
        println!("Conflict! Expected version {}", expected_version);
    }
    Err(e) => return Err(e.into()),
}
```

### B) 与事务结合

```rust,ignore
pgorm::transaction!(client, async |tx| {
    let article = pgorm::query("SELECT * FROM articles WHERE id = $1 FOR UPDATE")
        .bind(1_i64)
        .fetch_one::<Article>(tx)
        .await?;

    let patch = ArticlePatch {
        id: article.id,
        title: Some("New Title".into()),
        body: None,
        version: article.version,
    };

    patch.update_by_id(tx).await?;
    Ok(())
});
```

### C) 管理员强制覆盖

```rust,ignore
// 管理员场景：忽略版本冲突
let admin_patch = ArticlePatch {
    id: 1,
    title: Some("Admin Override".into()),
    body: None,
    version: 0, // 版本会被忽略
};

admin_patch.update_by_id_force(&client).await?;
```

## 关键取舍

| 选项 | 优点 | 缺点 | 决策 |
|------|------|------|------|
| 版本号自动递增 vs 手动设置 | 自动递增更安全，防止用户忘记 | 灵活性降低 | **自动递增** |
| 返回错误 vs 返回 affected_rows | 错误语义更明确 | 需要新增错误类型 | **返回 StaleRecord 错误** |
| 默认启用 vs 显式启用 | 显式更清晰 | 需要加属性 | **显式 `#[orm(version)]`** |

## 与现有功能的关系

- **write_graph**：多表更新时，每个带 `#[orm(version)]` 的表都会检查版本。
- **update_by_id_returning**：返回更新后的记录，包含新版本号。
- **事务**：乐观锁 + `FOR UPDATE` 可以结合使用（先锁再改）。

## 兼容性与迁移

- 纯新增功能，不影响现有 `UpdateModel` 行为。
- 现有代码无需修改，只有显式加了 `#[orm(version)]` 才会启用版本检查。
- 数据库迁移：需要用户自行添加 `version` 列（INT DEFAULT 0）。

## 里程碑 / TODO

### M1（核心实现）

- [x] `#[orm(version)]` 属性解析
- [x] 生成带版本检查的 SQL
- [x] `OrmError::StaleRecord` 错误类型
- [x] 单元测试

### M2（扩展方法）

- [x] `update_by_id_force*` 方法
- [x] `update_by_id_returning` 版本递增验证
- [ ] 集成测试（需要数据库）

### M3（文档与示例）

- [x] `examples/optimistic_locking`
- [ ] 中英文文档
- [ ] README 更新

## Open Questions

1. 版本字段是否支持 `uuid`/`timestamp`？（建议 M1 只支持整数，后续扩展）
2. 批量更新 `update_many` 是否支持乐观锁？（建议不支持，批量场景下版本检查复杂度高）
3. 是否提供 `on_conflict_version` 回调？（建议不提供，由业务层决定重试逻辑）
