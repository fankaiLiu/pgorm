# 07 Plan: Model Codegen Relation Inference + Write Model Generation

状态: Draft  
优先级: P1  
目标版本: v0.5.x

## 1. 背景与问题

当前 model codegen 只生成字段和 `#[orm(id)]`（`crates/pgorm-cli/src/model_codegen.rs:288`），没有：

- relation attrs（`has_many`/`belongs_to`/`has_one`/`many_to_many`）
- `InsertModel` / `UpdateModel` 写模型

另外，schema introspection 数据结构目前只有表和列（`crates/pgorm-check/src/schema_introspect.rs` 的 `TableInfo`）。没有外键信息，无法可靠推断关系。

## 2. 目标

- 代码生成可直接覆盖“读模型 + 写模型 + 常见关系声明”。
- 通过可配置策略减少错误推断风险。

## 3. 非目标

- 不追求 100% 自动准确（复杂业务关系仍允许手工覆盖）。
- 不自动生成业务级 write graph。

## 4. 设计方案

### 4.1 扩展 schema 元数据

在 `pgorm-check` 增加：

- `ForeignKeyInfo`（源表列 -> 目标表列）
- `UniqueConstraintInfo` / `PrimaryKeyInfo`
- （可选）索引信息

### 4.2 relation 推断规则

- 非空 FK + 目标唯一约束: 默认 `belongs_to`。
- 反向自动生成 `has_many`。
- 唯一 FK 可提升为 `has_one`。
- 典型 join table（双 FK + 复合唯一）可推断 `many_to_many`。

### 4.3 写模型生成

新增开关（`[models.codegen]`）：

- `emit_insert_models = true`
- `emit_update_models = true`
- `emit_relations = true`

输出：

- `User`（Model）
- `NewUser`（InsertModel）
- `UserPatch`（UpdateModel）

### 4.4 覆盖与逃生口

- `models.overrides.relations` 支持禁用/改名/改方向。
- 对不确定关系默认不生成，给 warning。

## 5. 实施拆分

### M1: schema 元数据增强

- 先补 FK + 唯一约束。

### M2: relation 推断 + 只读模型输出

- 先生成 `belongs_to` 与 `has_many`。

### M3: 写模型生成

- 生成 InsertModel / UpdateModel skeleton。

### M4: many_to_many 与高级覆盖

- 加入 join table 自动识别。

## 6. 风险与兼容

- 风险: 自动关系推断误报。
- 缓解: 默认 conservative；所有自动生成段落打 `@generated` + warning 列表。

## 7. 验收标准

- 在含 FK 的示例库中生成可编译模型。
- 至少 80% 常见单表 CRUD 场景无需手工补写模型。
- 生成结果支持 `--check` 稳定通过。
