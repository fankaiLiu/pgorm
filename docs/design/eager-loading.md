# Eager Loading（批量预加载关联）

- 状态：Proposal（可分阶段实现）
- 目标版本：0.2.x（暂定）
- 最后更新：2026-01-29

> 参考来源：ActiveRecord / Ecto / Prisma；但 pgorm 坚持 **SQL-first** 与 **最小魔法**。

## 0. 背景

`#[derive(Model)]` 目前已经支持用属性声明简单关联，并生成按记录查询的便捷方法：

- `#[orm(has_many(Post, foreign_key = "user_id", as = "posts"))]` → `user.select_posts(&conn)`
- `#[orm(belongs_to(User, foreign_key = "user_id", as = "author"))]` → `post.select_author(&conn)`

这些方法直观，但在列表场景会触发典型的 N+1 查询：

```rust
use pgorm::{GenericClient, ModelPk as _, OrmResult};

// ❌ N+1：1 + N
async fn list_users_with_posts_n_plus_1(conn: &impl GenericClient) -> OrmResult<()> {
    let users = User::select_all(conn).await?;
    for u in &users {
        let _posts = u.select_posts(conn).await?;
    }
    Ok(())
}

// ✅ Eager Loading：2 次（users + posts）
async fn list_users_with_posts_eager(conn: &impl GenericClient) -> OrmResult<()> {
    let users = User::select_all(conn).await?;
    let posts_by_user = User::load_posts_map(conn, &users).await?;
    for u in &users {
        let _posts = posts_by_user.get(u.pk());
    }
    Ok(())
}
```

Eager Loading 的目标是：在不牺牲 pgorm 的“显式 SQL”哲学下，提供一套可落地、Rust 风格的批量加载工具。

## 1. 设计目标

### Goals

- **显式**：只有调用 `load_*` 才会额外查询；不会暗中触发查询
- **不侵入模型**：不要求用户在 `User` 上增加 `posts: Vec<Post>` 之类字段
- **不要求字段 `pub`**：跨模块依然可用（不依赖直接读写其它类型的私有字段）
- **可预测**：输出顺序与输入 base 顺序一致；空输入快速返回
- **默认多查询**：每个关联 1 条额外 SQL（`ANY($1)` 绑定数组，避免 `$1..$N` 爆炸）

### Non-goals（暂不做）

- 自动递归嵌套 include（先把 1 层做对）
- per-parent limit / top-N（需要窗口函数或 lateral join，后续再议）
- “自动选择 Join/MultiQuery” 策略（pgorm 提供工具，策略由用户显式选择/手写 SQL）

## 2. 对外 API 形态（提案）

pgorm 适合两种使用方式：`Map` 方式（最 Rust、最不侵入）和 `Attach` 方式（遍历更顺手）。

> 注：当前代码只提供 `select_*`（单记录关联）。下文的 `load_*` 属于拟新增/宏生成的 API。

### 2.1 Map 方式（推荐默认）

`#[derive(Model)]` 在声明了关联后，为该关联生成批量加载方法：

```rust
#[derive(pgorm::Model, pgorm::FromRow)]
#[orm(table = "users")]
#[orm(has_many(Post, foreign_key = "user_id", as = "posts"))]
pub struct User {
    #[orm(id)]
    id: i64,
    name: String,
}

#[derive(pgorm::Model, pgorm::FromRow)]
#[orm(table = "posts")]
pub struct Post {
    #[orm(id)]
    id: i64,
    user_id: i64,
    title: String,
}

use pgorm::ModelPk as _;

let users = User::query().eq("status", "active")?.find(&client).await?;

// 只多 1 条查询：按 user_id 批量加载 posts
let posts_by_user = User::load_posts_map(&client, &users).await?;

for u in &users {
    let posts = posts_by_user.get(u.pk()).map(Vec::as_slice).unwrap_or(&[]);
    // ...
}
```

建议返回类型：

```rust
pub type HasManyMap<Id, Child> = std::collections::HashMap<Id, Vec<Child>>;
pub type BelongsToMap<Id, Parent> = std::collections::HashMap<Id, Parent>;
```

特点：

- 不改变 `users: Vec<User>` 的类型（便于继续复用）
- 可以把 `posts_by_user` 传递给其它层（service / handler）

### 2.2 Attach 方式（可选）

如果你希望直接迭代“带关联的行”，提供 wrapper：

```rust
pub struct Loaded<M, R> {
    pub base: M,
    pub rel: R,
}

impl<M, R> std::ops::Deref for Loaded<M, R> {
    type Target = M;
    fn deref(&self) -> &M {
        &self.base
    }
}
```

并为关联生成：

- `User::load_posts(&client, users) -> OrmResult<Vec<Loaded<User, Vec<Post>>>>`
- `Post::load_author(&client, posts) -> OrmResult<Vec<Loaded<Post, Option<User>>>>`

## 3. SQL 形态（默认 MultiQuery）

### 3.1 has_many

给定 `users: &[User]`：

- base 已经由用户查询得到（`select_all` / `query().find()` 等）
- eager load 只负责再跑 1 条 relation 查询：

```sql
SELECT <post_columns>
FROM posts
WHERE user_id = ANY($1)
```

参数 `$1` 是 parent 主键的数组参数（例如 `Vec<i64>` / `Vec<uuid::Uuid>`）。

### 3.2 belongs_to

给定 `posts: &[Post]`：

```sql
SELECT <user_columns>
FROM users
WHERE id = ANY($1)
```

并在内存中按 `post.user_id -> user.id` 组装。

## 4. 关键实现点（Rust / pgorm 风格）

### 4.1 不依赖子表字段可见性（has_many）

`has_many` 的分组需要 child 的外键值。为了避免必须访问 `Post.user_id`（可能是私有字段），实现上直接从 `Row` 读取外键列：

```rust
use pgorm::{FromRow, GenericClient, OrmResult, TableMeta, sql};
use std::collections::HashMap;

pub async fn load_has_many_map<Child, Id>(
    conn: &impl GenericClient,
    parent_ids: Vec<Id>,
    fk_col: &'static str,
) -> OrmResult<HashMap<Id, Vec<Child>>>
where
    Child: FromRow + TableMeta,
    Id: tokio_postgres::types::ToSql
        + tokio_postgres::types::FromSqlOwned
        + Eq
        + std::hash::Hash
        + Send
        + Sync
        + 'static,
{
    if parent_ids.is_empty() {
        return Ok(HashMap::new());
    }

    // SELECT <columns> FROM <table> WHERE <fk_col> = ANY($1)
    let select_list = Child::columns().join(", ");
    let mut q = sql("SELECT ");
    q.push(&select_list).push(" FROM ");
    q.push_ident(Child::table_name())?;
    q.push(" WHERE ");
    q.push_ident(fk_col)?;
    q.push(" = ANY(").push_bind(parent_ids).push(")");

    let rows = q.fetch_all(conn).await?;

    let mut out: HashMap<Id, Vec<Child>> = HashMap::new();
    for row in rows {
        let fk: Id = row.try_get(fk_col)?;
        let child = Child::from_row(&row)?;
        out.entry(fk).or_default().push(child);
    }
    Ok(out)
}
```

这要求：

- 外键列名来自 `#[orm(has_many(... foreign_key = "..."))]`（编译期常量）
- `Id` 实现 `FromSqlOwned`（`i64/uuid::Uuid` 等通常满足）

### 4.2 `ANY($1)` 避免占位符爆炸

相比 `IN ($1, $2, ... $N)`，`ANY($1)` 只占用一个参数位，避免：

- `tokio-postgres` 参数数量上限（65535）
- 超长 SQL 字符串与 parse overhead

### 4.3 顺序语义

- base 输出顺序：严格保持输入 `Vec<M>` 的顺序
- has_many 子列表顺序：默认不保证（除非用户通过 `ORDER BY` 自定义）

### 4.4 可选的 relation query 定制（全局）

提供 `*_with` 变体，让用户在 relation SQL 上附加过滤/排序（全局生效，非 per-parent）：

```rust
let posts_by_user = User::load_posts_map_with(&client, &users, |q| {
    q.push(" AND status = ").push_bind("published");
    q.push(" ORDER BY created_at DESC");
})
.await?;
```

## 5. 与现有功能的关系

- **与 `select_*`（单记录关联）互补**：`select_posts` 适合详情页；`load_posts_*` 适合列表页/批处理
- **与 JOIN 的关系**：对 1:1 场景，如果你更想用单条 SQL，推荐直接定义 `#[derive(ViewModel)]` + `#[orm(join(...))]` 做 JOIN（更贴合 pgorm 的 SQL-first）
- **与 Write Graph 无关**：Write Graph 解决“原子写入”；Eager Loading 解决“批量读取”

## 6. 错误与语义

- has_many：找不到 child 不是错误，返回空 `Vec`
- belongs_to：建议返回 `Option<Parent>`（类似 LEFT JOIN）；可额外提供 `*_strict` 变体在缺失时返回 `OrmError::NotFound`
- decode/SQL 构建错误：沿用现有 `OrmError`（`Query` / `Decode` / `Validation`）

## 7. 实现检查清单

- [ ] 在 `pgorm` 增加 `eager` 模块：`Loaded`、通用装配 helpers
- [ ] 扩展 `#[derive(Model)]`：为 `has_many` / `belongs_to` 生成 `load_*_map` / `load_*` 方法
- [ ] 增加单元测试：空输入、重复 id、缺失 parent、NULL fk（如果支持 Option fk）
- [ ] 文档：在 README 或 guide 中补充 eager-loading 用法

> 执行进度与更细粒度的拆解：见 `docs/design/eager-loading.todo.md`。
