# Eager Loading 改造 TODO（执行清单）

- 目标：把 `docs/design/eager-loading.md` 的 Proposal 落地到可用 API + 测试 + 文档
- 最后更新：2026-01-29

## 方案（已拍板）

- 同时提供 **Map** 与 **Attach** 两种用法
- `belongs_to` 默认返回 `Option<Parent>`；同时提供 `*_strict` 变体（返回 `Parent`，缺失时报错）
- `*_with` 回调签名：`FnOnce(&mut pgorm::Sql)`（用于追加全局过滤/排序）

## Checklist

### 1) Runtime：新增 eager 模块

- [x] 新增 `crates/pgorm/src/eager.rs`：类型 `HasManyMap` / `BelongsToMap` / `Loaded`
- [x] 新增通用 helper：`load_has_many_map` / `load_has_many_map_with`
- [x] 新增通用 helper：`load_belongs_to_map` / `load_belongs_to_map_with`
- [x] 在 `crates/pgorm/src/lib.rs` 暴露 `pub mod eager` 并导出常用类型
- [x] 在 `crates/pgorm/src/prelude.rs` 增加 eager 相关 re-export

### 2) Derive：为 `#[derive(Model)]` 生成 eager-loading API

- [x] `has_many`：生成 `load_{rel}_map` / `load_{rel}_map_with`
- [x] `has_many`：生成 `load_{rel}` / `load_{rel}_with`（返回 `Vec<Loaded<Parent, Vec<Child>>>`，保持 base 顺序）
- [x] `belongs_to`：生成 `{rel}_id()` 访问器（避免跨模块读私有 fk 字段）
- [x] `belongs_to`：生成 `load_{rel}_map` / `load_{rel}_map_with`（返回 `HashMap<fk_id, Parent>`）
- [x] `belongs_to`：生成 `load_{rel}` / `load_{rel}_with`（返回 `Vec<Loaded<Child, Option<Parent>>>`）
- [x] `belongs_to`：生成 `load_{rel}_strict` / `load_{rel}_strict_with`（返回 `Vec<Loaded<Child, Parent>>`）

### 3) Tests

- [x] 新增编译期测试：`crates/pgorm/tests/eager_loading.rs`（覆盖 has_many / belongs_to / _with / _strict 的签名可编译）
- [x] 新增行为测试：空输入应快速返回且不触发 query（panic client）

### 4) Docs

- [x] 在 `README.md` 增加 eager-loading 用法示例（Map + Attach + belongs_to strict 提示）
- [x] （可选）在 `docs/design/eager-loading.md` 补充最终 API 说明/链接到本 TODO

### 5) Verify

- [x] `cargo test -p pgorm`
