# pgorm examples 补齐计划（crates/pgorm）

状态：Draft  
相关代码：`crates/pgorm/examples/*`、`crates/pgorm/src/*`、`crates/pgorm-derive/*`  
最后更新：2026-01-30

## 背景

目前 `crates/pgorm/examples/` 已经覆盖了几条关键路径：

- `changeset`：`#[orm(input)]` + 校验 + `InsertModel/UpdateModel`
- `eager_loading`：`Model` 关系 + preload
- `update_model`：patch 语义（`Option<T>` / `Option<Option<T>>`）
- `pg_client`：运行时 SQL 检查 + 监控（推荐客户端）
- `migrate`：`refinery` 迁移
- `sql_builder`：动态 SQL + 条件树 + 排序 + 分页（可选 DB 运行）
- `insert_many`：UNNEST bulk insert（`insert_many_returning`）
- `upsert`：ON CONFLICT（`upsert_returning` / `upsert_many_returning`）
- `write_graph`：多表写入图（`insert_graph_report` + 事务）
- `monitoring`：`InstrumentedClient` + `QueryMonitor/QueryHook`
- `jsonb`：`Json<T>` + `serde_json::Value`（jsonb）
- `fetch_semantics`：`fetch_one` vs `fetch_one_strict` vs `fetch_opt`

仍然建议把新增例子作为“覆盖面清单”维护在这里：当未来增加新特性/新宏参数时，能快速判断是否需要同步新增或更新 examples。

- 动态 SQL/条件/分页/排序的推荐写法（`Sql`/`Condition`/`WhereExpr`/`OrderBy`/`Pagination`/`Ident`）
- 批量写入（`InsertModel::insert_many` / `*_returning`，UNNEST bulk insert）
- Upsert（`conflict_target/conflict_constraint/conflict_update` + `upsert/upsert_many`）
- 多表写入图（`insert_graph*` / `update_by_id_graph*` / `*_report`）
- 监控/Hook 的自定义玩法（`QueryMonitor/QueryHook/InstrumentedClient`）

## 目标 / 非目标

### 目标

1) **覆盖主路径 + 易踩坑点**：从“最小可用”到“高级特性”循序渐进。  
2) **每个例子只讲一个点**：能复制到业务代码里直接改造使用。  
3) **可运行 / 可复现**：DB 例子保证自建 schema（DROP/CREATE），输出稳定。  
4) **文档可索引**：有一个入口索引所有 examples，并标注 feature / 运行方式。

### 非目标

- 不把 `examples/` 做成完整教程（教程放 `docs/`）。
- 不强依赖外部服务（除了 Postgres 本身）。
- 不在本 spec 里讨论 API 设计取舍（另开 RFC/ADR）。

## 约定（建议先统一）

### 运行方式

- 统一 `DATABASE_URL`：`postgres://postgres:postgres@localhost/pgorm_example`
- 示例头部统一写：
  - `cargo run --example <name> -p pgorm`
  - 若需要 feature：`--features <...>`

### Schema 隔离

- 每个 example 只管理自己的表集合：启动时 `DROP TABLE IF EXISTS ... CASCADE` 再 `CREATE TABLE ...`。
- 避免依赖“上一个 example 跑过”的状态。

### Example 复用代码

目前每个 example 都有一份几乎相同的 `common/schema.rs` / `common/output.rs`。
建议后续做一次收敛（两种路线二选一即可）：

- 路线 A：`examples/common/` + `#[path = "../common/mod.rs"] mod common;` 复用
- 路线 B：新增 workspace 内部 crate：`pgorm-example-support`（更干净，但更重）

## Roadmap（按优先级）

> 说明：下面的“例子名”建议与 Cargo example 名称一致（`crates/pgorm/Cargo.toml` 里的 `[[example]]`）。

### P0（优先补齐：能力已实现但无范例）

- [x] `sql_builder`（`crates/pgorm/examples/sql_builder/main.rs`）
  - 覆盖点：
    - `pgorm::sql()`：`push/push_bind/push_bind_list/push_ident/to_sql`
    - `Condition/Op`：`in_list/between/is_null/ilike` 等
    - `builder::{WhereExpr, OrderBy, Pagination}`：动态过滤 + 排序 + 分页的推荐写法
    - `Sql::fetch_scalar_one` / `Sql::exists`（若接 DB）
  - 建议结构：
    - 先打印 `to_sql()` 结果（让用户理解占位符生成）
    - 再给出一个“可选 filter”函数：`build_query(filters) -> Sql`

- [x] `insert_many`（`crates/pgorm/examples/insert_many/main.rs`）
  - 覆盖点：
    - `InsertModel::insert_many` / `insert_many_returning`
    - `PgType`/数组 cast 的隐式机制（用户不需要懂实现，但要知道“适合 bulk”）
    - 与 `auto_now_add` 的组合（可选：顺带展示）
  - 建议 schema：`products(id, name, price_cents, created_at, updated_at)`

- [x] `upsert`（`crates/pgorm/examples/upsert/main.rs`）
  - 覆盖点：
    - `#[orm(conflict_target = "...")]` vs `#[orm(conflict_constraint = "...")]`
    - `#[orm(conflict_update = "...")]`（部分字段更新）
    - `upsert` / `upsert_returning`
    - `upsert_many` / `upsert_many_returning`
  - 建议 schema：带 unique key（如 `products(sku UNIQUE)` 或 `tags(name UNIQUE)`）。

- [x] `write_graph`（`crates/pgorm/examples/write_graph/main.rs`）
  - 覆盖点：
    - `InsertModel` graph：`belongs_to / has_one / has_many / after_insert`
    - `insert_graph_returning` / `insert_graph_report`（展示 `WriteReport/WriteStepReport`）
    - 事务边界：建议用 `pgorm::transaction!` 把 graph 写入包起来（示例强调“原子性”）
  - 建议 schema：
    - `categories`（父表）
    - `products`（root，belongs_to category）
    - `product_tags` / `audit_logs`（after_insert）

### P1（增强可观测性与易用性）

- [x] `monitoring`（`crates/pgorm/examples/monitoring/main.rs`）
  - 覆盖点：
    - `MonitorConfig`：超时/慢查询阈值
    - `LoggingMonitor` / `StatsMonitor`
    - 自定义 `QueryHook`：演示 `Abort/ModifySql/Continue`

备注：`pg_client` 已覆盖“带检查的推荐客户端”；这里主要补齐“只监控/只 hook”的玩法。

- [x] `jsonb`（`crates/pgorm/examples/jsonb/main.rs`）
  - 覆盖点：
    - `pgorm::Json<T>`：强类型 jsonb
    - `serde_json::Value`：动态 jsonb
    - 简单的 jsonb 插入 + 查询

### P2（补齐“语义边界/易踩坑”）

- [x] `fetch_semantics`（`crates/pgorm/examples/fetch_semantics/main.rs`）
  - 覆盖点：
    - `fetch_one*` vs `fetch_one_strict*` 的差异
    - `fetch_opt*` 的推荐用法
    - `OrmError::NotFound` 的处理方式

## 验收标准（DoD）

- 每个新增 example：
  - 头部包含运行命令与 `DATABASE_URL` 说明
  - 自建/清理 schema，不依赖外部状态
  - 输出包含关键结果（至少能肉眼确认“确实跑了/确实生效了”）
- `cargo test -p pgorm --tests` 通过（不新增需要 DB 的 test）
- `cargo build -p pgorm --examples` 通过（如需额外 feature，补 `required-features` 或在 README 标注）

## Open Questions

1) Examples 是否要 **强制只用一个数据库名**（`pgorm_example`），还是每个 example 用不同 schema/table 前缀？  
2) `write_graph` example 的 scope：只演示 `insert_graph*`，还是顺带把 `update_by_id_graph*` 也做出来？  
3) `sql_builder` 是否要做到“无 DB 可运行”（只打印 SQL），还是默认也连 DB 做一轮真实查询？
