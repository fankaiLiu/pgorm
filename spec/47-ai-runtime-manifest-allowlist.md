# Runtime SQL Manifest Allowlist（AI-first 门禁）计划

状态：Draft  
相关代码：`crates/pgorm/src/pg_client.rs`、`crates/pgorm/src/check.rs`、`crates/pgorm-check/src/*`、`spec/00-ai-first-roadmap.md`  
最后更新：2026-02-03

## 背景

当 SQL 的作者越来越常是 LLM 时，风险来自两类：

- 语义/类型错误（线上报错、数据错误）
- 安全/性能风险（无 LIMIT 全表扫、危险 DML、越权访问）

`pgorm` 已有运行时校验与 policy（`PgClientConfig::strict()`、`SqlPolicy`），但仍缺一个关键门禁：

> **只允许执行“已审查/已资产化”的 SQL**（allowlist）

Roadmap（`spec/00-ai-first-roadmap.md`）提出了 `.pgorm/sql-manifest.json` 与 fingerprint 机制，本计划用于落地运行时 allowlist。

## 目标 / 非目标

### 目标

1) 提供 `PgClient` 的 **manifest allowlist** 模式（Off/Warn/Enforce）。  
2) allowlist 的匹配基于 **稳定 fingerprint**（而不是原始 SQL 字符串）。  
3) allowlist 与现有 policy 协同：policy 仍然生效（LIMIT/WHERE/危险 DML）。  
4) 输出可诊断：当拒绝执行时，提示如何把 SQL 资产化并更新 manifest。

### 非目标

- 不内置任何 LLM provider 或网络调用（BYO-LLM）。
- 不提供“自动审批”或 runtime 学习（审批必须走 code review/CI）。
- 不替代数据库权限体系（只做应用层门禁）。

## 依赖与前置能力

要做 fingerprint allowlist，必须先解决：

1) **SQL normalization**：把 whitespace/comments/无关差异去噪，保证 fingerprint 稳定。  
2) **SQL fingerprint**：对 normalized SQL 做哈希（或结构化指纹）输出稳定 ID。

目前 `pgorm-check` 具备 pg_query parse 能力与 parse cache，但尚未暴露 “normalized_sql/fingerprint_sql” API；需要先补齐。

## 方案概述（分三层）

### Layer A：pgorm-check 提供 normalize + fingerprint

- `normalize_sql(sql) -> String`：pg_query parse + deparse（或等价规范化）。
- `fingerprint_sql(sql) -> String`：对 normalized_sql 做稳定 hash（如 SHA256/xxhash；需定型）。
- 可选：返回 `StatementKind/tables` 等元数据（复用现有 analysis）。

### Layer B：manifest 文件格式

位置：`.pgorm/sql-manifest.json`（建议）

字段（建议最小集）：
- `fingerprint`
- `normalized_sql`
- `source`（file path）
- `stmt_idx`（多语句文件）
- `tag`（可选）

### Layer C：PgClient runtime allowlist

在 `PgClientConfig` 增加：

```rust,ignore
pub enum AllowlistMode { Off, Warn, Enforce }

impl PgClientConfig {
    pub fn allowlist_manifest_path(mut self, path: impl Into<PathBuf>) -> Self;
    pub fn allowlist_mode(mut self, mode: AllowlistMode) -> Self;
}
```

执行前：
- 计算 fingerprint（对 exec_sql 或 canonical_sql，需统一口径）。
- 查 manifest：命中则继续；未命中则：
  - Warn：记录 warning（monitor），仍执行
  - Enforce：Abort（返回可诊断错误）

可观测性：
- `QueryContext.fields` 增加 `fingerprint`（短 hash）与 `allowlist=hit/miss`。

## 实施计划（Milestones）

### M1（pgorm-check：normalize/fingerprint API）

- [ ] 在 `pgorm-check` 新增 `normalize_sql` 与 `fingerprint_sql`（单语句与多语句策略需定义）。
- [ ] 复用/扩展 parse cache，避免每次都 parse。
- [ ] 单元测试：同义 SQL（不同空格/注释）fingerprint 一致。

### M2（pgorm：manifest 解析与数据结构）

- [ ] 定义 `SqlManifest` struct（serde 反序列化）。
- [ ] 加载方式：启动时从文件加载；支持 reload（可选，M3 再做）。

### M3（PgClient：allowlist 检查）

- [ ] `PgClient` 执行前计算 fingerprint 并检查 manifest。
- [ ] 报错信息：包含 fingerprint、tag、建议命令（例如 `pgorm sql manifest`）与文件路径。
- [ ] 与监控联动：记录 hit/miss。

### M4（CLI 闭环与文档）

- [ ] `pgorm-cli` 增加 `pgorm sql manifest`（或 `pgorm ai manifest`）生成 `.pgorm/sql-manifest.json`。
- [ ] 文档：AI-first 工作流（生成 → 离线 check → 更新 manifest → runtime enforce）。

## 风险与取舍

- fingerprint 算法与 normalize 规则一旦发布就需要稳定性承诺（否则会导致 manifest 大量 churn）。
- 多语句 SQL（migrations/seed）如何切分与 fingerprint：需要与 CLI 的切分规则保持一致。

## Open Questions

1) fingerprint 算法选择：SHA256（稳定但稍慢） vs xxhash（快但碰撞风险更高）。  
2) normalize 采用 pg_query deparse 是否会改变语义/格式（一般不改语义，但可能影响某些边界 SQL）。  
3) allowlist 检查使用 `canonical_sql` 还是 `exec_sql`（若 Hook 会改写 SQL，需要明确以哪个为准）。  

