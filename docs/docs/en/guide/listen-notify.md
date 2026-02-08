# LISTEN/NOTIFY Events

pgorm provides `PgListener` for PostgreSQL `LISTEN/NOTIFY` subscriptions.

Use it for low-latency in-cluster events such as:

- Cache invalidation
- Background worker triggers
- Lightweight domain events

`LISTEN/NOTIFY` is **best-effort**. It is not a durable queue and does not provide exactly-once delivery.

## Quick Start

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

## Dedicated Connection Model

`PgListener` uses a dedicated PostgreSQL connection.

Do not run long-lived subscriptions on regular pooled query connections. Keep:

- one pool for request/query traffic
- one dedicated listener connection per listener worker

This avoids pool starvation and lifecycle conflicts.

## Reconnect Behavior

By default, reconnect is enabled:

- State transitions: `Connected -> Reconnecting -> Connected`
- Backoff: exponential between `reconnect_backoff_min` and `reconnect_backoff_max`
- Subscriptions are restored automatically after reconnect

Important: notifications emitted while disconnected can be lost.

## Queue / Backpressure

`PgListenerConfig` controls buffering:

- `queue_capacity`: internal queue size
- `queue_policy`:
  - `DropNewest`: drop new messages when full (default)
  - `Block`: apply backpressure and wait for consumer

For high-volume channels, monitor dropped notifications via stats.

## State and Stats

```rust
println!("state={:?}", listener.state());
let stats = listener.stats();
println!(
    "reconnects={}, dropped={}",
    stats.reconnect_count,
    stats.dropped_notifications
);
```

## Graceful Shutdown

```rust
listener.unlisten("orders_changed").await?;
listener.close().await?;
```

## Runnable Example

- `crates/pgorm/examples/listen_notify/main.rs`

---

Next: [Models & Derive Macros](/en/guide/models)
