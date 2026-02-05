# spec（设计文档 / RFC）

本目录用于存放 `pgorm` 的**设计说明、方案对比、Roadmap、取舍记录（ADR）**等“工程决策类文档”。它不是用户手册：面向使用者的教程/指南请放在 `docs/`（Rspress 站点）或仓库根目录的 `README.md` / `AI_USAGE.md`。

## 1. 文档分层（建议）

- **用户文档（How-to / Guide）**：`docs/docs/*`（中英文站点内容）
- **API 入口（是什么 + 最短路径）**：根 `README.md`
- **AI/工程协作（提示词、约定）**：`AI_USAGE.md` / `llms.txt`
- **设计文档（Why / Trade-offs / RFC）**：`spec/*`（本目录）

## 2. `spec/` 的组织方式（建议）

以“模块 + 主题”来拆分，每篇文档尽量只回答一个问题：

1) **核心库（crates/pgorm）**：SQL-first API、执行模型、事务、pool、tag/可观测性等  
2) **检查能力（crates/pgorm-check）**：SQL 解析/规范化、schema 校验、lint/policy、指纹/manifest  
3) **派生宏（crates/pgorm-derive）**：`FromRow/Model/*Model` 的语义、约束、生成代码边界  
4) **CLI（crates/pgorm-cli）**：`pgorm.toml`、命令设计、CI 入口、生成物闭环  
5) **可选特性**：`migrate`（refinery）、`validate`（changeset 风格校验）等

> 经验法则：**和代码目录一一映射**，能让 spec 更容易维护，也更容易落到实现与测试。

## 3. 目录结构（推荐落地形态）

当前 `spec/` 只有少量文件；可以逐步演进为下面的结构（不需要一次性做完）：

```text
spec/
  README.md                    # 本索引
  adr/                         # 架构决策记录（Architecture Decision Records）
  core/                        # crates/pgorm 相关设计
  check/                       # crates/pgorm-check 相关设计
  derive/                      # crates/pgorm-derive 相关设计
  cli/                         # crates/pgorm-cli 相关设计
```

如果你更喜欢“单层扁平化”，也可以保持 `spec/*.md`，但建议至少加上编号前缀（便于排序与引用）：

```text
spec/
  00-overview.md
  10-cli-*.md
  20-check-*.md
  30-derive-*.md
  40-core-*.md
  90-adr-*.md
```

## 4. 模块索引（按仓库实际代码）

### 4.0 总览 / 路线图

- `spec/00-ai-first-roadmap.md`：AI-first 路线图（把 AI 当成默认入口，SQL 资产化 + 校验闭环）

### 4.1 CLI（`crates/pgorm-cli`）

- 现有：
  - `spec/pgorm-cli.md`：CLI 优化计划与 Roadmap（建议后续拆分到 `spec/cli/*`）
  - `spec/54-cli-schema-generation.md`：Schema 生成（DDL Generation / Schema Diff / 迁移生成）计划
- 建议拆分主题（可逐篇补齐）：
  - `pgorm.toml` 配置规范（version/engine/database/schema_cache/packages/models）
  - 命令与工作流：`pgorm gen/*`、`pgorm model`、`pgorm sql check`
  - SQL 资产模型：queries/migrations/seeds/裸 SQL 的输入边界与约定
  - schema cache：`auto/refresh/cache_only`、缓存文件格式、刷新策略
  - 生成物闭环：`--check`、指纹/manifest、CI 失败语义（exit code / json 输出）

### 4.2 SQL 检查与策略（`crates/pgorm-check`）

建议主题（可逐篇补齐）：

- SQL 解析范围：支持的语法子集、多语句切分策略、错误定位（file:line/stmt_idx）
- schema 校验：从数据库 introspection 到本地 cache 的数据模型
- lint/policy：规则集合、warn/error 分级、可配置性（离线/运行时一致性）
- normalized SQL & fingerprint：去噪规则、稳定性保证、用于 diff/manifest 的字段定义

### 4.3 派生宏（`crates/pgorm-derive`）

- 现有：
  - `spec/55-derive-field-modifiers.md`：字段修饰符（只读字段 / 不可变字段 / 生命周期回调 / 字段转换器）计划

建议主题（可逐篇补齐）：

- `FromRow`：字段映射、可选/非空、jsonb、类型覆盖策略
- `Model/*Model`：表/视图、主键、关系声明（has_many/belongs_to）、eager loading 的生成规则
- 兼容性边界：哪些事情必须显式写 SQL，哪些可以宏生成；breaking change 策略

### 4.4 核心库（`crates/pgorm`）

建议主题（可逐篇补齐）：

- examples 规划：
  - `spec/pgorm-examples.md`：examples 覆盖面补齐计划与 Roadmap
- 数据库类型映射：
  - `spec/40-core-inet-ipaddr.md`：PostgreSQL `inet` / Rust `IpAddr` 支持增强方案与里程碑
- Query Builder 体验：
  - `spec/41-core-query-optional-filters.md`：可选过滤（`*_opt` / `apply_if_*`）语法糖设计与计划
- SQL builder：`query()` vs `sql()` 的定位，bind 占位符生成规则
- 执行语义：`fetch_one*` / `fetch_one_strict*` / `fetch_all*` 的行为约定
- `PgClient/CheckedClient`：运行时检查/观测的组合方式、性能与开关策略
- pool/transaction：GenericClient 抽象、事务宏的边界与错误处理

补充计划（已新增草案）：

- `spec/42-core-streaming-rowstream.md`：流式查询（RowStream）与监控集成计划
- `spec/43-core-prepared-statement-cache.md`：prepared statement 缓存（per-connection）计划
- `spec/44-core-keyset-pagination.md`：keyset/cursor pagination（seek method）计划
- `spec/45-core-eager-loading-extensions.md`：eager loading 扩展（has_one / many-to-many）计划
- `spec/46-core-condition-operator-extensions.md`：Condition/Op 扩展（IS DISTINCT FROM、array/jsonb、ANY/ALL、全文检索）计划
- `spec/47-ai-runtime-manifest-allowlist.md`：运行时 manifest allowlist（AI-first 门禁）计划
- `spec/48-core-optimistic-locking.md`：乐观锁（版本号字段 + 并发冲突检测）计划
- `spec/49-core-aggregate-queries.md`：聚合查询（count/sum/avg/min/max + group_by）计划
- `spec/50-core-bulk-operations.md`：批量更新/删除（update_many / delete_many）计划
- `spec/51-core-cte-queries.md`：CTE（WITH 子句）查询支持计划
- `spec/52-core-pg-special-types.md`：PostgreSQL 特有类型（ENUM / Range / Composite）支持计划
- `spec/53-core-transaction-enhancements.md`：事务增强（Savepoint / 嵌套事务）计划

### 4.5 可选特性

- `migrate`（refinery）：迁移来源、嵌入方式、与 schema cache/检查的协同
- `validate`：changeset 风格校验模型、内置校验器的规则与扩展点

## 5. 单篇 Spec 模板（复制即用）

```md
# <标题>

状态：Draft | Accepted | Implemented | Deprecated  
相关代码：crates/<name>/...  
最后更新：YYYY-MM-DD

## 背景

## 目标 / 非目标

## 方案

## 关键取舍（Trade-offs）

## 对外接口（CLI/API/Config）

## 兼容性与迁移

## 里程碑 / TODO

## Open Questions
```
