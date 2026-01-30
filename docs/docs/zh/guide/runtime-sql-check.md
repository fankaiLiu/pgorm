# 运行时 SQL 检查：`PgClient` / `CheckedClient`

当你在运行时生成 SQL（尤其是“AI 生成 SQL”或“动态拼接 SQL”）时，容易出现两类问题：

1) **查了不存在的表/列**（上线才爆）  
2) **危险 SQL**（例如 `DELETE` 没有 `WHERE`、`SELECT` 没有 `LIMIT`）  

pgorm 提供“运行时检查”作为兜底：在 SQL 发给数据库之前，先根据已注册的 schema 做校验/策略检查。

## 0) 前置：需要启用 `check` feature

默认 features 已包含 `check`。如果你关闭了默认特性，需要手动打开：

```toml
[dependencies]
pgorm = { version = "0.1.1", features = ["check"] }
```

## 1) 推荐：`PgClient`（检查 + 监控 + 策略）

`PgClient` 是“开箱即用的推荐客户端”，它把下面这些整合在一起：

- `#[derive(Model)]` 自动注册表/列信息（inventory）
- SQL 校验（WarnOnly / Strict / Disabled）
- 基础安全策略（例如阻止危险 DML）
- 统计/日志/慢查询（默认开统计，可配置）

```rust
use pgorm::{CheckMode, PgClient, PgClientConfig, query};
use std::time::Duration;

let pg = PgClient::with_config(
    &client,
    PgClientConfig::new()
        .check_mode(CheckMode::WarnOnly) // 默认 WarnOnly
        .timeout(Duration::from_secs(30))
        .slow_threshold(Duration::from_millis(100))
        .with_stats(),
);

// 之后把 &pg 当作连接传给 query()/sql()/Model 方法即可
let n: i64 = query("SELECT COUNT(*) FROM products")
    .tag("products.count")
    .fetch_scalar_one(&pg)
    .await?;
```

### 严格模式：检查失败直接阻止执行

```rust
let pg_strict = PgClient::with_config(&client, PgClientConfig::new().strict());

// 如果 SQL 引用了不存在的表/列，会在执行前返回 OrmError::Validation(...)
let _ = query("SELECT nonexistent FROM products")
    .fetch_all(&pg_strict)
    .await?;
```

> 注意：`strict()` 只影响“SQL 检查与策略”，不会改变 `fetch_one` 的行数语义。行数语义请看：[`Fetch 语义`](/zh/guide/fetch-semantics)。

## 2) 轻量选项：`CheckedClient`（只做 SQL 校验）

如果你只想要“schema 校验”，不关心监控与策略，可以用 `CheckedClient`：

```rust
use pgorm::{CheckedClient, query};

let checked = CheckedClient::new(&client).strict();

let _ = query("SELECT id, name FROM products")
    .fetch_all(&checked)
    .await?;
```

## 3) 运行时安全策略（可选但推荐）

`PgClientConfig` 内置了一些“安全策略”开关（属于运行时兜底，尤其适合生成型 SQL）：

- `SELECT` 没有 `LIMIT`
- `DELETE` 没有 `WHERE`
- `UPDATE` 没有 `WHERE`
- `TRUNCATE`、`DROP TABLE` 等

你可以在 `PgClientConfig` 上进行定制（生产环境建议更严格）。

## 4) 看可运行示例

- `crates/pgorm/examples/pg_client`：展示 registry、校验结果、Strict/WarnOnly、统计等

## 下一步

- 下一章：[`输入校验与 Input（#[orm(input)]）`](/zh/guide/validation-and-input)
