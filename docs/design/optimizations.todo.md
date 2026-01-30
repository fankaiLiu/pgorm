# pgorm 优化 TODO（执行清单）

- 目标：把“容易踩坑的语义/生产接入阻塞点”先补齐；再做热点路径的性能与维护成本优化。
- 最后更新：2026-01-30

## P0（高优先级：语义清晰 + 更易接入生产）

### 1) 明确 `fetch_one/query_one` 语义 + 补齐 `*_strict` API

#### 现状（需要明确写进文档）

- `GenericClient::query_one` 当前语义更接近 **“至少一行就返回第一行”**：
  - 0 行：返回 `OrmError::NotFound`
  - 1 行：返回该行
  - 多行：**返回第一行（不报错）**
- 影响范围：`Query::fetch_one*` / `Sql::fetch_one*` / `PgClient` 等基于 `GenericClient::query_one` 的封装。
- 代码参考：`crates/pgorm/src/client.rs`

#### TODO

- [ ] 文档：在 `crates/pgorm/src/client.rs` / `crates/pgorm/src/sql.rs` 的 rustdoc 中把上述语义写清楚（避免“exactly one”误导）。
- [ ] 文档：在 `README.md`（或 SQL-first 章节）增加一句提示：若期望严格唯一，请使用 `*_strict`。
- [ ] API：新增严格版本（遇到多行时报错）：
  - [ ] `GenericClient::query_one_strict(...)`
  - [ ] `Query::{fetch_one_strict, fetch_one_strict_as, fetch_one_strict_tagged, ...}`
  - [ ] `Sql::{fetch_one_strict, fetch_one_strict_as, ...}`（含 tagged 变体）
- [ ] 错误模型：新增一个可区分的错误（建议 `OrmError::TooManyRows { expected: 1, got: usize }` 或等价信息）。
- [ ] Tests：
  - [ ] `fetch_one`：多行时返回第一行（回归测试，锁定现有行为）
  - [ ] `fetch_one_strict`：0 行 = NotFound；多行 = TooManyRows
- [ ]（可选）`PgClientConfig::strict()` 是否应同时要求 `*_strict` 语义：明确策略并在文档中写清楚（避免“strict 只做 schema check”与“strict 也做 rowcount”概念混淆）。

### 2) `Query/Sql` 增加链式设置 tag（减少 `*_tagged(tag, ...)` 噪音）

#### 目标

- 让调用点更贴近“可观测性”实际用法：先设置 tag，然后正常 `fetch/execute`。

#### TODO

- [ ] API（Query）：新增链式方法（命名二选一即可）：
  - [ ] `Query::tag(tag: impl Into<String>) -> Self`
  - [ ] `Query::with_tag(...) -> Self`
- [ ] API（Sql）：新增链式方法（更贴近 `push/push_bind` 风格）：
  - [ ] `Sql::tag(tag: impl Into<String>) -> &mut Self`（或同时提供 `Sql::with_tag(...) -> Self`）
- [ ] 执行层：当 tag 已设置时，`fetch_* / execute` 自动走 `GenericClient::{query_tagged, query_one_tagged, query_opt_tagged, execute_tagged}`。
- [ ] 兼容性：保留现有 `*_tagged(conn, tag)` 方法（可后续 deprecate，但先不强推 breaking change）。
- [ ] Docs：补一组示例（建议放 README + rustdoc）：
  - `query("...").tag("users.by_id").bind(...).fetch_one_as(&pg).await?;`
  - `sql("...").tag("users.search").push(...).fetch_all_as(&pg).await?;`

### 3) Pool：提供 TLS 与 deadpool 配置注入（生产接入第一步不卡住）

#### 现状

- `create_pool` 固定 `NoTls`，且仅暴露少量配置（如 max_size）。
- 代码参考：`crates/pgorm/src/pool.rs`

#### TODO

- [ ] 新增 `create_pool_with_tls(...)`：
  - [ ] 支持传入 `tls`（`NoTls` / `native-tls` / `rustls` 等），并能返回对应泛型 `Pool<Tls>`。
- [ ] 新增 `create_pool_with_manager_config(...)`（或更通用的 builder/closure 方式）：
  - [ ] 支持注入 `deadpool_postgres::ManagerConfig`
  - [ ] 支持注入 `Pool::builder(...)` 的常见项（timeouts / recycling_method / max_size 等）
- [ ] Docs：在 README 的 pool 段落明确：
  - [ ] `create_pool` 是“快速上手默认值”（适合本地/测试）
  - [ ] 生产建议：明确 TLS 与连接池参数从应用层配置注入
- [ ] Examples：增加一个最小 TLS 示例（可选：单独 example feature/README 片段）。

### 4) 写清楚 MSRV / edition，并在 `Cargo.toml` 增加 `rust-version`

#### 现状

- workspace 使用 `edition = "2024"`，但 README 与 `Cargo.toml` 未声明 MSRV。
- 代码参考：`Cargo.toml`

#### TODO

- [ ] 明确 MSRV（至少需要满足 edition 2024 的最低 Rust 版本；并考虑依赖的 MSRV）。
- [ ] 在 workspace 与各 crate `Cargo.toml` 增加 `rust-version = "..."`（尽量使用 workspace 继承）。
- [ ] README 增加 “Rust toolchain requirement” 小节（明确 edition + rust-version）。
- [ ] CI：固定 toolchain（或至少覆盖 MSRV + stable 两条线，避免误报）。

## P1（中优先级：性能/维护成本）

### 5) `InsertModel::insert_many*` 生成静态/缓存 SQL，减少热点路径分配

#### 现状

- `insert_many` 每次调用都会拼接 `UNNEST($n::type[])` 相关字符串（多处 `format!` + `Vec<String>` + `join`）。
- 代码参考：`crates/pgorm-derive/src/insert_model/gen_base.rs`

#### TODO（推荐优先做“缓存版”，收益高且实现风险低）

- [ ] 方案 A（缓存版 / 推荐）：在生成的方法内部引入 `OnceLock<String>`（或等价）：
  - [ ] 首次调用时根据 `<T as PgType>::pg_array_type()` 组装 SQL 并缓存
  - [ ] 后续调用复用缓存 SQL（避免重复分配/格式化）
- [ ] 方案 B（真正静态 const）：在 derive 阶段直接生成 SQL 字面量：
  - [ ] 需要在宏展开阶段确定每个字段的 PG array type（可能需要新增属性如 `#[orm(pg_array_type = "uuid[]")]` 兜底）
- [ ] 覆盖面：`insert_many` / `insert_many_returning` /（如有）bulk upsert 等同类路径保持一致。
- [ ] Bench/Verify：
  - [ ] 微基准：对比“每次 format” vs “OnceLock 缓存” 的 alloc/latency（热点路径有收益才合并）。
  - [ ] 单测：断言生成 SQL 与 bind 顺序不变（避免改坏 `$n` 对齐）。

### 6) `SchemaRegistry::check_sql` 的 `system_columns` 静态化，减少重复分配

#### 现状

- `check_sql` 每次调用都会构建一个 `HashSet` 存系统列名。
- 代码参考：`crates/pgorm/src/check.rs`

#### TODO

- [ ] 改成无分配实现（优先）：
  - [ ] `matches!(col, "ctid" | "xmin" | "xmax" | "cmin" | "cmax" | "tableoid")`
- [ ] 或改成 `OnceLock<HashSet<...>>`（若后续需要更复杂的集合语义）。
- [ ] Tests：覆盖 INSERT/UPDATE 校验时 system column 应被放行。

### 7) 为高 QPS 用户提供“借用型” builder（减少 `Arc<dyn ToSql>` 堆分配）

#### 背景

- `Sql/Query` 绑定参数用 `Arc<dyn ToSql>`，通用但会引入额外堆分配与引用计数开销。

#### TODO

- [ ] 设计一个借用版本（暂定名 `SqlRef<'a>` / `QueryRef<'a>`）：
  - [ ] 参数存 `&'a (dyn ToSql + Sync)`（或封装为 enum 支持 owned/borrowed 混合）
  - [ ] 明确生命周期约束：调用方必须保证 bind 值活到 query 执行完成
- [ ] API：提供最小可用集合（`push/push_bind_ref/fetch_*` 等），避免与现有 `Sql` 重叠过多。
- [ ] Feature gate：考虑放在 `sql-ref`（或 `unstable`) feature 下，避免主 API 复杂化。
- [ ] Bench：只有在确认明显收益（alloc/吞吐）后再推进到稳定 API。

