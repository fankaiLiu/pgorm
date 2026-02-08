# pgorm Spec / Plans

本目录用于存放与 `pgorm` 相关的设计与改进计划（偏“工程规划/落地步骤”，不是 API 文档本体）。

## 当前计划（按推荐执行顺序）

1. `spec/04-plan-composite-primary-key.md` — 复合主键一等支持（P0）
2. `spec/05-plan-transaction-level-config.md` — 事务级别配置（P0）
3. `spec/06-plan-migration-toolchain.md` — 迁移工具链（P0）
4. `spec/07-plan-model-codegen-relations-write-models.md` — model codegen 的关系推断与写模型生成（P1）
5. `spec/08-plan-pagination-expansion.md` — 分页能力扩展（P1）
6. `spec/09-plan-productize-known-limitations.md` — 已知限制产品化（P1-P2）
7. `spec/10-plan-pg-listen-notify.md` — PostgreSQL LISTEN/NOTIFY 支持（P1）
8. ~~`spec/00-plan-maintainability-split-modules.md`~~ — 可维护性（尚未创建）
9. ~~`spec/01-plan-api-design.md`~~ — API 设计（尚未创建）
10. ~~`spec/02-plan-performance-cache-lock.md`~~ — 性能（尚未创建）
11. `spec/03-plan-docs-and-examples.md` — 文档与示例：让用户 30 秒上手、5 分钟写对
12. `spec/CODE_REVIEW.md` — 代码审查与改进追踪

## 统一验收命令

每一步改动尽量保证下面三条都能过：

```bash
cargo fmt --all
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```
