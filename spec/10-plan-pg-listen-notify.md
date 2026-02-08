# 10 Plan: PostgreSQL LISTEN/NOTIFY 支持

状态: Draft  
优先级: P1  
目标版本: v0.5.x

## 1. 结论（当前是否缺失）

结论：**当前 `pgorm` 缺少 PostgreSQL `LISTEN/NOTIFY` 的一等支持能力**。

依据：

- 现有核心抽象 `GenericClient` 仅覆盖 query/execute/prepared（`crates/pgorm/src/client.rs:15`），没有订阅通知相关接口。
- 现有流式能力 `StreamingClient` 只覆盖“行流”（`RowStream`），不覆盖异步通知流（`crates/pgorm/src/client.rs:397`）。
- 对外导出仅有 `GenericClient` / `StreamingClient`（`crates/pgorm/src/lib.rs:70`、`crates/pgorm/src/prelude.rs:19`），没有 listener 类型。
- 连接池入口仅封装 deadpool 创建（`crates/pgorm/src/pool.rs:22`），不暴露通知消费面向的连接驱动能力。

## 2. 背景与约束

- PostgreSQL 通知属于 `AsyncMessage::Notification`，语义上不是 SQL 行结果集。
- 通知消费需要持续驱动连接消息循环，不是一次性 query/execute 模式。
- 连接池中的普通业务连接不适合直接复用为长期 listener（会占池、生命周期冲突、通知流消费与查询竞争）。

## 3. 目标

- 提供可产品化的通知订阅能力：`LISTEN channel` + 持续接收 payload。
- 支持单/多 channel 订阅，并提供可取消、可关闭的消费 API。
- 提供基础稳定性能力：断线重连（带退避）、背压策略、错误透传。
- 提供完整示例、文档与集成测试。

## 4. 非目标（首版不做）

- 不保证 exactly-once 或持久化投递语义（通知天然为 best-effort）。
- 不内建业务级重试/去重协议（由上层应用处理）。
- 不把通知与 ORM 查询统计强行混为一套指标模型。

## 5. 设计方案（建议）

### 5.1 新增模块与类型

- 新模块：`crates/pgorm/src/listen.rs`（名称可定为 `listener.rs`）。
- 新导出类型（示意）：
  - `PgNotification { process_id, channel, payload, received_at }`
  - `PgListenerConfig { channels, queue_capacity, reconnect, backoff_min, backoff_max }`
  - `PgListener`（核心句柄）

### 5.2 API 形态（首版）

推荐独立类型，不直接改 `GenericClient` trait（降低公共 trait 变更风险）。

示例 API（草案）：

```rust
let mut listener = PgListener::connect(&database_url).await?;
listener.listen("order_events").await?;

while let Some(msg) = listener.next().await {
    let msg = msg?;
    println!("{} {}", msg.channel, msg.payload);
}
```

补充 API（建议）：

- `listen(channel)` / `unlisten(channel)` / `unlisten_all()`
- `next()`（单条拉取）
- `into_stream()`（转 `Stream<Item = OrmResult<PgNotification>>`）
- `close()`（优雅关闭）

### 5.3 连接与驱动策略

- 使用**专用连接**承载 listener，不复用业务 pool 连接。
- 内部任务持续驱动连接消息循环，筛选 `Notification` 并转发到有界队列。
- 队列满时策略可配：
  - `DropOldest`
  - `DropNewest`
  - `BlockProducer`（默认不建议）

### 5.4 重连策略

- 可配置指数退避重连。
- 重连成功后自动恢复 `LISTEN channel...`。
- 重连期间通过错误事件或状态接口向上游暴露健康状态。

### 5.5 与现有 API 的关系

- `GenericClient` / `StreamingClient` 保持不变（首版）。
- `PgClient` 可在后续阶段增加便捷构造器，但底层仍基于独立 listener 组件。

## 6. 分阶段实施

### M1: 最小可用（P1）

- 增加 `PgNotification`、`PgListener`、基础 `listen/unlisten/next`。
- 单连接、单进程内消费；支持多 channel。
- 提供 `examples/listen_notify/main.rs`。

验收：

- 两连接用例（A `LISTEN`，B `NOTIFY`）可稳定收到消息。
- `UNLISTEN` 后不再收到对应 channel 消息。

### M2: 稳定性增强（P1）

- 重连 + 自动恢复订阅。
- 有界队列与背压策略配置。
- 增加运行状态观测（最少：连接状态、重连次数、丢弃计数）。

验收：

- 人为断连后可自动恢复并继续接收通知。
- 背压压测下行为符合配置策略。

### M3: 产品化集成（P2）

- 文档章节（EN/ZH）+ cookbook 示例。
- 若需要，补充 `PgClient` 便捷入口（不破坏现有 API）。
- 明确与 pool 并存最佳实践（listener 专用连接）。

验收：

- 文档从 0 到可用（含“为什么不能直接拿 pool 查询连接当 listener”说明）。
- examples 可直接运行并覆盖常见场景。

## 7. 测试计划

- 集成测试：
  - 单 channel 收发
  - 多 channel 路由
  - `UNLISTEN` 行为
  - 断线重连恢复
- 并发与压力：
  - 高频 `NOTIFY` 下队列策略行为
  - 慢消费者场景
- 兼容回归：
  - 现有 query/streaming/transaction 测试必须无回归

## 8. 风险与缓解

- 风险：连接生命周期管理复杂，容易泄漏后台任务。  
  缓解：`Drop + close()` 双保险，任务统一由内部管理器持有。
- 风险：高峰通知导致内存增长。  
  缓解：有界队列 + 明确丢弃策略 + 指标暴露。
- 风险：重连窗口丢消息（通知语义本身不保证持久）。  
  缓解：文档明确语义，并建议业务侧引入补偿拉取机制。

## 9. 验收标准（统一）

- 用户可在 `pgorm` 中以稳定 API 订阅 PostgreSQL 通知，不需要自行管理底层消息循环。
- 断线恢复、背压策略、错误路径有可验证行为。
- 文档、示例、测试齐备并通过工作区 CI。
