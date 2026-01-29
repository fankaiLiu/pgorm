# pgorm 设计文档：多表写入（Write Graph）最终方案

- 状态：Final Proposal（允许分阶段实现，但不改变对外语义与 API 形态）
- 目标版本：0.1.x
- 最后更新：2026-01-29

> 本文是 `multi-table-writes.md` 的“最终自洽版”。核心原则：**不依赖跨模块字段可见性**、API 易用、实现可落地、行为可预期。

---

## 0. 背景与问题

当前 `pgorm-derive` 的写入宏（`InsertModel` / `UpdateModel`）只支持单表写入，且禁止 joined/other table 字段。

业务层虽可用 `pgorm::transaction!` 手写多条 SQL 达到原子性，但缺少：

- 声明式的“写入图（write graph）”组织方式；
- 主从级联（主表 + 多个子表）的统一策略（replace/append/upsert/diff）；
- belongs_to 前置依赖（先写父表得到 id，再写主表外键）；
- 统一的步骤统计与可观测性（按步骤 tag 的 affected rows）。

## 1. 设计目标

### Goals

- 支持 **Insert Graph**：root insert + belongs_to + has_one/has_many + before/after steps
- 支持 **Update Graph**：root patch update + 子表策略化更新（replace/append/upsert/diff）
- **不要求模型字段 `pub`**：跨模块场景也能用（这是最终方案的硬约束）
- 事务友好：以 `&impl pgorm::GenericClient` 为入口，天然支持传事务；另提供可选 `*_atomic` 便捷入口
- 可观测：每一步都有稳定 tag，并可返回 `WriteReport`

### Non-goals（明确不做）

- 不做任意深度自动 diff（先支持 1 层主从；多层可递归扩展但不在本文强约束）
- 不做“最少 SQL”承诺（可以多语句；未来优化为 CTE/单语句不改变语义）

---

## 2. 核心难点与最终解法：跨模块字段可见性

**难点**：graph 代码生成在“root struct 的 impl”里，如果它需要访问其它类型的字段（如 `created.id` 或 `child.order_id = ...`），当这些字段不是 `pub` 且类型在其它模块时，会直接编译失败。

**最终解法**：Write Graph 生成代码 **只通过公开方法/trait** 与其它类型交互，不直接读写字段。

### 2.1 `ModelPk`：获取 returning 的主键值

在 `pgorm` 中新增 trait：

```rust
pub trait ModelPk {
    type Id: Clone + Send + Sync + 'static;
    fn pk(&self) -> &Self::Id;
}
```

并由 `#[derive(Model)]` 自动实现（使用被标记为 `#[orm(id)]` 的 Rust 字段）：

```rust
impl pgorm::ModelPk for Order {
    type Id = i64;
    fn pk(&self) -> &Self::Id { &self.id }
}
```

Write Graph 需要 root_id（用于子表 fk 注入）时，一律使用：

```rust
let root_id = created.pk().clone();
```

> returning 类型若不是 `Model`（只是 `FromRow`），用户可自行 `impl ModelPk`（在其模块内可访问私有字段）。
>
> 约束：当前 pgorm 的 `Model` 只支持单个 `#[orm(id)]` 字段，因此 `ModelPk` 也只覆盖单列主键。

### 2.2 `with_*`：给 InsertModel 生成“可注入”的 setter

为避免 `child.fk_field = root_id` 的字段写入，`#[derive(InsertModel)]` 对每个字段生成 builder-style setter：

- 对 `T` 字段：

```rust
impl NewOrderItem {
    pub fn with_order_id(mut self, v: i64) -> Self {
        self.order_id = v;
        self
    }
}
```

- 对 `Option<T>` 字段（fk 常见形态）：建议生成两种：

```rust
impl NewOrderItem {
    pub fn with_order_id(mut self, v: i64) -> Self {
        self.order_id = Some(v);
        self
    }

    pub fn with_order_id_opt(mut self, v: Option<i64>) -> Self {
        self.order_id = v;
        self
    }
}
```

这样 graph 注入 fk 时永远可以写成：

```rust
child = child.with_order_id(root_id.clone());
```

无需关心 fk 字段是否为 `Option<Id>`，也不要求字段 `pub`。

---

## 3. 对外类型：WriteReport（已在 pgorm 中存在）

Write Graph 的详细返回建议统一为：

```rust
pub struct WriteReport<R> {
    pub affected: u64,
    pub steps: Vec<WriteStepReport>,
    pub root: Option<R>,
}

pub struct WriteStepReport {
    pub tag: &'static str,
    pub affected: u64,
}
```

要求：

- graph 内每一步 SQL 走 `execute_tagged/query_one_tagged/...`（`GenericClient` 已支持 tagged 默认实现）
- tag 稳定且可预测，例如：
  - `graph:belongs_to:categories`
  - `graph:root:orders`
  - `graph:has_many:order_items`

---

## 4. 最终用户 API（生成矩阵）

### 4.1 InsertModel：root + graph

当一个 `InsertModel` 上声明了任意 graph 元素（belongs_to/has_one/has_many/before_insert/after_insert），生成：

```rust
pub async fn insert_graph(self, conn: &impl pgorm::GenericClient) -> pgorm::OrmResult<u64>;
pub async fn insert_graph_report(
    self,
    conn: &impl pgorm::GenericClient,
) -> pgorm::OrmResult<pgorm::WriteReport<()>>;
```

当该 `InsertModel` 同时配置了 `#[orm(returning = "R")]` 时（并且需要 root returning 或用户想要 root returning），额外生成：

```rust
pub async fn insert_graph_returning(
    self,
    conn: &impl pgorm::GenericClient,
) -> pgorm::OrmResult<R>
where
    R: pgorm::FromRow;

pub async fn insert_graph_report_returning(
    self,
    conn: &impl pgorm::GenericClient,
) -> pgorm::OrmResult<pgorm::WriteReport<R>>
where
    R: pgorm::FromRow;
```

可选增强（更易用，但不影响默认用法）：自动开事务（需要新增 trait，见 §8）：

```rust
pub async fn insert_graph_atomic(
    self,
    client: &mut impl pgorm::TransactionStarter,
) -> pgorm::OrmResult<R>;
```

### 4.2 UpdateModel：patch + graph

当一个 `UpdateModel` 上声明了子表更新策略（has_one_update/has_many_update），生成：

```rust
pub async fn update_by_id_graph<I>(
    self,
    conn: &impl pgorm::GenericClient,
    id: I,
) -> pgorm::OrmResult<u64>
where
    I: ::tokio_postgres::types::ToSql + Sync + Send + 'static;
```

若该 `UpdateModel` 配置了 `#[orm(returning = "R")]`（或可从 `model` 推导），额外生成：

```rust
pub async fn update_by_id_graph_returning<I>(
    self,
    conn: &impl pgorm::GenericClient,
    id: I,
) -> pgorm::OrmResult<R>
where
    I: ::tokio_postgres::types::ToSql + Sync + Send + 'static,
    R: pgorm::FromRow;
```

并建议提供 report 版本（与 Insert 对齐）：

```rust
pub async fn update_by_id_graph_report<I>(
    self,
    conn: &impl pgorm::GenericClient,
    id: I,
) -> pgorm::OrmResult<pgorm::WriteReport<()>>
where
    I: ::tokio_postgres::types::ToSql + Sync + Send + 'static;
```

---

## 5. Root ID 获取规则（消除 `graph_root_key`）

任何需要 root_id 的步骤（has_one/has_many，以及 update 子表策略）都必须能得到 root_id。

最终方案支持两种来源：

### 5.1 Returning（默认）

要求：

- root `InsertModel` 配置 `#[orm(returning = "R")]`
- 并且 `R: pgorm::ModelPk`（推荐 `R` 就是 `#[derive(Model)]` 的类型）

行为：

- 即使调用的是 `insert_graph()`（返回 u64），只要后续需要 root_id，内部也会执行 returning 插入以拿到 root_id（但对外仍返回 u64 / report）。

### 5.2 Input 主键（业务侧提供主键）

适用于 UUID/雪花 id 等“主键由应用生成”的表。

新增属性：

```rust
#[orm(graph_root_id_field = "id")]
```

含义：

- root_id 从 `self.id`（root InsertModel 的 Rust 字段）取得，用于注入子表 fk
- 该字段类型必须是 `Id` 或 `Option<Id>`（Option 为 None 时返回 Validation error）
- 在此模式下，root `returning` 不是必须，但如果用户调用 `insert_graph_returning` 仍可配 returning

优先级：

- 若同时配置 returning 与 `graph_root_id_field`，以 `graph_root_id_field` 为准（显式优先）。

---

## 6. 声明语法（最终统一版）

### 6.1 InsertModel：关系与步骤

#### 6.1.1 has_one / has_many（后置依赖）

```rust
#[orm(has_one(ChildInsert,
    field = "profile",
    fk_field = "user_id",
    mode = "insert" // insert | upsert（默认 insert）
))]

#[orm(has_many(ChildInsert,
    field = "items",
    fk_field = "order_id",
    mode = "insert" // insert | upsert（默认 insert）
))]
```

约束：

- `field` 必须存在于 root struct
  - has_one：`ChildInsert` / `Option<ChildInsert>`
  - has_many：`Vec<ChildInsert>` / `Option<Vec<ChildInsert>>`
- `fk_field` 是 child 的 Rust 字段名，child 必须存在对应 `with_<fk_field>(Id)` setter（由 InsertModel 自动生成）
- 若 has_* 存在但 root_id 无法获取（既无 returning + ModelPk，也无 `graph_root_id_field`），derive 直接报错

#### 6.1.2 belongs_to（前置依赖）

```rust
#[orm(belongs_to(ParentInsert,
    field = "category",
    set_fk_field = "category_id",
    mode = "upsert_returning", // insert_returning | upsert_returning
    required = true
))]
```

语义（最终要求）：

- `set_fk_field` 与 `field` **互斥**：
  - 两者同时提供：Validation error
  - 两者都缺失且 `required = true`：Validation error
- 如果 `set_fk_field` 已有值：跳过 belongs_to 写入
- 否则：
  - 从 `field` 取得 `ParentInsert`
  - 执行 `insert_returning` 或 `upsert_returning`
  - 用返回的 `R: ModelPk` 得到 parent_id，并写回到 root 的 `set_fk_field`

约束：

- ParentInsert 必须配置 returning，并且 returning 类型实现 `ModelPk`
- `set_fk_field` 类型通常为 `Option<Id>`（也允许非 Option：表示必须由 belongs_to 提供）

#### 6.1.3 before_insert / after_insert（纯编排步骤）

用于“同时写两个表但不一定有主从关系”的场景。

```rust
#[orm(before_insert(OtherInsert, field = "x", mode = "insert"))]
#[orm(after_insert(OtherInsert, field = "y", mode = "insert"))]
```

- `field` 支持：`OtherInsert` / `Option<OtherInsert>` / `Vec<OtherInsert>` / `Option<Vec<OtherInsert>>`
- `mode`：
  - `insert`：单个 `.insert(conn)`；Vec `OtherInsert::insert_many(conn, rows)`
  - `upsert`：单个 `.upsert(conn)`；Vec `OtherInsert::upsert_many(conn, rows)`（要求 OtherInsert 支持 upsert）

#### 6.1.4 执行顺序（稳定、可预测）

最终固定顺序：

1. 所有 `belongs_to`（按声明出现顺序）
2. 所有 `before_insert`（按声明出现顺序）
3. root insert（必要时 returning 获取 root_id）
4. 所有 `has_one/has_many`（按声明出现顺序）
5. 所有 `after_insert`（按声明出现顺序）

### 6.2 UpdateModel：子表策略

#### 6.2.1 has_many_update

```rust
#[orm(has_many_update(ChildInsert,
    field = "items",
    fk_column = "order_id",
    fk_field = "order_id",
    strategy = "replace" | "append" | "upsert" | "diff",
    // diff 必填：用这些列定义“保留集合”
    key_columns = "sku",          // 单列
    // 或：key_columns = "sku,variant_id"（多列）
))]
```

约束：

- `field` 推荐类型：`Option<Vec<ChildInsert>>`
  - `None`：不触碰子表
  - `Some(vec)`：按 strategy 执行（空 vec 允许，语义见下）
- `fk_column` 是子表列名（用于 DELETE 过滤）
- `fk_field` 是 child 的 Rust 字段名（用于注入 fk）；child 必须有 `with_<fk_field>(Id)`
- strategy 要求：
  - replace：先 delete，再 insert_many/upsert_many（取决于你选的实现；默认 insert_many）
  - append：只 insert_many
  - upsert：调用 `ChildInsert::upsert_many`（要求 child 支持 upsert）
  - diff：见 §7（最终实现不依赖读取 child key 字段）

空集合语义：

- replace(Some([]))：删除该 fk 下所有子行
- diff(Some([]))：删除该 fk 下所有子行
- append(Some([])) / upsert(Some([]))：no-op

#### 6.2.2 has_one_update

```rust
#[orm(has_one_update(ChildInsert,
    field = "profile",
    fk_column = "user_id",
    fk_field = "user_id",
    strategy = "replace" | "upsert"
))]
```

推荐字段类型：`Option<Option<ChildInsert>>`

- `None`：不触碰
- `Some(None)`：删除（`DELETE FROM child WHERE fk_column = $1`）
- `Some(Some(v))`：
  - replace：删除旧行，再插入 v
  - upsert：对 v 执行 upsert

### 6.3 Update Graph 的 NotFound 语义（避免写出孤儿）

`update_by_id_graph*` 必须保证：如果 root 不存在，不会对 children 做任何修改。

最终规则：

- 若 root patch 有主表字段更新：先执行 UPDATE
  - affected == 0：返回 `OrmError::NotFound`，停止
- 若 root patch **没有任何主表字段更新**但 children 有变更：
  - 必须先执行 `SELECT 1 FROM root WHERE id = $1` 校验存在性
  - 不存在则 NotFound，停止

`update_by_id_graph_returning` 的返回：

- 若执行了 returning UPDATE：直接返回 returning
- 否则（主表无更新但 children 有变更）：在 children 步骤完成后执行一次 `SELECT` 返回 root 行

无操作语义（最终要求）：

- 如果主表没有任何字段更新，且所有子表字段都是 `None`（也没有任何 before/after step）：返回 `OrmError::Validation("WriteGraph: no operations to perform")`。

---

## 7. diff 策略（最终实现：不读 child key 字段）

diff 的难点是“删除缺失项”需要知道 keep keys，但我们不能跨模块直接读 child.key 字段。

最终方案：**用 SQL 自己产生 keep set，不把 keys 拉回 Rust**。

要求：

- child 必须支持 upsert（`conflict_target` 或 `conflict_constraint`，或主键 id 冲突）
- `key_columns` 必须是能唯一标识一行（至少在 fk_scope 内）的列集合（建议与唯一约束一致）

实现策略（单语句 CTE，但 **由 child 的 InsertModel 生成 helper 执行**）：

> 关键点：CTE upsert 需要把 `Vec<ChildInsert>` 展开成多列数组（UNNEST 输入）。这一步必须在 `ChildInsert` 自己的 impl 内完成（才能解构访问私有字段）。  
> 因此最终方案要求：`#[derive(InsertModel)]` 在生成 `upsert_many` 的同时，额外生成一个 `#[doc(hidden)]` 的 diff helper，供 `UpdateModel` 的 graph 代码调用。

建议生成的 helper 形态（名称可内部固定，不承诺稳定）：

```rust
impl ChildInsert {
    #[doc(hidden)]
    pub async fn __pgorm_diff_many_by_fk<I>(
        conn: &impl pgorm::GenericClient,
        fk_column: &'static str,
        fk_value: I,
        key_columns: &'static [&'static str],
        rows: ::std::vec::Vec<Self>,
    ) -> pgorm::OrmResult<u64>
    where
        I: ::tokio_postgres::types::ToSql + Sync + Send + 'static;
}
```

`UpdateModel` 的 graph 在 `strategy = "diff"` 时只做两件事：

1. 先对每个 child 调用 `with_<fk_field>(root_id)` 注入 fk
2. 调用 `ChildInsert::__pgorm_diff_many_by_fk(...)`

1. 用 child 的批量 UNNEST 输入做 upsert，并 `RETURNING key_columns`
2. 立即基于 `RETURNING` 的结果集删除缺失项

SQL 形态（示意，省略类型 cast）：

```sql
WITH upserted AS (
  INSERT INTO child_table (col1, col2, ...)
  SELECT * FROM UNNEST($1::t1[], $2::t2[], ...) AS t(col1, col2, ...)
  ON CONFLICT (...) DO UPDATE SET ...
  RETURNING key1, key2
),
deleted AS (
  DELETE FROM child_table c
  WHERE c.fk_column = $fk
    AND NOT EXISTS (
      SELECT 1 FROM upserted u
      WHERE u.key1 = c.key1 AND u.key2 = c.key2
    )
  RETURNING 1
)
SELECT (SELECT COUNT(*) FROM deleted) AS deleted_count;
```

受影响行数建议定义为：

- `upsert_count = rows.len() as u64`（每个输入行必然插入或更新一次；与 `execute()` 语义一致）
- `affected = upsert_count + deleted_count`

好处：

- 不需要从 Rust 读取 key 值（彻底消除字段可见性问题）
- 支持单列或多列 key
- 可用单语句完成 diff（性能与一致性更好）

---

## 8. Upsert 冲突定义（最终形态）

为支持 belongs_to（按唯一键 upsert 返回 id）与 child diff/upsert，`InsertModel` 必须支持自定义冲突目标。

### 8.1 新增 struct-level 属性

```rust
#[orm(conflict_target = "order_id, sku")]      // 列名列表（逗号分隔）
#[orm(conflict_constraint = "uq_order_items")] // 约束名（ON CONSTRAINT）
#[orm(conflict_update = "qty, price_cents")]   // 可选：指定更新列（列名列表）
```

规则（最终要求）：

- `conflict_target` 与 `conflict_constraint` **互斥**（同时出现直接 derive 报错）
- 未配置上述两者时：
  - 若存在 `#[orm(id)]` 字段：默认以该列为冲突列（保持现有行为）
  - 否则：不生成 upsert 系列方法（调用方编译期看不到 upsert API）

### 8.2 更新列默认规则

- 若配置 `conflict_update`：仅更新这些列（都用 `EXCLUDED.col`）
- 否则：
  - `conflict_target` 模式：更新除冲突列外的所有 insertable 列
  - `conflict_constraint` 模式：更新所有 insertable 列（约束定义唯一性，不排除列）
- 若更新列集合最终为空：生成一个 no-op update（例如把第一列赋值给自己）以满足语法

---

## 9. 事务与并发（最终建议）

### 9.1 原子性

Write Graph 往往包含多语句。最终推荐：

- 业务侧用 `pgorm::transaction!` 包裹，传 `&tx` 给 graph 方法

```rust
pgorm::transaction!(&mut client, tx, {
    let out = new_order.insert_graph_returning(&tx).await?;
    Ok(out)
})?;
```

### 9.2 自动开事务（可选增强）

新增 trait（便于 `_atomic`）：

```rust
pub trait TransactionStarter {
    type Tx<'a>: pgorm::GenericClient
    where
        Self: 'a;

    fn transaction<'a>(
        &'a mut self,
    ) -> impl ::core::future::Future<Output = pgorm::OrmResult<Self::Tx<'a>>> + Send;
}
```

并为 `tokio_postgres::Client` / `deadpool_postgres::Client` 实现。

### 9.3 并发一致性提示

对于 `diff` / `replace`，如果并发写同一 parent 的 children：

- 推荐在事务内执行
- 需要更强一致性时，可在 root 行上 `SELECT ... FOR UPDATE`（由业务决定是否加锁）

---

## 10. 示例（最终版）

### 10.1 Insert：orders + order_items

```rust
use pgorm::{InsertModel, Model, ModelPk};

#[derive(Model)]
#[orm(table = "orders")]
struct Order {
    #[orm(id)]
    id: i64,
    user_id: i64,
    total_cents: i64,
}

#[derive(Clone, InsertModel)]
#[orm(table = "order_items", conflict_target = "order_id, sku")]
struct NewOrderItem {
    order_id: Option<i64>,
    sku: String,
    qty: i32,
}

#[derive(InsertModel)]
#[orm(table = "orders", returning = "Order")]
#[orm(has_many(NewOrderItem, field = "items", fk_field = "order_id"))]
struct NewOrder {
    user_id: i64,
    total_cents: i64,
    items: Vec<NewOrderItem>,
}

let created: Order = NewOrder {
    user_id: 1,
    total_cents: 1000,
    items: vec![
        NewOrderItem { order_id: None, sku: "A".into(), qty: 1 },
        NewOrderItem { order_id: None, sku: "B".into(), qty: 2 },
    ],
}
.insert_graph_returning(&tx)
.await?;

let _id = created.pk();
```

### 10.2 belongs_to：先 upsert category，再插入 product

```rust
#[derive(Model)]
#[orm(table = "categories")]
struct Category {
    #[orm(id)]
    id: i64,
    name: String,
}

#[derive(Clone, InsertModel)]
#[orm(table = "categories", returning = "Category", conflict_target = "name")]
struct NewCategory {
    name: String,
}

#[derive(Model)]
#[orm(table = "products")]
struct Product {
    #[orm(id)]
    id: i64,
    name: String,
    category_id: Option<i64>,
}

#[derive(InsertModel)]
#[orm(table = "products", returning = "Product")]
#[orm(belongs_to(NewCategory,
    field = "category",
    set_fk_field = "category_id",
    mode = "upsert_returning",
    required = true
))]
struct NewProduct {
    name: String,
    category_id: Option<i64>,
    category: Option<NewCategory>,
}
```

### 10.3 Update：diff 同步子表集合

```rust
#[derive(UpdateModel)]
#[orm(table = "orders", model = "Order", returning = "Order")]
#[orm(has_many_update(NewOrderItem,
    field = "items",
    fk_column = "order_id",
    fk_field = "order_id",
    strategy = "diff",
    key_columns = "sku"
))]
struct OrderPatch {
    total_cents: Option<i64>,
    items: Option<Vec<NewOrderItem>>,
}
```

---

## 11. 宏实现要点（确保可落地）

### 11.1 Model derive

- 生成 `impl pgorm::ModelPk for T`（仅当存在 `#[orm(id)]` 字段）
- 无 id 字段时不实现；同时 graph 若依赖 pk 会在 derive 阶段报错（更早、更清晰）

### 11.2 InsertModel derive

- 生成 `with_*` / `with_*_opt` setter（对所有字段）
- 解析并实现 `conflict_target` / `conflict_constraint` / `conflict_update`
- 若生成了 upsert：同时生成 `#[doc(hidden)] __pgorm_diff_many_by_fk(...)`，供 UpdateGraph 的 `diff` 使用
- graph 相关：
  - 解析 has_one/has_many/belongs_to/before_insert/after_insert
  - graph 声明引用的 `field` 自动从 root insert columns 中排除（无需用户写 `skip_insert`）
  - 生成 `insert_graph*`：按固定顺序执行，并记录 `WriteStepReport`

### 11.3 UpdateModel derive

- 解析 has_one_update/has_many_update
- graph 声明引用的 `field` 自动从 root update set_stmts 中排除（无需用户写 `skip_update`）
- 生成 `update_by_id_graph*`：
  - 先保障 root 存在性（见 §6.3）
  - 再按策略执行 children
    - diff：调用 child 的 `__pgorm_diff_many_by_fk`（避免跨模块读 child key 字段）
  - returning 版本在必要时补一次 SELECT
