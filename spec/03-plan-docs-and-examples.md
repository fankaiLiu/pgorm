# 计划：文档与示例（README / cookbook / examples）

目标：让新用户 30 秒跑通、5 分钟写对；让示例成为“可执行/可编译的真实用法”，并在 CI 中被持续验证。

## README 改进（信息架构）

- [ ] Quick Start 只保留一条推荐路线：`PgClient`（带 feature/环境说明）
- [ ] 把“SQL 模式 / Model 模式 / 监控与检查”作为二级章节展开
- [ ] 明确“安全边界”：动态 ident、raw SQL、policy（where/limit）默认行为
- [ ] feature 表补充“依赖成本/用途/是否推荐默认”

## 示例工程化

- [ ] 每个 example 顶部统一模板：
  - 运行命令（含 `-p pgorm`）
  - 是否需要 `DATABASE_URL`
  - `required-features`（如果有）
- [ ] 示例尽量做到：
  - “无 DB”也能展示 SQL 生成（打印 SQL + params 数）
  - “有 DB”再跑 live demo（明确跳过逻辑）
- [ ] 把关键示例对应到 compile-only tests（防 API 漂移）

## Cookbook（最佳实践集合）

- [ ] 新增 `docs/cookbook.md`（或 `docs/` 下分多篇）：
  - 分页（page vs keyset）
  - 动态 where/order（用 `Ident`/`Condition` 的正确姿势）
  - 事务/保存点
  - 乐观锁重试模式
  - bulk insert/upsert 的性能注意事项

## 中英文同步

- [ ] `README.md` 与 `README.zh-CN.md` 的章节结构保持一致
- [ ] 关键 API（PgClient/Sql/derive）示例两边都要有

## CI 守护

- [ ] CI 至少保证：
  - `cargo fmt --check`
  - `cargo clippy ... -D warnings`
  - `cargo test --workspace`
- [ ] 对 examples：至少保证能编译（`--all-targets` 已覆盖大部分）

## 验收标准（Definition of Done）

- [ ] README 新用户不需要翻很多页就能跑通
- [ ] examples 是“持续可用”的（CI 守护）
- [ ] cookbook 覆盖 80% 常见坑与推荐做法

