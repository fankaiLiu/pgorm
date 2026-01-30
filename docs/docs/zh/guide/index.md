# 快速开始

pgorm 是一个专为 PostgreSQL 设计的 Rust ORM 库，保持 SQL 显式化。

## 安装

在 `Cargo.toml` 中添加 pgorm：

```toml
[dependencies]
pgorm = "0.1.0"
```

如果只需要 SQL 构建器（不需要连接池/派生宏/运行时 SQL 检查）：

```toml
[dependencies]
pgorm = { version = "0.1.0", default-features = false }
```

## Rust 工具链

- Edition: 2024
- MSRV: Rust 1.88+

## 功能标志

默认功能：`pool`、`derive`、`check`、`validate`。

| 功能       | 描述                                                              |
| ---------- | ----------------------------------------------------------------- |
| `pool`     | deadpool-postgres 连接池辅助函数 (`create_pool`)                   |
| `derive`   | 过程宏 (`FromRow`、`Model`、`InsertModel`、`UpdateModel`、`ViewModel`) |
| `check`    | 运行时 SQL 检查 + 推荐包装器 (`CheckedClient`、`PgClient`)         |
| `validate` | 变更集风格的验证辅助函数 (email/url/regex 等)                      |
| `migrate`  | 通过 `refinery` 进行 SQL 迁移                                      |

## 基本用法

pgorm 提供两个主要的查询 API：

- 使用 `query()` 执行带有 `$1, $2, ...` 占位符的完整 SQL 字符串
- 使用 `sql()` 动态组合 SQL，无需手动跟踪 `$n`

```rust
use pgorm::{query, sql, FromRow};

#[derive(FromRow)]
struct User {
    id: i64,
    username: String,
}

// 手写 SQL
let user: User = query("SELECT id, username FROM users WHERE id = $1")
    .bind(1_i64)
    .fetch_one_as(&client)
    .await?;

// 动态 SQL 组合（占位符自动生成）
let mut q = sql("SELECT id, username FROM users WHERE 1=1");
q.push(" AND username ILIKE ").push_bind("%admin%");
let users: Vec<User> = q.fetch_all_as(&client).await?;
```

## 可观测性标签

你可以附加可观测性标签用于追踪：

```rust
let user: User = query("SELECT id, username FROM users WHERE id = $1")
    .tag("users.by_id")
    .bind(1_i64)
    .fetch_one_as(&client)
    .await?;
```
