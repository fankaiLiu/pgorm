# 08 Plan: Pagination Capability Expansion (Beyond Keyset1/2)

状态: Draft  
优先级: P1  
目标版本: v0.5.x

## 1. 背景与问题

当前只提供 `Keyset1` / `Keyset2`（`crates/pgorm/src/builder.rs:495`, `:589`），当排序字段超过 2 个或需要复杂 tie-breaker 时能力不足。

## 2. 目标

- 支持 3 列及以上 keyset。
- 支持混合方向排序（如 `created_at DESC, score DESC, id ASC`）。
- 保持现有 `Keyset1/2` API 不破坏。

## 3. 非目标

- 不替代 page-based 分页。
- 不在首版解决跨 DB 方言差异。

## 4. 设计方案

### 4.1 新 Builder

新增 `Keyset`（动态列版本）：

- `push_key("created_at", SortDir::Desc)`
- `push_key("id", SortDir::Asc)`
- `after_values(vec![...])` / `before_values(vec![...])`

### 4.2 SQL 生成策略

采用词典序展开（兼容混合方向）：

- `(k1 op v1)`
- `OR (k1 = v1 AND k2 op v2)`
- `OR (k1 = v1 AND k2 = v2 AND k3 op v3)`

避免依赖 tuple compare 的同向限制。

### 4.3 与现有 API 的关系

- `Keyset1/2` 保留；内部可复用新 `Keyset`。
- 文档仍推荐 `Keyset1/2` 处理简单场景。

## 5. 实施拆分

### M1: 动态 Keyset 核心

- builder + SQL + 参数绑定。

### M2: 兼容层

- `Keyset1/2 -> Keyset` 复用，保证行为不变。

### M3: 文档与示例

- 新增 3 列分页示例与“上一页/下一页”示例。

## 6. 风险与兼容

- 风险: 条件展开后 SQL 更长。
- 缓解: 提供建议上限（例如最多 4 列），并在 docs 给索引建议。

## 7. 验收标准

- 3 列以上 keyset 可用。
- 混合方向排序可正确分页，无重复/漏项。
- `Keyset1/2` 现有测试全部通过。
