# pgorm-cli 优化计划（面向 pgorm 项目）

> 目标：让 `pgorm-cli` 不再“鸡肋”，而是成为 **SQL 资产管理 + 离线检查 + 生成物一致性** 的入口；同时与运行时的 `pgorm::PgClient/CheckedClient` 形成闭环。

## 1. 现状与痛点

### 1.1 SQL 来源分裂
- **离线（CLI）**：`queries/**/*.sql`（sqlc 风格）/ 裸 SQL 文件 / migrations / seeds
- **运行时（Rust）**：`pgorm::query("...")`、`pgorm::sql("...")` 动态拼接、derive 生成的 SQL

当 SQL “真实执行版本” 不在文件里时，CLI 很难做可靠的离线检查；反之，如果 SQL 以文件为真相，CLI 才能 100% 对齐线上执行。

### 1.2 检查能力与项目不贴合
- 仅检查 sqlc 风格查询文件会漏掉大量真实 SQL（migrations、脚本、include_str 引入的 SQL 等）
- 多语句 SQL（migrations/seed）需要分 statement 检查；不应一刀切拒绝
- 需要面向 CI 的输出（稳定格式/可机读）、可配置的规则强度（warn vs error）

### 1.3 “生成器”缺少闭环
生成代码后需要：
- 能 `--check` 保证生成物 up-to-date
- 能把“生成出来的 SQL”也纳入检查（或者至少能证明生成 SQL 的集合/指纹稳定）

## 2. 设计原则（更贴合 Rust 项目）

1) **单一真相（Single source of truth）**  
   推荐把业务查询 SQL 放在文件里，然后 Rust 用 `include_str!` 引入；CLI 检查的就是线上会执行的那份 SQL。

2) **离线检查不尝试穷举运行时分支**  
   动态 SQL 的正确性由运行时 `PgClient/CheckedClient` 的 strict 检查兜底；CLI 负责检查“可枚举的 SQL 资产”。

3) **输出稳定、可 CI**  
   失败/警告规则明确，支持 JSON 输出，方便在 CI 挂门禁。

4) **可扩展的输入模型**  
   不只支持 sqlc header；要把 “SQL 文件 = 资产” 抽象成统一输入源。

## 3. Roadmap（按优先级）

> 注：下面是“计划”。其中一部分能力已经在仓库里实现（例如 `pgorm sql check`、`pgorm model`）；后续按阶段补全闭环与工程化。

### Phase A（立即收益，1–3 天）

#### A1. 引入统一入口：`pgorm check`
目的：一条命令跑完常用校验，适配 CI。

建议行为：
- `pgorm check` 默认执行：
  - `pgorm gen check`（如果配置了 `[[packages]]`）
  - `pgorm sql check`（按配置扫描 SQL 资产；见 A2）
  - `pgorm model --check`（如果配置了 `[models]`）
- 支持 `--deny-warnings`、`--format text|json`、`--only gen|sql|model`、`--skip ...`
- Exit code 约定：
  - 0：无 error（warn 可由 deny-warnings 控制）
  - 1：有 error（或 deny-warnings 时有 warning）

#### A2. 配置化 SQL 资产输入（让 CLI 更贴项目）
在 `pgorm.toml` 增加一个可选段，例如：
```toml
[sql]
inputs = ["sql/**/*.sql", "migrations/**/*.sql"]
allow_multi = true
deny_warnings = false
```

要点：
- inputs 支持 glob（与 packages.queries 同风格）
- allow_multi 用于 migrations/seed
- 对 migrations 可提供弱化规则（例如不对 `DROP`/`TRUNCATE` 报警，或仅 warning）

#### A3. 输出增强（定位/可读性）
- 每条 issue 保留：`file:line`（如果可取）、`stmt_idx`、`rule_code`、`level`、`message`
- 对 SQL 输出：同时显示 `normalize` 后的 SQL + `fingerprint`（方便去重/追踪）

### Phase B（可维护性提升，3–7 天）

#### B1. 统一 schema cache 加载逻辑
- `gen/gen_check/model/sql check/schema` 不再各写一套加载逻辑
- 统一行为：mode=auto/refresh/cache_only，schemas 选择规则一致

#### B2. 提供 “SQL Manifest” 机制（桥接离线与运行时）
目标：把 SQL 资产（文件）生成一个稳定清单，用于：
- CI 做差异（新 SQL 是否被审查）
- 运行时监控对齐（tag/指纹）

建议新增：
- `pgorm sql manifest --config pgorm.toml --out .pgorm/sql-manifest.json`
- 内容：每条 statement 的 `{source, stmt_idx, normalized_sql, fingerprint, tables, kind}`

### Phase C（更强的“生成器闭环”，1–2 周）

#### C1. “运行时代码生成 SQL” 的可检查化
CLI 无法直接拿到 `pgorm::sql()` 在运行时拼出来的最终 SQL。建议在工程里引入一个约定，让这些 SQL 可被枚举：

方案 1（推荐，最 Rust/最稳）：
- 动态 SQL 尽量仅做拼接“条件片段”，主 SQL 仍在文件里：`include_str!("...")`
- 这样离线可检查，运行时 `strict()` 再兜底

方案 2（测试驱动枚举）：
- 写一个 `#[cfg(test)]` 的“SQL 审计测试”，在测试里跑所有代码路径，收集实际执行 SQL（或调用 `Model::generated_sql()` / 自己维护列表）
- CLI 提供 `pgorm check --only runtime`：调用 `cargo test -p your_crate sql_audit`（这会引入对外部工程的编译/运行耦合，需要你确认是否接受）

#### C2. SQL 安全策略（policy）与项目一致
把 `pgorm` 运行时 `SqlPolicy` 的规则，做一个 CLI 镜像（离线同样提示）：
- `SELECT` 无 `LIMIT`
- `DELETE`/`UPDATE` 无 `WHERE`
- `TRUNCATE`/`DROP TABLE`（按环境/目录分级）

## 4. 推荐工程约定（让 CLI 与代码一致）

### 4.1 SQL 文件化 + include_str（推荐默认）
- `sql/`：业务查询（单语句）
- `migrations/`：迁移（允许多语句）
- Rust：
  - `const SQL: &str = include_str!("../sql/users/by_id.sql");`
  - `pgorm::query(SQL).bind(...).fetch_one_as(...)`

收益：CLI 检查 == 线上执行。

### 4.2 运行时强约束（线上兜底）
在 prod/staging 使用：
- `PgClientConfig::new().strict()` 或 `CheckedClient::new(client).strict()`
- 让所有实际执行 SQL 都经过 schema/lint/policy 检查

## 5. 验收标准（Definition of Done）

- `pgorm check` 能在 CI 一条命令跑完，并给出稳定 exit code
- `pgorm sql check` 能覆盖你项目中的 SQL 资产（queries/migrations/sql）
- issue 输出能直接定位（source + stmt + rule），并支持 JSON
- 推荐的工程约定能让“离线检查”和“运行时执行”不再分叉

## 6. 需要你确认的关键选择

1) `pgorm check` 是否要 **默认** 执行 `gen/sql/model` 三件套？还是只做 `sql`？
2) migrations 的策略：多语句允许，但对 `DROP/TRUNCATE` 规则要不要降级？
3) 是否接受 CLI 通过 `cargo test` 方式做“运行时 SQL 审计”（会变成强耦合）？

