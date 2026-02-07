# 04 Plan: Composite Primary Key First-Class Support

状态: Draft  
优先级: P0  
目标版本: v0.4.x

## 1. 背景与问题

当前主键模型是“单列主键优先”：

- `TableMeta::primary_key() -> Option<&'static str>` 仅表达单列主键（`crates/pgorm/src/check/registry.rs:20`）。
- `#[derive(Model)]` 生成的 `TableMeta` 实现也仅回填单列主键（`crates/pgorm-derive/src/model.rs:256`）。

这导致复合主键表在以下方面受限：

- 运行时 schema check 无法表达完整 PK 约束。
- `Model` 派生生成的按 ID 查询/删除接口仅覆盖单列。
- 关系加载、写入图、UpdateModel 的 ID 语义缺少统一复合键表示。

## 2. 目标

- 在 `check`、derive 宏、模型 API 三层提供复合主键一等支持。
- 对单列主键保持向后兼容。
- 让复合主键模型拥有可用的查询/删除/关系加载基础能力。

## 3. 非目标

- 本阶段不做自动主键策略迁移工具。
- 本阶段不强制重写所有旧 API（保留单列便捷方法）。

## 4. 设计方案

### 4.1 元数据层（check）

- 新增 `TableMeta::primary_keys() -> &'static [&'static str]`。
- 保留 `primary_key()`：
  - 默认实现从 `primary_keys()` 取第一个元素（兼容旧调用）。
  - 标记为 deprecated（仅文档层先提示，不立刻移除）。
- `SchemaRegistry` 与 `TableSchema` 增加复合键读取 helper（例如 `primary_keys()`）。

### 4.2 derive 层（Model）

- 允许多个字段标注 `#[orm(id)]`。
- 生成：
  - `const IDS: &'static [&'static str]`（新增）
  - `TableMeta::primary_keys()`（新增）
  - 单键模型继续生成 `select_one(id)` / `delete_by_id(id)`。
  - 复合键模型生成 `select_by_pk(...)` / `delete_by_pk(...)`（参数与 id 字段顺序一致）。

### 4.3 模型键 trait

- 保留 `ModelPk` 兼容单键场景。
- 新增 `ModelPkTuple`（或 `ModelPrimaryKey`）用于复合键返回值（tuple owned）。
- 关系加载内部统一改用“可哈希复合键”类型参数，避免只支持单列 ID。

### 4.4 UpdateModel / 写入路径

- `UpdateModel` 新增 `id_columns = "a,b,..."`（可选，复合键时必需）。
- 生成 `update_by_pk(...)` 变体。
- 单键 `update_by_id(...)` 行为不变。

## 5. 实施拆分

### M1: 元数据与兼容层

- 修改 `TableMeta` trait 与 `SchemaRegistry`。
- 覆盖 `check` 相关单测。

### M2: Model 宏与 API

- `#[orm(id)]` 多字段解析与冲突校验。
- 生成复合键方法。

### M3: 关系与写入联动

- Eager loading / UpdateModel 对复合键的最小支持闭环。

### M4: 文档与示例

- 新增复合主键示例（订单行 `order_id + line_no`）。

## 6. 风险与兼容

- 风险: trait 变更影响第三方手写 `TableMeta` 实现。
- 缓解: 提供默认实现 + 迁移指南 + 至少一个小版本过渡。

## 7. 验收标准

- 复合键模型可通过 derive 编译并注册 schema。
- 复合键模型具备 `select_by_pk` / `delete_by_pk`。
- 单键模型 API 与行为无回归。
- `cargo test --workspace` 全绿。

## 8. 回滚方案

- 保留 feature gate（如 `composite_pk_experimental`）直到 M3 完成。
- 若出现兼容事故，可临时关闭复合键生成路径，仅保留 metadata 支持。
