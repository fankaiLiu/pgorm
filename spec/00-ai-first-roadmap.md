# pgorm AI-first 路线图（把 AI 当成默认入口）

状态：Draft  
相关代码：`crates/pgorm-check/*`、`crates/pgorm-cli/*`、`crates/pgorm/*`、`docs/*`  
最后更新：2026-01-30

## 背景

`pgorm` 目前的定位是 **SQL-first ORM**：开发者显式写 SQL（或半动态拼接），框架提供类型映射、派生宏、schema 校验与运行时防护。

但在实际工程里，“SQL 的第一作者”越来越经常是 **LLM（AI）**：

- 需求/意图是自然语言（或业务规则），SQL 由 AI 生成/改写。
- AI 生成的 SQL **不可信**：可能语义错误、类型不匹配、越权访问、性能灾难（全表扫/无 LIMIT）、或产生破坏性 DML。
- 团队需要一个“可审计、可门禁、可回滚”的闭环：**生成 → 校验 → 资产化 → 上线 → 观测 → 反馈**。

`pgorm` 已经具备 AI-first 的关键护栏（schema cache、SQL 解析/规范化、离线校验、运行时 strict 检查、Hook/监控）。缺少的是：把这些能力组织成 **默认的 AI 工作流**，并提供明确的产物与验收标准。

## 目标 / 非目标

### 目标（AI-first 的含义）

1) **AI 是默认入口**：从“描述需求/约束”开始，而不是从“手写 SQL”开始。  
2) **SQL 仍然是真相（Source of Truth）**：最终落盘为 SQL 资产（文件/manifest），可 code review、可 diff、可追踪。  
3) **离线 + 运行时双重护栏**：CI 离线校验 + 线上运行时策略拦截，默认更安全。  
4) **可观测与可反馈**：把线上真实 SQL、指纹、慢查询等反馈回开发侧，让下一轮 AI 生成更稳。

### 非目标（本路线图暂不做）

- 不把 `pgorm` 做成“内置某家 LLM 的一体化产品”（避免网络/秘钥/合规耦合）。默认走 **BYO-LLM**：你用任何模型生成 SQL，`pgorm` 负责提供上下文与校验闭环。
- 不承诺“自然语言直接执行 SQL”（避免把未审查的生成 SQL 直接打到生产）。
- 不替代数据库权限/审计体系（`pgorm` 只做应用层防护与工程闭环）。

## 总体方案（AI-first 闭环）

把 AI 生成 SQL 的流程拆成 6 个可组合的环节，每个环节都有明确产物：

1) **Schema Context**（上下文）  
   - 输入：数据库 schema（来自 `.pgorm/schema.json`）  
   - 输出：适合 LLM 的“可读上下文”（精简后的 schema 摘要 + 约束 + 示例）

2) **Prompt Contract**（约定/规范）  
   - 输出：`AI_USAGE.md` / `llms.txt` + prompt 模板（告诉 AI：允许做什么、必须输出什么格式、哪些规则必须满足）

3) **SQL 资产化（Assets）**  
   - 输出：`queries/**/*.sql` / `sql/**/*.sql`（最终 SQL 落盘），可被 `pgorm gen` 生成 Rust 代码

4) **离线校验（CI Gate）**  
   - 输出：稳定的校验结果（text/json），用于 PR 门禁  
   - 工具：`pgorm sql check`、`pgorm gen`（已有能力）

5) **运行时防护（Runtime Guardrails）**  
   - 输出：线上拒绝危险 SQL、拒绝未批准 SQL（可选）、记录指纹与耗时  
   - 工具：`CheckedClient` / `InstrumentedClient` + policy/hook（已有能力，需补齐“批准清单”）

6) **反馈回流（Feedback Loop）**  
   - 输出：线上真实 SQL 指纹、慢查询样本、错误样本 → 进入下一轮 prompt/context  
   - 工具：manifest/采样导出（需补齐）

## 需要新增/强化的“AI-first 产物”

> 这些产物是 AI-first 的核心：**让生成可控、让校验可重复、让上线可门禁。**

### 1) LLM Context Pack（离线生成）

- 位置建议：`.pgorm/ai/`（或 `./.pgorm/` 下固定文件名）
- 组成建议：
  - `context.md`：人/LLM 可读的 schema 摘要（表、列、主外键、典型查询模式、禁止项）
  - `context.json`：机器可读（后续可做模板化、压缩、diff）
  - `examples.sql`：项目内“推荐写法”样例（占位符 `$1/$2`、分页、软删除等约定）

### 2) SQL Manifest（稳定清单 + 指纹）

- 位置建议：`.pgorm/sql-manifest.json`
- 每条 statement 建议字段：
  - `source`（file）/ `stmt_idx`
  - `normalized_sql`
  - `fingerprint`
  - `kind`（select/insert/update/delete/ddl）
  - `tables`（读/写集合）
  - `tags`（可选：业务域、owner、风险等级）

用途：
- PR/CI：新 SQL 必须进入 manifest（可审查）
- Runtime：可选启用“**只允许执行 manifest 内 SQL**”（AI 生成 SQL 默认不允许直接上线执行）

### 3) Policy Pack（AI 默认安全策略）

把离线 lint 与运行时 hook/policy 对齐成“策略包”：
- `SELECT` 默认要求 `LIMIT`（可按目录/标签豁免）
- `UPDATE/DELETE` 默认要求 `WHERE`
- `TRUNCATE/DROP` 只允许在 migrations 目录（或仅 warning）

## CLI 计划（把 AI-first 做成默认工作流）

### 新增命令组：`pgorm ai`

> 核心原则：默认不调用网络；只做上下文/模板/校验/清单闭环。

- `pgorm ai init`  
  生成/更新：
  - `AI_USAGE.md`（提示词约定 + 输出格式）
  - `llms.txt`（给 IDE/LLM 工具索引入口）
  - `.pgorm/ai/prompts/*`（模板）

- `pgorm ai context [--out ...]`  
  读取 `.pgorm/schema.json`（或 `pgorm.toml` 的 schema_cache），生成 `.pgorm/ai/context.md|json`。

- `pgorm ai manifest [--inputs <glob...>]`（或并入 `pgorm sql manifest`）  
  扫描 SQL 资产 → split statements → normalize/fingerprint → 写 `.pgorm/sql-manifest.json`。

- `pgorm ai check`（聚合命令，偏向 AI-first 命名）  
  等价于（或封装）：
  - `pgorm sql check`（校验生成 SQL）
  - `pgorm gen --check`（生成物一致性）
  - `pgorm ai manifest --check`（manifest up-to-date）

## 运行时计划（把“AI 生成”变成“可控执行”）

### 1) Manifest Allowlist（可选强门禁）

在 `CheckedClient`（或新 wrapper）增加一个可选模式：
- 若开启 allowlist：执行前计算 canonical/normalized SQL 的 fingerprint
- fingerprint 不在 `.pgorm/sql-manifest.json` → **拒绝执行**（提示如何将 SQL 资产化并更新 manifest）

这条能力把“AI 动态生成/拼接 SQL”从根上变成：**只能在 dev/staging 试验，不能直接绕过 PR 上生产**（除非显式允许）。

### 2) Query Tagging（可追踪）

建议在客户端侧统一提供 tag 入口（已有 `tag()`/monitor 体系则沿用），并在日志/指标里同时输出：
- tag
- fingerprint
- duration
- rows

## 里程碑 / TODO（按优先级）

### Phase 0：把 AI 工作流“写清楚”（1–2 天）

- [ ] 在仓库根新增 `AI_USAGE.md` 与 `llms.txt`（先手写，后续可由 `pgorm ai init` 生成）
- [ ] 文档：在 `docs/` 增加一页 “AI-first Workflow”（链接到 `pgorm sql check`/`gen schema`/`gen`）
- [ ] 明确 SQL 资产目录约定（`queries/` vs `sql/` vs `migrations/`）与默认 policy

验收：新同学只看文档就能走通“生成 → 校验 → 代码生成 → 运行时防护”的最小闭环。

### Phase 1：Context Pack + Manifest（3–7 天）

- [ ] `pgorm ai context`：schema → context.md/json
- [ ] `pgorm sql manifest`（或 `pgorm ai manifest`）：生成 `.pgorm/sql-manifest.json`
- [ ] `pgorm ai check`：一条命令跑完 sql check + gen --check + manifest --check

验收：CI 能稳定失败；manifest diff 清晰；context 可直接喂给 LLM。

### Phase 2：运行时 Allowlist + Policy Pack（1–2 周）

- [ ] Runtime：可选启用 manifest allowlist（默认关闭，提供清晰迁移路径）
- [ ] Policy pack：离线与运行时规则一致，支持按目录/标签分级
- [ ] 观测：fingerprint 在日志/指标里可追踪

验收：线上默认拦截危险 SQL；开启 allowlist 后，新 SQL 无法绕过 PR 上线执行。

### Phase 3：反馈回流（1–2 周）

- [ ] `pgorm ai export-corpus`：导出线上慢查询/错误样本（脱敏后）+ fingerprint 对照
- [ ] `pgorm ai analyze`：按表/指纹聚合问题（慢、错、风险）
- [ ] 用 corpus 反哺 prompt 模板（最佳实践沉淀）

验收：AI 生成 SQL 的质量可持续提升，问题能被快速定位到“哪条 SQL/哪段 prompt”。

## Open Questions

1) `pgorm ai` 是否需要提供“可选 provider 插件”（如 feature + env 配置），还是坚持 BYO-LLM 到底？  
2) allowlist 模式如何兼容“运行时动态 SQL”（建议：只允许动态拼接片段，主 SQL 资产化；或提供 staging 捕获 + 审批机制）？  
3) policy 的默认强度：对 `SELECT` 无 `LIMIT` 是 error 还是 warning？是否按目录分级更合适？  
4) context 的输出格式：纯文本（最好用） vs JSON（可模板化/压缩）。是否两种都要？  

