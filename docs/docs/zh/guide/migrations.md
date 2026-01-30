# 迁移：`refinery`

pgorm 通过可选 feature `migrate` 集成了 `refinery`，用于运行 SQL migrations。

## 1) 启用 feature

```toml
[dependencies]
pgorm = { version = "0.1.1", features = ["migrate", "pool"] }
```

> 如果你不使用内置连接池，也可以自己管理 `tokio_postgres::Client`；但示例里一般用 `pool`。

## 2) 嵌入迁移文件：`embed_migrations!`

建议的目录结构示例：

```text
your_app/
  migrations/
    V1__init.sql
    V2__add_users.sql
```

在代码里嵌入：

```rust
use pgorm::{create_pool, migrate};

mod embedded {
    use pgorm::embed_migrations;
    embed_migrations!("./migrations");
}

let pool = create_pool(&database_url)?;
let report = migrate::run_pool(&pool, embedded::migrations::runner()).await?;
println!("Applied {} migration(s)", report.applied_migrations().len());
```

> 可运行示例见 `crates/pgorm/examples/migrate`（需要 `--features migrate,pool`）。

## 3) 常见问题

1) **路径是相对哪里？** `embed_migrations!("./migrations")` 以当前 crate 的 `Cargo.toml` 所在目录为基准。  
2) **生产环境怎么跑？** 通常放在应用启动阶段（或单独的迁移 job），并做好并发部署时的互斥/幂等策略。  

## 下一步

- 下一章：[`监控与 Hook`](/zh/guide/monitoring)
