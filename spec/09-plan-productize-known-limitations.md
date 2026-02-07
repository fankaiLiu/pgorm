# 09 Plan: Productize Known Limitations

状态: Draft  
优先级: P1-P2  
目标版本: v0.5.x

## 1. 背景

当前存在几类“已知限制”：

- 批量更新不支持版本检查（`crates/pgorm-derive/src/update_model/gen_base.rs:133`）。
- `input_as` 不支持 `Option<Option<T>>`（`crates/pgorm-derive/src/update_model.rs:342`）。
- `QueryParams` 单字段不支持多个 filter op（`crates/pgorm-derive/src/query_params.rs:139`）。

这些限制都不是理论不可做，属于可产品化能力。

## 2. 子计划 A: 批量更新 + 乐观锁

### 目标

提供“批量 patch + version check”安全路径。

### 方案

- 新增 API（示例）：
  - `update_by_ids_with_versions(ids, versions)`
  - `update_by_ids_with_versions_returning(...)`
- SQL 采用 `UNNEST(ids, versions)` + join 进行逐行版本匹配。
- 返回结构包含 `affected` 与 `stale_ids`（可选）。

### 验收

- 并发冲突时只标记冲突行，不误更新。

## 3. 子计划 B: input_as + 三态 Option

### 目标

允许 `Option<Option<T>>` 字段配合 `input_as` 使用。

### 方案

- 输入模型使用 `Option<Option<TWire>>`。
- 转换只在 `Some(Some(v))` 路径执行 parse/validate。
- `Some(None)` 保持“显式设为 NULL”语义；`None` 保持“跳过更新”。

### 验收

- tri-state 语义不回归。
- validation error 指向字段路径准确。

## 4. 子计划 C: QueryParams 多过滤操作

### 目标

允许单字段叠加多个 filter op（例如同一字段同时支持 `eq` 和 `in`）。

### 方案

- 调整宏解析器：`op_kind: Option<_>` -> `Vec<_>`。
- 生成代码按声明顺序应用过滤器。
- 冲突规则显式化（如 `paginate` 仍限制唯一）。

### 验收

- 单字段多 op 编译通过并按预期生成 SQL。

## 5. 统一实施顺序

1. 子计划 A（P1）
2. 子计划 B（P1）
3. 子计划 C（P2）

## 6. 统一风险

- 宏复杂度上升，错误信息可能退化。
- SQL 生成路径变复杂，回归风险增加。

## 7. 统一保障

- 每个子计划都配“编译测试 + 行为测试 + 文档示例”。
- 新增回归测试矩阵覆盖 old/new API 共存场景。
