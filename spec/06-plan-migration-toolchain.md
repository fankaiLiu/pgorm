# 06 Plan: Migration Toolchain (CLI + Library)

状态: Draft  
优先级: P0  
目标版本: v0.4.x

## 1. 背景与问题

当前 CLI 命令仅 `gen/model/sql`（`crates/pgorm-cli/src/cli.rs:16`），库侧 `migrate` 仅 `run/run_pool`（`crates/pgorm/src/migrate.rs:29`）。

缺少完整迁移工具链：`new/up/down/status/diff`，导致用户要额外接入第三方脚本。

## 2. 目标

- 提供可直接落地的迁移工作流：创建、执行、查看状态、差异草案。
- CLI 与库 API 统一语义，支持 CI/CD。

## 3. 非目标

- 不做“自动安全重构 SQL”的黑盒方案。
- 不在首版实现跨数据库方言。

## 4. 命令设计

- `pgorm migrate init`：初始化迁移目录与模板。
- `pgorm migrate new <name>`：创建新迁移文件。
- `pgorm migrate up [--to <version>]`：执行迁移。
- `pgorm migrate down [--steps N|--to <version>]`：回滚。
- `pgorm migrate status`：显示本地/已应用版本。
- `pgorm migrate diff`：生成 schema 差异草案（SQL skeleton）。

## 5. 库 API 设计

- `migrate::plan(...)`：返回待执行迁移列表。
- `migrate::run_to(target)`：执行到目标版本。
- `migrate::status(...)`：返回迁移状态结构体。
- `migrate::down(...)`：回滚接口（需要可逆策略支持）。

## 6. 关键实现点

### 6.1 迁移元数据

- 定义本地迁移扫描器（版本、名称、checksum）。
- 与 `refinery_schema_history` 对齐。

### 6.2 down 策略

二选一并在首版固定：

- 方案 A: `Vx__name.up.sql` + `Vx__name.down.sql` 双文件。
- 方案 B: 仅支持 `--to` 到旧版本，且要求 migration 元信息声明 reversible。

### 6.3 diff 策略

- 首版先做“基础差异草案”（表/列增删改）。
- 复杂对象（索引、约束、函数）先标记 TODO 注释，避免误导为“全自动”。

## 7. 实施拆分

### M1: init/new/up/status

- 先提供 80% 常用能力。

### M2: down

- 引入可逆迁移约束与校验。

### M3: diff

- 先最小可用，再逐步覆盖约束/索引。

## 8. 风险与兼容

- 风险: `down` 与现有单文件迁移格式冲突。
- 缓解: 提供迁移格式升级命令与明确错误提示。

## 9. 验收标准

- 新项目可从 `init` 到 `up` 全流程跑通。
- `status` 可准确展示 pending/applied。
- `down` 至少支持一步回滚。
- `diff` 至少生成可审阅的 SQL 草案文件。
