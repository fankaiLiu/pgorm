# pgorm 设计文档：多表写入（Insert/Update Graph）——主从级联 + 多表编排

- 状态：Draft / Proposed
- 目标版本：0.1.x（API 先定型，允许分阶段实现；但对用户呈现为“最终形态”）
- 最后更新：2026-01-29

## 背景与现状

当前 `pgorm-derive` 的写入 derive 只覆盖**单表**：

- `InsertModel`（`crates/pgorm-derive/src/insert_model.rs`）禁止字段来自其它表：遇到 `#[orm(table = "...")]` 且不等于结构体 `#[orm(table = "...")]` 会编译报错：`InsertModel does not support fields from joined/other tables`。
- `UpdateModel`（`crates/pgorm-derive/src/update_model.rs`）同样禁止更新其它表字段：`UpdateModel does not support fields from joined/other tables`。

因此目前无法“一个 API 调用”完成：

- 同时插入/更新两个（或多个）表；
- 插入/更新主表时，自动级联插入/更新多个子表（has_one / has_many）；
- 更新子表时按策略（replace / append / diff / upsert）处理子表集合。

业务层虽然能用 `pgorm::transaction!` 组合多条 SQL 达到原子性，但缺少：

- 一致的“写入图（write graph）”声明方式；
- 可复用、可审计的执行顺序与策略；
- 统一的返回/统计（affected rows）与校验（字段互斥、缺失等）。

## 设计原则（最终方案要满足）

1. **SQL-first**：仍以清晰 SQL 与显式策略为中心，不引入复杂 DSL。
2. **易用**：用户声明一次关系/策略，生成 `insert_graph/update_graph` 一键写入。
3. **可实现**：实现上优先复用已生成的 `InsertModel/UpdateModel` 方法（`insert_many` / `upsert_many` / `update_by_id` 等），额外 SQL 只做少量 DELETE/SELECT。
4. **事务友好**：默认以 `&impl GenericClient` 执行；如需跨多语句原子性，天然支持用户把 `conn` 换成事务；并额外提供可选的“自动开事务”便捷入口（需要新 trait 支持）。
5. **可扩展**：同一套声明能力覆盖：
   - 主表 + 多子表
   - 额外写入步骤（例如 audit_log、outbox 事件）
   - 主表依赖 belongs_to（先插入/UPSERT 父表拿 id，再写主表）

## 核心概念：Write Graph（写入图）

写入图由 3 类节点组成，按固定顺序执行：

1. **Pre（前置依赖）**：belongs_to（先写“被引用表”，再把其 id 填到主表外键字段）
2. **Root（主表）**：主表 insert / update
3. **Post（后置依赖）**：has_one / has_many（根据主表 id 写子表），以及额外 after-step

所有节点都允许声明：

- 输入字段（来自当前 struct 的哪个字段）
- 外键/联接方式（fk_field / fk_column / key_column 等）
- 策略（insert / upsert / replace / append / diff）

## 用户侧 API（目标形态）

### A) 插入：主表 + 多子表（最常见）

```rust
use pgorm::{InsertModel, Model};

#[derive(Model)]
#[orm(table = "orders")]
struct Order {
    #[orm(id)]
    id: i64,
    user_id: i64,
    total_cents: i64,
}

#[derive(InsertModel)]
#[orm(table = "order_items")]
struct NewOrderItem {
    // 将由 graph 自动填充
    order_id: i64,
    sku: String,
    qty: i32,
}

#[derive(InsertModel)]
#[orm(table = "orders", returning = "Order")]
// graph_root_key：告诉 pgorm “主表 id 从 returning 结果的哪个 Rust 字段拿”
#[orm(graph_root_key = "id")]
// has_many：插入 items，自动把 order_id 填到每个 child 上
#[orm(has_many(NewOrderItem, field = "items", fk_field = "order_id"))]
struct NewOrder {
    user_id: i64,
    total_cents: i64,

    // 关系字段：不会参与 orders 表列插入；由 graph 负责
    items: Vec<NewOrderItem>,
}

// 一条调用：写 orders + 写 order_items
// 建议：把 conn 换成事务以保证跨多语句原子性（见后文）
let created: Order = NewOrder {
    user_id: 1,
    total_cents: 1000,
    items: vec![
        NewOrderItem { order_id: 0, sku: "A".into(), qty: 1 },
        NewOrderItem { order_id: 0, sku: "B".into(), qty: 2 },
    ],
}.insert_graph_returning(&tx).await?;
```

说明：

- `insert()` / `insert_returning()`：仍只负责 orders 单表（忽略 items 字段）。
- `insert_graph*()`：负责整张写入图（orders + order_items）。

### B) 同时插入两个表（无主从也能编排）

通过 “after/before step” 机制，把额外表写入作为一个独立节点：

```rust
#[derive(InsertModel)]
#[orm(table = "audit_logs")]
struct NewAuditLog {
    action: String,
    entity: String,
}

#[derive(InsertModel)]
#[orm(table = "users", returning = "User")]
#[orm(graph_root_key = "id")]
// after_step：插入 users 之后再插入一条 audit log（不依赖 user.id 也可以）
#[orm(after_insert(NewAuditLog, field = "audit"))]
struct NewUserWithAudit {
    username: String,

    audit: NewAuditLog,
}
```

> 如果 audit 需要引用 `user_id`，把它写成 `has_one` 并指定 `fk_field = "user_id"` 即可。

### C) 主表依赖 belongs_to：先 UPSERT 父表，填外键，再插入主表

常见场景：插入 product 时，如果 category 不存在就创建（按 name 唯一），然后 product.category_id = category.id。

```rust
#[derive(Model)]
#[orm(table = "categories")]
struct Category {
    #[orm(id)]
    id: i64,
    name: String,
}

#[derive(InsertModel)]
#[orm(table = "categories", returning = "Category")]
#[orm(graph_root_key = "id")]
// 最终方案要求 InsertModel 支持自定义 upsert 冲突键（详见后文）
#[orm(conflict_target = "name")]
struct NewCategory {
    name: String,
}

#[derive(InsertModel)]
#[orm(table = "products", returning = "Product")]
#[orm(graph_root_key = "id")]
// belongs_to：如果 category_id 没给，则用 category 插入/UPSERT 后得到 id 写回 category_id
#[orm(belongs_to(NewCategory,
    field = "category",
    set_fk_field = "category_id",
    referenced_id_field = "id", // 从 Category returning 的哪个 Rust 字段取 id（默认 id）
    mode = "upsert_returning",
    required = true
))]
struct NewProduct {
    name: String,
    category_id: Option<i64>,
    category: Option<NewCategory>,
}
```

此处的“易用规则”：

- `category_id` 与 `category` **互斥**：
  - 两者都提供：Validation error
  - 两者都不提供且 `required = true`：Validation error
  - 否则：自动选择已有 id 或插入/UPSERT 父表拿 id

## 更新：主表 + 子表集合的策略化更新

`UpdateModel` 仍是 patch 风格（Option 表示是否更新字段），写入图扩展为：在更新主表后，根据策略处理子表。

### has_many 子表策略（最终支持的 4 种）

- `replace`：删掉旧 children（按 fk），再插入新 children（全量替换）
- `append`：只插入新增 children（不删除旧数据）
- `upsert`：对 children 做 upsert（需要冲突键）
- `diff`：upsert + 删除缺失项（以 key 列表为准，常用于“客户端提交全量列表，DB 同步为一致”）

### 示例：更新 order 并用 diff 同步 items

```rust
#[derive(UpdateModel)]
#[orm(table = "orders", model = "Order", returning = "Order")]
// diff_many：用 items 的 key_field/key_column 去做 delete-missing
#[orm(has_many_update(NewOrderItem,
    field = "items",
    fk_column = "order_id",
    fk_field = "order_id",
    strategy = "diff",
    // key 用于 “保留集合”，最终执行：DELETE ... AND NOT (key_column = ANY($2))
    key_field = "sku",       // Rust 字段：从 NewOrderItem 里取 sku
    key_column = "sku",      // SQL 列名：order_items.sku（通常与字段同名）
    upsert_mode = "upsert_many" // 使用 NewOrderItem::upsert_many（需要 conflict_target = sku）
))]
struct OrderPatch {
    total_cents: Option<i64>,

    // None：不改子表；Some(vec)：按 strategy 处理子表
    items: Option<Vec<NewOrderItem>>,
}

let patch = OrderPatch {
    total_cents: Some(1200),
    items: Some(vec![
        NewOrderItem { order_id: 0, sku: "A".into(), qty: 3 },
        NewOrderItem { order_id: 0, sku: "C".into(), qty: 1 },
    ]),
};

let updated: Order = patch.update_by_id_graph_returning(&tx, order_id).await?;
```

## 生成的 API（最终形态）

### InsertModel（新增）

当 struct 上出现任意 graph 声明（`has_many/has_one/belongs_to/before_insert/after_insert`）时，新增生成：

```rust
pub async fn insert_graph(self, conn: &impl pgorm::GenericClient) -> pgorm::OrmResult<u64>;
pub async fn insert_graph_returning(
    self,
    conn: &impl pgorm::GenericClient,
) -> pgorm::OrmResult<Returning>
where
    Returning: pgorm::FromRow;

pub async fn insert_graph_report(
    self,
    conn: &impl pgorm::GenericClient,
) -> pgorm::OrmResult<pgorm::WriteReport<Returning>>;
```

说明：

- `insert_graph` 返回总 `affected rows`（各步骤求和）。
- `insert_graph_returning` 返回主表 returning 类型。
- `insert_graph_report` 返回更详细信息（每个步骤的 tag、affected、可选的主表 returning）。

可选增强（为了易用）：新增能自动开启事务的入口（需要新增 trait，见“事务与原子性”）：

```rust
pub async fn insert_graph_atomic(
    self,
    client: &mut impl pgorm::TransactionStarter,
) -> pgorm::OrmResult<Returning>;
```

### UpdateModel（新增）

```rust
pub async fn update_by_id_graph<I>(
    self,
    conn: &impl pgorm::GenericClient,
    id: I,
) -> pgorm::OrmResult<u64>
where
    I: ::tokio_postgres::types::ToSql + Sync + Send + 'static;

pub async fn update_by_id_graph_returning<I>(
    self,
    conn: &impl pgorm::GenericClient,
    id: I,
) -> pgorm::OrmResult<Returning>
where
    I: ::tokio_postgres::types::ToSql + Sync + Send + 'static,
    Returning: pgorm::FromRow;
```

关键语义（最终要求）：

- **主表无字段更新但子表有变更**：允许执行子表策略，不返回 “no fields to update”。
- 子表 `None` 表示“不触碰”；`Some(vec)` 才会触发 replace/append/diff/upsert。
- `update_by_id_graph_returning` 在“主表无字段更新”时：不执行 UPDATE，改为在子表策略完成后执行一次 `SELECT ... WHERE id = $1` 返回当前主表行（若不存在则 NotFound）。

## WriteReport（最终建议的返回结构）

为便于观测与调试，建议在 `crates/pgorm/src/lib.rs` 对外暴露：

```rust
pub struct WriteReport<R> {
    /// 所有步骤的 affected rows 求和
    pub affected: u64,
    /// 每个步骤的统计信息（顺序即执行顺序）
    pub steps: ::std::vec::Vec<WriteStepReport>,
    /// 主表 returning（若调用的是 returning 版本或 report 版本）
    pub root: ::std::option::Option<R>,
}

pub struct WriteStepReport {
    /// 例如：`graph:belongs_to:categories` / `graph:root:orders` / `graph:has_many:order_items`
    pub tag: &'static str,
    pub affected: u64,
}
```

实现细节建议：

- 写入图里的每个 SQL 都使用 `execute_tagged/query_one_tagged/...`（现有 `GenericClient` 已支持 tagged 默认实现），让监控/统计能按步骤拆分。
- `WriteReport` 的构造由 derive 生成代码完成：每执行一步就 push 一条 `WriteStepReport`。

## 属性语法（最终统一版）

### InsertModel：图声明

#### 1) root key（用于把主表 id 传递给子表）

```rust
#[orm(graph_root_key = "id")]
```

- 值为“主表 id”对应的 Rust 字段名，用于把主表 id 注入子表外键字段。
- 默认：若未显式提供，默认尝试 `id`（若不存在会由 Rust 编译报错）。

可选补充（覆盖“主键由输入提供”的场景）：

```rust
#[orm(graph_root_key_source = "returning" | "input")]
```

- `returning`（默认）：主表必须配置 `#[orm(returning = "...")]`，并从 returning 结果取 id。
- `input`：从 InsertModel 自身字段取 id（适用于主键不是 DB 自动生成，而是业务侧给定）。
  - 此模式下仍然允许配置 `returning`，但 graph 注入 fk 时优先用输入 id。

注意：

- 只要写入图里存在 `has_one/has_many`（需要 root_id 注入），且 `graph_root_key_source = "returning"`，那么即使调用的是 `insert_graph()`（返回 `u64`），实现也必须内部执行一次 returning 版 INSERT 来拿到 `root_id`（返回值丢弃即可）。

#### 2) has_one / has_many

```rust
#[orm(has_one(ChildInsert,
    field = "profile",
    fk_field = "user_id",
    fk_wrap = "value" // value|some，默认 value
))]

#[orm(has_many(ChildInsert,
    field = "items",
    fk_field = "order_id",
    fk_wrap = "value"
))]
```

字段约束：

- `field` 必须存在于当前 struct。
- `has_one` 支持字段类型：`ChildInsert` / `Option<ChildInsert>`
- `has_many` 支持字段类型：`Vec<ChildInsert>` / `Option<Vec<ChildInsert>>`

fk 注入规则（最终要求）：

- graph 会在写子表前**覆盖**每个 child 的 `fk_field`：
  - `fk_wrap = value`：`child.fk_field = root_id.clone()`
  - `fk_wrap = some`：`child.fk_field = Some(root_id.clone())`
- 为避免用户在构造 child 时被迫提供“占位值”，推荐把 child 的外键字段写成 `Option<Id>` 并使用 `fk_wrap = some`。

#### 3) belongs_to（前置依赖）

```rust
#[orm(belongs_to(ParentInsert,
    field = "category",
    set_fk_field = "category_id",
    referenced_id_field = "id",
    mode = "upsert_returning", // insert_returning | upsert_returning
    required = true
))]
```

字段约束：

- `field` 类型：`ParentInsert` / `Option<ParentInsert>`
- `set_fk_field` 类型：通常为 `Option<Id>`（也允许非 Option，表示必须靠 belongs_to 生成）
- `mode = upsert_returning` 依赖 InsertModel 提供对应 returning upsert（见下一节“自定义冲突键”）。

#### 4) before/after step（纯编排节点，用于“同时写两个表”等场景）

```rust
#[orm(before_insert(OtherInsert, field = "x", mode = "insert"))]
#[orm(after_insert(OtherInsert, field = "y", mode = "insert"))]
```

- `OtherInsert` 必须是 `#[derive(InsertModel)]` 类型。
- `field` 支持：`OtherInsert` / `Option<OtherInsert>` / `Vec<OtherInsert>` / `Option<Vec<OtherInsert>>`
- `mode`：
  - `insert`：对单个值调用 `.insert(conn)`；对 Vec 调用 `OtherInsert::insert_many(conn, rows)`
  - `upsert`：同理，调用 `.upsert` / `upsert_many`（要求 `OtherInsert` 支持 upsert）

执行顺序：

- `before_insert` 在 belongs_to 之后、root 之前执行
- `after_insert` 在 has_one/has_many 之后执行

### UpdateModel：子表策略声明

```rust
#[orm(has_many_update(ChildInsert,
    field = "items",
    fk_column = "order_id",
    fk_field = "order_id",
    fk_wrap = "value",
    strategy = "replace" | "append" | "upsert" | "diff",
    // diff 必填
    key_field = "id",
    key_column = "id",
    // upsert/diff 使用哪个批量方法
    upsert_mode = "upsert_many" // upsert_many（后续可扩展）
))]
```

字段约束（推荐）：

- `field` 类型：`Option<Vec<ChildInsert>>`

has_one_update（最终版）：

```rust
#[orm(has_one_update(ChildInsert,
    field = "profile",
    fk_column = "user_id",
    fk_field = "user_id",
    fk_wrap = "value",
    strategy = "replace" | "upsert"
))]
```

- `field` 推荐类型：`Option<Option<ChildInsert>>`
  - `None`：不触碰（不做任何子表操作）
  - `Some(None)`：删除该 fk 下的子行（`DELETE FROM child WHERE fk_column = $1`）
  - `Some(Some(v))`：
    - `replace`：先删除旧行，再插入 `v`
    - `upsert`：执行 upsert（要求 `ChildInsert` 支持 upsert）

## 自定义冲突键（最终方案必须补齐）

为支持：

- belongs_to 的“按唯一键 upsert 并返回 id”
- 子表 diff 的“按自然键 upsert（key_column）”

需要扩展 `InsertModel` 的 upsert 能力：从“只能 ON CONFLICT(id)”升级为“可指定冲突目标”。

### InsertModel 新增 struct-level 属性

```rust
#[orm(conflict_target = "name")]          // 单列
#[orm(conflict_target = "a,b")]           // 复合（按逗号分隔）
// 可选：显式指定 update 列集合（默认：除 skip_insert/default 之外的所有列）
#[orm(conflict_update = "col1,col2,...")] // 可选
```

并据此生成/改造：

- `upsert(self, conn)` / `upsert_many(conn, rows)`
- `upsert_returning(...)` / `upsert_many_returning(...)`（若配置了 returning）

生成条件（最终要求）：

- 只要存在 `#[orm(conflict_target = "...")]`，就生成 upsert 系列方法（不再强制要求 `#[orm(id)]` 字段）。
- 若同时存在 `#[orm(id)]` 与 `conflict_target`：upsert 默认以 `conflict_target` 为准；`id` 仍可用于其它用途（例如业务自定义）。

实现上只需在 `insert_model.rs` 里把冲突列从 “id 字段” 替换为 `conflict_target`（存在则优先），并复用现有 `EXCLUDED.col` 更新赋值逻辑即可。

## 事务与原子性（最终体验）

### 1) 默认（最简单、也最符合 pgorm 现状）

`insert_graph* / update_by_id_graph*` 都接受 `&impl GenericClient`：

- 你传 `&Transaction<'_>`：天然原子（推荐）
- 你传 `&Client`：多语句时不自动包事务（仍然按顺序执行）

推荐用法：

```rust
pgorm::transaction!(&mut client, tx, {
    let out = new_order.insert_graph_returning(&tx).await?;
    Ok(out)
})?;
```

### 2) 可选增强：提供“自动开事务”的 trait（为了更易用）

新增 trait（放 `crates/pgorm/src/transaction.rs` 或 `client.rs`）：

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

然后为 `tokio_postgres::Client`、`deadpool_postgres::Client` 实现它。

在 derive 里新增 `*_atomic(&mut impl TransactionStarter)` 方法：内部自动 begin/commit/rollback。

## 错误处理与校验（最终要求）

1. **配置校验（编译期尽量早）**
   - `field` 必须存在
   - `strategy` 取值必须合法
2. **输入校验（运行时 Validation error）**
   - belongs_to 的互斥/必填规则
   - diff/upsert 缺少 key 字段/冲突键时给出明确错误
3. **失败语义**
   - 任一步失败：立即返回 Err
   - 是否回滚由事务决定；使用 `*_atomic` 时自动回滚

## 宏实现要点（确保可实现）

### InsertModel（`crates/pgorm-derive/src/insert_model.rs`）

1. 解析 struct attrs：
   - 保留现有 `table/returning`
   - 新增解析：`graph_root_key`、`has_one/has_many/belongs_to/before_insert/after_insert` 等函数式属性（可重复）
2. 扫描字段时：
   - 若字段命中 graph 的 `field = "..."`
     - 不加入 `insert_columns/bind_field_idents`
     - 记录其 ident，用于 `insert_graph` 的 destructure
3. 生成 `insert_graph*` 方法：
   - pre：处理 belongs_to，必要时写回主表 fk 字段（局部变量）
   - root：调用现有 `insert_returning` 或 `insert`（根据是否需要 root_key/returning）
   - post：对 has_one/has_many 进行 fk 注入后调用 child 的 `insert/insert_many/upsert_many` 等
   - step：执行额外 after/before insert（同样复用其 InsertModel 方法）
4. 扩展 upsert 冲突键：解析 `conflict_target` 并替换现有 “id 冲突列” 逻辑

### UpdateModel（`crates/pgorm-derive/src/update_model.rs`）

1. 解析 struct attrs：
   - 保留现有 `table/id_column/model/returning`
   - 新增解析：`has_one_update/has_many_update`
2. 生成 `update_by_id_graph*`：
   - 先执行主表 update（复用现有生成的 update 逻辑，但要允许“无字段更新但仍有子表策略”）
   - 再按 strategy 执行子表：
     - replace：`DELETE FROM child WHERE fk_column = $1` + `ChildInsert::insert_many`
     - append：`ChildInsert::insert_many`
     - upsert：`ChildInsert::upsert_many`
     - diff：`ChildInsert::upsert_many` + `DELETE ... AND NOT (key_column = ANY($2))`
