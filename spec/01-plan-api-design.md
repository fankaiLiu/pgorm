# 计划：API 设计（入口收敛 / 稳定承诺 / feature 策略）

目标：让 `pgorm` 的对外 API 更一致、更可预期；让用户知道“应该从哪里开始用”，同时给未来演进留出空间（弃用/版本策略）。

## 设计原则

- **模型定义优先**：通过模型定义驱动 CRUD 代码生成；SQL builder 作为灵活补充。
- **两个层级**：
  - 推荐：`PgClient`（带监控 + SQL 检查 + statement cache + policy）
  - 低层：`GenericClient`/`Sql`（可插拔、最小抽象）
- **入口要少**：顶层 `pgorm::` 只保留少量“显而易见”的入口；其余走模块或 `prelude`。

## 交付物

- 清晰的“官方推荐路径”：
  - README 的 Quick Start 以 `PgClient` 为主
  - `prelude` 作为日常使用的统一入口
- API 稳定策略：
  - 弃用周期（例如：至少一个 minor 版本的 `#[deprecated]` 过渡）
  - MSRV 与 semver 承诺写清楚
- feature 策略：
  - 默认 feature 是否全开（当前是）vs 最小默认
  - 给出推荐组合（例如：`default` / `minimal` / `server`）

## 工作步骤

### Phase 0：盘点与“冻结范围”

- [ ] 列出当前所有对外导出项（`crates/pgorm/src/lib.rs` 的 `pub use` + `pub mod`）
- [ ] 给出“核心稳定入口名单”（建议优先级）：
  - `PgClient`、`PgClientConfig`、`sql()`/`query()`、`Sql`、`Condition/WhereExpr/OrderBy/Pagination`、derive 宏
- [ ] 明确“可调整入口名单”（可以改路径/改名字但要弃用过渡）：例如大量次要 `pub use`

### Phase 1：收敛顶层导出

- [ ] 把“常用集合”放进 `pgorm::prelude::*`（并在 README 推荐）
- [ ] 顶层只保留核心类型（其它通过模块路径访问）
- [ ] 对外路径变更：使用 `#[deprecated(note = \"use ...\")]` 过渡，不直接 break

### Phase 2：错误模型（可分支处理 vs 开发期错误）

- [ ] 给 `OrmError` 分类：哪些用户应该 match（NotFound/StaleRecord/Timeout），哪些是配置/编程错误
- [ ] 文档化错误语义（README 或 docs/cookbook）

### Phase 3：feature 与编译成本

- [ ] 为每个 feature 在 README 列“引入的依赖 + 典型用途 + 是否推荐默认”
- [ ] 如果要改默认 feature：先引入新的 meta-feature（如 `minimal`），再考虑调整 `default`

### Phase 4：发布与弃用策略
- [ ] 暂时警告使用者 随时可能改变

## 验收标准（Definition of Done）

- [ ] README 给出唯一的推荐 Quick Start（不再让用户在多种入口间迷路）
- [ ] `prelude` 足够覆盖 80% 常见使用（但不会把所有东西都塞进去）
- [ ] 任何对外路径变更都有弃用过渡（编译期提示清晰）
