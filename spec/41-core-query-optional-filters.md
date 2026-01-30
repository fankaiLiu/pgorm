# Query Builder「可选过滤」语法糖（`*_opt` / `apply_if_*`）设计与计划

状态：Draft  
相关代码：`crates/pgorm-derive/src/model/query.rs` / `crates/pgorm/src/where_expr.rs` / `crates/pgorm/src/condition.rs`  
最后更新：2026-01-30

## 背景

在 Web API / 管理后台 / 审计查询这类场景里，查询条件往往来自“可选参数”（`Option<T>`）。目前 `Model::query()` 的写法需要大量的：

- `if let Some(v) = ... { q = q.eq(..., v)?; }`
- `if let Ok(v) = s.parse() { ... }`

这会让业务代码看起来“全是样板”，而核心逻辑（筛选字段本身）被淹没。

示例（当前常见写法）：

```rust
let mut q = AuditLog::query();

if let Some(uid) = user_id {
    q = q.eq(AuditLogQuery::COL_USER_ID, uid)?;
}
if let Some(sd) = start_date {
    q = q.gte(AuditLogQuery::COL_CREATED_AT, sd)?;
}
// ...
```

## 目标 / 非目标

### 目标

- 为 `Model::query()` 提供“条件式链式调用”的最短路径，减少 `if let Some` 样板。
- 保持现有 `eq/ne/gt/gte/lt/lte/...` 的语义与错误模型不变（继续返回 `OrmResult<Self>`，可 `?`）。
- 增量、可选：无需引入宏 DSL，即可让调用端变得更干净。
- 处理“可选参数 + 预处理”的常见模式：如 `Option<String>` 先 `parse()` 再作为过滤条件（IP、UUID、日期等）。

### 非目标

- 不引入新的查询 DSL；不改变 “SQL-first + 显式 bind” 的定位。
- 不自动在 Query Builder 内做入参校验/转换（例如自动把 `String` 解析成 `IpAddr`）；这更适合在 input layer / validate 层统一处理。
- 不尝试覆盖所有业务侧的组合逻辑（复杂 OR/AND、动态列集合等）；这些应继续用 `and()/or()/raw()` 或业务自行封装。

## 方案

本 RFC 推荐在 **derive 生成的 Query Builder** 上新增两类能力：

1) **通用的“条件式应用”方法**：`apply_if_*`  
2) **高频操作的 `Option<T>` 版本**：`*_opt`（作为 `apply_if_some` 的薄封装）

### 1) 通用：`apply_if_*`

新增 3 个方法（名字可讨论，但建议统一用 `apply_*` 前缀）：

```rust
pub fn apply_if(
    self,
    cond: bool,
    f: impl FnOnce(Self) -> pgorm::OrmResult<Self>,
) -> pgorm::OrmResult<Self>;

pub fn apply_if_some<T>(
    self,
    v: Option<T>,
    f: impl FnOnce(Self, T) -> pgorm::OrmResult<Self>,
) -> pgorm::OrmResult<Self>;

pub fn apply_if_ok<T, E>(
    self,
    v: Result<T, E>,
    f: impl FnOnce(Self, T) -> pgorm::OrmResult<Self>,
) -> pgorm::OrmResult<Self>;
```

语义：

- `cond == false` / `Option::None` / `Result::Err(_)`：直接返回 `Ok(self)`，无副作用。
- “只在需要时才执行 closure”，避免无意义的 clone/parse。

优点：

- 覆盖面极广：不需要为每个过滤操作都加 `*_opt` 方法，也能把可选逻辑变成一行。
- 能自然处理“parse 成功才加条件”的场景。

代价：

- 调用端需要 closure，相比 `eq_opt` 仍然稍啰嗦。

### 2) 高频：`*_opt`（`Option<T>` 语法糖）

在 Query Builder 上为常用过滤操作提供 `Option<T>` 版本（建议 MVP 先做 `eq_opt/gte_opt/lte_opt`，其余按需求补齐）：

```rust
pub fn eq_opt<T>(
    self,
    column: impl pgorm::IntoIdent,
    value: Option<T>,
) -> pgorm::OrmResult<Self>
where
    T: tokio_postgres::types::ToSql + Send + Sync + 'static;
```

语义：`Some(v)` 时等价于 `self.eq(column, v)`；`None` 时 no-op。

建议覆盖的列表（按优先级）：

- P0：`eq_opt` / `gte_opt` / `lte_opt`
- P1：`ne_opt` / `gt_opt` / `lt_opt`
- P2：`like_opt` / `ilike_opt` / `in_list_opt` / `not_in_opt`
- P3：`between_opt`：形态建议为 `Option<(T, T)>`（调用端可用 `start.zip(end)`）

> 实现建议：`*_opt` 直接调用 `apply_if_some(value, |q, v| q.eq(column, v))`，避免重复逻辑。

## 使用示例（目标写法）

### A) 只用 `*_opt`（最贴近直觉）

```rust
let q = AuditLog::query()
    .eq_opt(AuditLogQuery::COL_USER_ID, user_id)?
    .eq_opt(
        AuditLogQuery::COL_OPERATION_TYPE,
        operation_type.map(|op| op.to_string()),
    )?
    .eq_opt(
        AuditLogQuery::COL_RESOURCE_TYPE,
        resource_type.map(|rt| rt.to_string()),
    )?
    .gte_opt(AuditLogQuery::COL_CREATED_AT, start_date)?
    .lte_opt(AuditLogQuery::COL_CREATED_AT, end_date)?
    .eq_opt(
        AuditLogQuery::COL_IP_ADDRESS,
        ip_address.and_then(|ip| ip.parse::<std::net::IpAddr>().ok()),
    )?
    .eq_opt(AuditLogQuery::COL_STATUS_CODE, status_code)?;
```

### B) 只用 `apply_if_some/apply_if_ok`（最通用）

```rust
let q = AuditLog::query()
    .apply_if_some(user_id, |q, uid| q.eq(AuditLogQuery::COL_USER_ID, uid))?
    .apply_if_some(start_date, |q, sd| q.gte(AuditLogQuery::COL_CREATED_AT, sd))?
    .apply_if_ok(ip.parse::<std::net::IpAddr>(), |q, addr| q.eq(AuditLogQuery::COL_IP_ADDRESS, addr))?;
```

## 实现要点

### 代码生成位置

Query Builder 来自 `pgorm-derive`（`crates/pgorm-derive/src/model/query.rs`），因此推荐：

- 在 `gen_filtering_methods()` 中新增 `apply_if_*`，并追加 `*_opt` 方法；
- 或拆分一个新的 `gen_utility_methods()`，保持 filtering methods 更纯粹。

### 命名冲突（列常量）

Query Builder 还会生成“字段名列常量”（`field` 和 `COL_FIELD`）。为了避免字段名与新增方法名冲突，需要把新增方法名加入 reserved 列表（例如：`eq_opt/gte_opt/lte_opt/apply_if/apply_if_some/apply_if_ok`）。

## 兼容性与迁移

- 纯新增方法：对现有用户无破坏性（不修改已有方法签名/行为）。
- 仅需注意极小概率的“字段名冲突”：若模型字段名刚好叫 `apply_if_some` 等，原本会生成同名 const；加入 reserved 后会切换为只生成 `COL_*` 常量（行为与现有 reserved 机制一致）。

## 里程碑 / TODO

### M1（MVP：解决 80% 的丑代码）

- [ ] `apply_if/apply_if_some/apply_if_ok`（derive 生成）
- [ ] `eq_opt/gte_opt/lte_opt`（derive 生成）
- [ ] reserved 列表补齐，避免常量冲突
- [ ] 文档补齐：在 guide/示例里给出“可选过滤”推荐写法

### M2（补齐常用操作）

- [ ] `ne_opt/gt_opt/lt_opt`
- [ ] `like_opt/ilike_opt`
- [ ] `in_list_opt/not_in_opt`（并明确 empty list 的语义）
- [ ] `between_opt`（`Option<(T, T)>`）

### M3（体验打磨）

- [ ] 评估是否需要 `apply_if_some_ref`（针对不想 move 的场景；但需与 ToSql `'static` 约束一起审视）
- [ ] 示例：审计日志查询（带分页/排序/可选过滤的完整 end-to-end）

## Open Questions

1) `in_list_opt(Some(vec![]))` 的语义：跳过？还是生成恒 false？还是返回错误？（需与 `Condition::in_list` 当前行为对齐）  
2) `between_opt` 应该只支持 `Option<(T, T)>`，还是提供 `between_opt(start, end)` 两个 `Option<T>` 的组合？  
3) 是否要把 `apply_if_ok` 的 `Err(_)` 丢弃视为“静默跳过”？还是提供一个可选的“把 parse 错误返回给上层”的版本（例如 `try_apply_if_some`）？  

