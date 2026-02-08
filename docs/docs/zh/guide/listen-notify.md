# LISTEN/NOTIFY 事件

pgorm 通过 `PgListener` 提供 PostgreSQL `LISTEN/NOTIFY` 订阅能力。

适合低延迟的库内事件场景，例如：

- 缓存失效通知
- 后台任务触发
- 轻量级领域事件分发

`LISTEN/NOTIFY` 是 **best-effort** 语义，不是持久化队列，也不保证 exactly-once。

## 快速开始

```rust
use pgorm::{
    OrmError, PgListener, PgListenerConfig, PgListenerQueuePolicy,
};
use std::time::Duration;

let cfg = PgListenerConfig::new()
    .queue_capacity(256)
    .queue_policy(PgListenerQueuePolicy::DropNewest)
    .reconnect(true)
    .reconnect_backoff(Duration::from_millis(250), Duration::from_secs(5));

let mut listener = PgListener::connect_with_no_tls_config(&database_url, cfg).await?;
listener.listen("orders_changed").await?;

while let Some(msg) = listener.next().await {
    let msg = msg?;
    println!("channel={} payload={}", msg.channel, msg.payload);
}
```

## 专用连接模型

`PgListener` 使用专用 PostgreSQL 连接。

不要把长期订阅放在普通查询连接池连接上。建议分离为：

- 一个连接池处理请求/查询流量
- 一个（或多个）专用 listener 连接处理通知流

这样可以避免连接池饥饿和生命周期冲突。

## 重连行为

默认开启自动重连：

- 状态流转：`Connected -> Reconnecting -> Connected`
- 退避策略：在 `reconnect_backoff_min` 和 `reconnect_backoff_max` 之间指数退避
- 重连成功后自动恢复订阅

注意：断连窗口内产生的通知可能丢失。

## 队列与背压

`PgListenerConfig` 提供缓冲控制：

- `queue_capacity`：内部队列容量
- `queue_policy`：
  - `DropNewest`：队列满时丢弃新消息（默认）
  - `Block`：阻塞发送端，对消费者施加背压

高频通知场景建议结合统计监控丢弃数。

## 状态与统计

```rust
println!("state={:?}", listener.state());
let stats = listener.stats();
println!(
    "reconnects={}, dropped={}",
    stats.reconnect_count,
    stats.dropped_notifications
);
```

## 优雅关闭

```rust
listener.unlisten("orders_changed").await?;
listener.close().await?;
```

## 可运行示例

- `crates/pgorm/examples/listen_notify/main.rs`

---

下一步：[模型与派生宏](/zh/guide/models)
