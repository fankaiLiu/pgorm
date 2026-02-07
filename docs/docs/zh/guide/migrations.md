# 数据库迁移

pgorm 通过可选的 `migrate` feature 集成了 [refinery](https://github.com/rust-db/refinery)，用于在 PostgreSQL 数据库上运行 SQL 迁移。

## 启用 Feature

在 `Cargo.toml` 中添加带有 `migrate` feature 的 pgorm：

```toml
[dependencies]
pgorm = { version = "0.3.0", features = ["migrate"] }
```

如果你同时使用连接池（大多数应用都会使用）：

```toml
pgorm = { version = "0.3.0", features = ["migrate", "pool"] }
```

## 迁移文件命名

迁移文件遵循 refinery 命名约定：`V{number}__{description}.sql`（注意是双下划线）。将它们放在 `migrations/` 目录中：

```text
your_app/
  migrations/
    V1__create_products.sql
    V2__create_categories.sql
    V3__add_users.sql
```

迁移文件示例：

```sql
-- V1__create_products.sql
CREATE TABLE IF NOT EXISTS products (
    id BIGSERIAL PRIMARY KEY,
    name TEXT NOT NULL,
    price_cents BIGINT NOT NULL,
    in_stock BOOLEAN NOT NULL DEFAULT TRUE
);
```

```sql
-- V2__create_categories.sql
CREATE TABLE IF NOT EXISTS categories (
    id BIGSERIAL PRIMARY KEY,
    name TEXT NOT NULL UNIQUE
);
```

## 嵌入和运行迁移

使用 `embed_migrations!` 宏在编译时将 SQL 文件嵌入到二进制文件中，然后通过 `migrate::run_pool` 运行：

```rust
use pgorm::{create_pool, migrate};

mod embedded {
    use pgorm::embed_migrations;
    embed_migrations!("./migrations");
}

#[tokio::main]
async fn main() -> pgorm::OrmResult<()> {
    let database_url = std::env::var("DATABASE_URL")
        .map_err(|_| pgorm::OrmError::Connection("DATABASE_URL is not set".to_string()))?;

    let pool = create_pool(&database_url)?;
    let report = migrate::run_pool(&pool, embedded::migrations::runner()).await?;

    println!("Applied {} migration(s)", report.applied_migrations().len());
    Ok(())
}
```

### 关键要点

- **`embed_migrations!("./migrations")`** -- 路径相对于当前 crate 的 `Cargo.toml` 所在目录
- **`embedded::migrations::runner()`** -- 返回一个 refinery `Runner`，用于跟踪哪些迁移已经执行
- **`migrate::run_pool(&pool, runner)`** -- 使用连接池中的连接执行待运行的迁移

## 生产环境注意事项

1. **在启动时或专用任务中运行迁移。** 大多数应用在服务器进程启动时运行迁移。对于大型部署，建议在部署流程中将迁移作为单独的步骤执行。

2. **处理并发。** Refinery 使用 `refinery_schema_history` 表加锁来防止多个实例同时运行迁移。但为了安全起见，应确保一次只有一个实例运行迁移（例如通过 Kubernetes init 容器或部署脚本）。

3. **保持迁移仅向前推进。** 每次迁移应该是累加的。避免修改或删除已执行的迁移文件 -- refinery 会通过校验哈希来检测变更。

4. **在 CI 中测试迁移。** 在 CI 流水线中对全新数据库运行迁移套件，以便尽早发现问题。

## Refinery 文档

如需了解高级功能（迁移分组、异步运行器、不同后端），请参阅 [refinery 文档](https://docs.rs/refinery)。

## 可运行示例

参见 `crates/pgorm/examples/migrate/main.rs`，完整的可运行示例。运行方式：

```bash
DATABASE_URL=postgres://postgres:postgres@localhost/pgorm_example \
  cargo run --example migrate -p pgorm --features migrate
```

## 下一步

- 下一章：[CLI：pgorm-cli](/zh/guide/cli)
