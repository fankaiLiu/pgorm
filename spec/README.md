# pgorm Spec / Plans

本目录用于存放与 `pgorm` 相关的设计与改进计划（偏“工程规划/落地步骤”，不是 API 文档本体）。

## 当前计划（按推荐执行顺序）

1. `spec/00-plan-maintainability-split-modules.md` — 可维护性：先拆大文件、理清模块边界（低风险/高收益）
2. `spec/01-plan-api-design.md` — API 设计：收敛入口、稳定对外承诺、制定弃用策略
3. `spec/02-plan-performance-cache-lock.md` — 性能：先基准，再优化缓存/锁/分配
4. `spec/03-plan-docs-and-examples.md` — 文档与示例：让用户 30 秒上手、5 分钟写对

## 统一验收命令

每一步改动尽量保证下面三条都能过：

```bash
cargo fmt --all
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

