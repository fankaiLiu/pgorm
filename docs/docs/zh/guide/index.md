# 快速开始

pgorm 是一个专为 PostgreSQL 设计的 Rust ORM 库，保持 SQL 显式化。

## 安装

在 `Cargo.toml` 中添加 pgorm：

```toml
[dependencies]
pgorm = "0.1.4"
```

如果只需要 SQL 构建器（不需要连接池/派生宏/运行时 SQL 检查）：

```toml
[dependencies]
pgorm = { version = "0.1.4", default-features = false }
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
| `rust_decimal` | `rust_decimal::Decimal` 的 PgType 支持（用于 UNNEST cast）   |
| `time`     | `time` crate 类型支持（启用 tokio-postgres `with-time-0_3`）        |
| `cidr`     | `cidr` crate 类型支持（启用 tokio-postgres `with-cidr-0_3`）        |
| `geo_types` | `geo-types` crate 类型支持（启用 tokio-postgres `with-geo-types-0_7`） |
| `eui48`    | `eui48` crate 类型支持（启用 tokio-postgres `with-eui48-1`）        |
| `bit_vec`  | `bit-vec` crate 类型支持（启用 tokio-postgres `with-bit-vec-0_8`）  |
| `extra_types` | 便捷开关：开启以上所有常见额外类型                           |

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

## 学习路径（建议按顺序）

如果你是第一次使用 pgorm，建议按下面的顺序阅读：

1) 连接池：[`create_pool` / TLS / 推荐客户端](/zh/guide/pooling)  
2) 手写 SQL：[`query()`](/zh/guide/query)  
3) 动态 SQL：[`sql()`](/zh/guide/sql-builder)  
4) 动态条件：[`Condition/WhereExpr/OrderBy/Pagination`](/zh/guide/conditions)  
5) Fetch 语义：[`fetch_one` vs `fetch_one_strict` vs `fetch_opt`](/zh/guide/fetch-semantics)  
6) 行映射：[`FromRow` / `RowExt` / JSONB / INET](/zh/guide/from-row)  
7) 模型与派生宏：[`Model / InsertModel / UpdateModel`](/zh/guide/models)  
8) 关系声明：[`has_many` / `belongs_to`](/zh/guide/relations)  
9) 预加载：[`load_*`（避免 N+1）](/zh/guide/eager-loading)  
10) 写入：[`InsertModel`](/zh/guide/insert-model) / [`UpdateModel`](/zh/guide/update-model) / [`Upsert`](/zh/guide/upsert)  
11) 高级写入：[`多表写入图（Write Graph）`](/zh/guide/write-graph)  
12) 事务：[`transaction!` / 保存点](/zh/guide/transactions)  
13) 迁移：[`refinery` 迁移](/zh/guide/migrations)  
14) 监控与 Hook：[`InstrumentedClient`](/zh/guide/monitoring)  
15) 运行时 SQL 检查：[`PgClient / CheckedClient`](/zh/guide/runtime-sql-check)  
16) 输入校验：[`#[orm(input)]` changeset 风格](/zh/guide/validation-and-input)  
