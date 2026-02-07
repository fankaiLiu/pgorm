# 安装与功能标志

## Cargo.toml

将 pgorm 添加到你的项目中：

```toml
[dependencies]
pgorm = "0.3.0"
```

默认功能（`pool`、`derive`、`check`、`validate`）覆盖了大多数使用场景。如果只需要 SQL 构建器和行映射的最小化构建：

```toml
[dependencies]
pgorm = { version = "0.3.0", default-features = false }
```

## Rust 工具链

- **最低支持版本（MSRV）：** Rust 1.88+
- **Edition：** 2024

pgorm 跟踪最新的稳定 Rust Edition。请确保你的工具链满足最低支持版本要求。

## 功能标志

| 标志 | 默认 | 依赖 | 用途 | 推荐 |
|------|------|------|------|------|
| `pool` | 是 | `deadpool-postgres` | 连接池（`create_pool`） | 服务端推荐开启 |
| `derive` | 是 | `pgorm-derive`（过程宏） | `FromRow`、`Model`、`InsertModel`、`UpdateModel`、`ViewModel`、`QueryParams` | 推荐开启 |
| `check` | 是 | `pgorm-check` + `libpg_query` | SQL 模式检查、lint、`PgClient` | 开发/预发布环境推荐开启 |
| `validate` | 是 | `regex`、`url` | 输入校验（email/url/regex） | 接受用户输入时推荐开启 |
| `migrate` | 否 | `refinery` | SQL 迁移 | 仅迁移运行工具需要 |
| `tracing` | 否 | `tracing` | 通过 `tracing` 输出 SQL（target: `pgorm.sql`） | 使用 tracing 时推荐开启 |
| `rust_decimal` | 否 | `rust_decimal` | `Decimal` 类型支持 | 按需启用 |
| `time` | 否 | `time` | `time` crate 日期/时间类型支持 | 按需启用 |
| `cidr` | 否 | `cidr` | 网络类型支持 | 按需启用 |
| `geo_types` | 否 | `geo-types` | 几何类型支持 | 按需启用 |
| `eui48` | 否 | `eui48` | MAC 地址支持 | 按需启用 |
| `bit_vec` | 否 | `bit-vec` | 位向量支持 | 按需启用 |
| `extra_types` | 否 | 以上所有 | 启用所有可选类型支持 | 便捷别名 |

### 启用可选功能

要启用特定功能：

```toml
[dependencies]
pgorm = { version = "0.3.0", features = ["tracing", "rust_decimal"] }
```

要一次性启用所有额外类型支持：

```toml
[dependencies]
pgorm = { version = "0.3.0", features = ["extra_types"] }
```

## 重导出

pgorm 重导出了 `tokio-postgres` 中的关键类型，因此常见操作不需要将其作为直接依赖添加：

- `tokio_postgres::Row`
- `tokio_postgres::types::ToSql`、`FromSql`、`FromSqlOwned`
- `tokio_postgres::GenericClient`
- `tokio_postgres::NoTls`

当你需要完整的 `tokio-postgres` API（例如不使用连接池的裸连接）时，直接添加它：

```toml
[dependencies]
pgorm = "0.3.0"
tokio-postgres = "0.7"
```

## 最小化构建示例

如果你只需要 SQL 构建器（`sql()`、`query()`）和行映射（`FromRow`），不需要连接池、派生宏或运行时 SQL 检查：

```toml
[dependencies]
pgorm = { version = "0.3.0", default-features = false }
```

这样可以获得最小的依赖体积，同时保留核心查询 API。

---

下一步：[连接与连接池](/zh/guide/connection)
