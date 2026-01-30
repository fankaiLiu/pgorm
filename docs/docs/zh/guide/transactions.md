# 事务

pgorm 中所有查询执行都接受 `&impl GenericClient`，因此相同的代码可以用于普通客户端连接或事务内部。

## 使用 transaction! 宏

```rust
use pgorm::{query, OrmResult};

// 适用于 `tokio_postgres::Client` 和 `deadpool_postgres::Client`。
pgorm::transaction!(&mut client, tx, {
    query("UPDATE users SET last_login = NOW() WHERE id = $1")
        .bind(1_i64)
        .execute(&tx)
        .await?;

    query("INSERT INTO login_history (user_id) VALUES ($1)")
        .bind(1_i64)
        .execute(&tx)
        .await?;

    Ok(())
})?;
```

## 下一步

- 下一章：[`迁移：refinery`](/zh/guide/migrations)

## 手动事务管理

你也可以手动管理事务：

```rust
let tx = client.transaction().await?;

query("UPDATE users SET balance = balance - $1 WHERE id = $2")
    .bind(100_i64)
    .bind(1_i64)
    .execute(&tx)
    .await?;

query("UPDATE users SET balance = balance + $1 WHERE id = $2")
    .bind(100_i64)
    .bind(2_i64)
    .execute(&tx)
    .await?;

tx.commit().await?;
```

## 嵌套事务（保存点）

支持 PostgreSQL 保存点用于嵌套事务：

```rust
pgorm::transaction!(&mut client, tx, {
    query("INSERT INTO orders (user_id) VALUES ($1)")
        .bind(1_i64)
        .execute(&tx)
        .await?;

    // 订单项的保存点
    let savepoint = tx.savepoint("items").await?;

    // 如果失败，只回滚到保存点
    let result = query("INSERT INTO order_items (order_id, product_id) VALUES ($1, $2)")
        .bind(1_i64)
        .bind(100_i64)
        .execute(&savepoint)
        .await;

    match result {
        Ok(_) => savepoint.commit().await?,
        Err(_) => savepoint.rollback().await?,
    }

    Ok(())
})?;
```
