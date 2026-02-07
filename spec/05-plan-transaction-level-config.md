# 05 Plan: Transaction Isolation/ReadOnly/Deferrable Config

状态: Draft  
优先级: P0  
目标版本: v0.4.x

## 1. 背景与问题

`transaction!` 宏当前固定走 `client.transaction()`（`crates/pgorm/src/transaction.rs:49`），无法配置：

- isolation level（`READ COMMITTED`/`REPEATABLE READ`/`SERIALIZABLE`）
- read-only
- deferrable

在需要强一致、报表只读事务、队列消费防并发冲突等场景下，这个缺口会直接影响可用性。

## 2. 目标

- 提供统一事务选项 API（宏 + 函数）。
- 同时覆盖 `tokio_postgres::Client` 与 `deadpool_postgres::Client`。
- 默认行为保持不变（不配置即现状）。

## 3. 非目标

- 不改变 savepoint 语义。
- 不引入分布式事务（2PC）。

## 4. 设计方案

### 4.1 新增类型

- `IsolationLevel` enum。
- `TransactionOptions` builder：
  - `isolation(...)`
  - `read_only(bool)`
  - `deferrable(bool)`

### 4.2 新增执行入口

- 函数式：`begin_transaction_with(client, options)`。
- 宏式：`transaction_with!(&mut client, tx, options, { ... })`。
- 兼容：`transaction!` 维持现有行为。

### 4.3 实现策略

- 优先使用 `tokio-postgres` transaction builder（若 API 可用）。
- 对不支持的组合做明确错误（例如非 serializable + deferrable）。

## 5. 实施拆分

### M1: 类型与最小 API

- 定义 `TransactionOptions` 与校验规则。
- 新增 `begin_transaction_with`。

### M2: 宏集成

- 新增 `transaction_with!`。
- 文档示例替换为推荐写法（保留旧写法示例）。

### M3: 测试与文档

- 单测: 选项组合校验。
- 集成测试: 事务行为（成功提交/失败回滚）不变。
- 文档: 事务隔离级别选择建议。

## 6. 风险与兼容

- 风险: 不同连接类型可用 API 不一致。
- 缓解: 在 trait 层封装，运行时给清晰报错。

## 7. 验收标准

- 可配置 isolation/read-only/deferrable 并成功开启事务。
- 未配置时与现有 `transaction!` 行为一致。
- 文档新增“何时选用 SERIALIZABLE/READ ONLY”。
