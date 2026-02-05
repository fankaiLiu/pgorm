# 计划：可维护性（拆文件 / 模块化）

目标：把 `pgorm` 当前“超大单文件”拆成清晰模块，降低认知负担与修改风险，同时 **不破坏对外 API**（尽量保持路径与行为不变）。

## 背景与现状

当前几个文件体量较大、职责混杂，后续加功能/修 bug 会越来越难：

- `crates/pgorm/src/pg_client.rs`（PgClient + statement cache + policy + hook + stats + tests…）
- `crates/pgorm/src/monitor.rs`（监控/Hook/Instrumented stream…）
- `crates/pgorm/src/sql.rs`（SQL builder + exec + bind + validate…）
- `crates/pgorm/src/check.rs`（schema registry + lint/check glue…）

## 约束（拆分规则）

- **不改行为**：除非顺手修复明显 bug，否则只做结构调整（移动/拆分/收紧可见性）。
- **不破坏路径**：外部仍然通过原来的 `pgorm::PgClient`、`pgorm::Sql` 等访问；内部用 `mod` + `pub use` 维持兼容。
- **每次只拆一块**：一次 PR/一次提交范围内只拆一个大文件（降低 review/回滚成本）。
- **每一步都可验证**：拆分完成后跑 `fmt/test/clippy -D warnings`。

## 交付物

- 模块目录化（示例结构，按实际调整）：
  - `crates/pgorm/src/pg_client/`：`mod.rs`, `config.rs`, `statement_cache.rs`, `policies.rs`, `execute.rs`, `tests.rs`
  - `crates/pgorm/src/monitor/`：`mod.rs`, `config.rs`, `hook.rs`, `monitor.rs`, `stream.rs`, `tests.rs`
  - `crates/pgorm/src/sql/`：`mod.rs`, `parts.rs`, `builder.rs`, `exec.rs`, `validate.rs`, `tests.rs`
  - `crates/pgorm/src/check/`：`mod.rs`, `registry.rs`, `lint.rs`, `tests.rs`

## 拆分步骤（推荐从 PgClient 开始）

### Phase 0：准备

- [ ] 新建 `crates/pgorm/src/pg_client/` 目录与 `mod.rs`（先只 `mod`，不搬代码）
- [ ] 在 `crates/pgorm/src/lib.rs` 里把 `mod pg_client;` 改成 `mod pg_client;` + `pub use` 不变（确保对外路径稳定）
- [ ] 跑验收命令确保基线干净

### Phase 1：PgClient 拆分（第一块）

- [ ] 把纯配置结构体（`PgClientConfig`、`StatementCacheConfig`、policy types）移动到 `pg_client/config.rs`
- [ ] 把 statement cache 的内部实现移动到 `pg_client/statement_cache.rs`
- [ ] 把执行路径（query/execute/prepare/timeout/retry）移动到 `pg_client/execute.rs`
- [ ] 把仅测试使用的内容集中到 `pg_client/tests.rs`（或保留 `#[cfg(test)]` 子模块）
- [ ] 在 `pg_client/mod.rs` 中 `pub use` 需要暴露的类型，其余 `pub(crate)` 收紧

### Phase 2：Monitor 拆分（第二块）

- [ ] 同样手法拆 `monitor.rs`：配置、hook、监控器、stream instrumentation 分文件
- [ ] 保留 `InstrumentedClient`/`QueryContext`/`HookAction` 等对外路径不变

### Phase 3：Sql / Check 拆分（第三、四块）

- [ ] 把 `SqlPart`/params 等内部结构迁到 `sql/parts.rs`
- [ ] 把 exec（fetch/stream/strict 语义）迁到 `sql/exec.rs`
- [ ] `check` 模块拆出 `SchemaRegistry` 与 lint/check glue

## 验收标准（Definition of Done）

- [ ] 对外 API 访问路径不变（至少不需要改 README 里的示例代码）
- [ ] `cargo test --workspace` 通过
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` 通过
- [ ] 代码结构能回答三个问题：
  1) “找 PgClient 的配置去哪？” 2) “statement cache 在哪？” 3) “stream instrumentation 在哪？”

## 风险与应对

- **风险：移动后循环依赖/可见性错误**：优先用 `pub(crate)` + `mod` 层级解决，不要把 `pub use` 撒得到处都是。
- **风险：一次拆太大**：严格控制每次只拆一个文件，必要时拆成多次小改动。

