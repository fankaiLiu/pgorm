# 事务与保存点

pgorm 中所有查询执行都接受 `&impl GenericClient`，因此相同的代码既可以用于普通客户端连接，也可以用于事务内部。pgorm 提供了用于事务、保存点和嵌套事务的宏。

## 1. `transaction!` 宏

`transaction!` 宏在 `Ok` 时自动提交，在 `Err` 时自动回滚。它同时支持 `tokio_postgres::Client` 和 `deadpool_postgres::Client`。

```rust
use pgorm::{query, OrmError};

pgorm::transaction!(&mut client, tx, {
    query("UPDATE accounts SET balance = balance - $1 WHERE name = $2")
        .bind(200_i64)
        .bind("Alice")
        .execute(&tx)
        .await?;

    query("UPDATE accounts SET balance = balance + $1 WHERE name = $2")
        .bind(200_i64)
        .bind("Bob")
        .execute(&tx)
        .await?;

    Ok::<(), OrmError>(())
})?;
```

该宏展开为 `BEGIN` / `COMMIT` / `ROLLBACK` 调用。如果代码块返回 `Err`，事务会自动回滚。

## 2. 手动事务

如果你需要更多控制，可以手动管理事务：

```rust
let tx = client.transaction().await?;

query("UPDATE accounts SET balance = balance - $1 WHERE id = $2")
    .bind(100_i64)
    .bind(1_i64)
    .execute(&tx)
    .await?;

query("UPDATE accounts SET balance = balance + $1 WHERE id = $2")
    .bind(100_i64)
    .bind(2_i64)
    .execute(&tx)
    .await?;

tx.commit().await?;
```

## 3. 保存点

PostgreSQL 保存点允许你在事务中创建"检查点"。如果保存点之后发生了错误，你可以回滚到该点而不必中止整个事务。

### 显式保存点：`pgorm_savepoint`、`.release()`、`.rollback()`

```rust
use pgorm::{OrmError, TransactionExt, query};

pgorm::transaction!(&mut client, tx, {
    // 保存点之前的操作
    query("UPDATE accounts SET balance = balance - $1 WHERE name = $2")
        .bind(200_i64)
        .bind("Alice")
        .execute(&tx)
        .await?;

    // 创建命名保存点
    let sp = tx.pgorm_savepoint("credit_bob").await?;

    query("UPDATE accounts SET balance = balance + $1 WHERE name = $2")
        .bind(200_i64)
        .bind("Bob")
        .execute(&sp)
        .await?;

    // 在事务内使其永久生效
    sp.release().await?;
    // 或者回滚：sp.rollback().await?;

    Ok::<(), OrmError>(())
})?;
```

### 保存点回滚

当你回滚保存点时，只有保存点之后的操作会被撤销。事务的其余部分不受影响：

```rust
pgorm::transaction!(&mut client, tx, {
    query("UPDATE accounts SET balance = balance - $1 WHERE name = $2")
        .bind(100_i64)
        .bind("Alice")
        .execute(&tx)
        .await?;

    let sp = tx.pgorm_savepoint("risky_op").await?;

    query("UPDATE accounts SET balance = balance + $1 WHERE name = $2")
        .bind(100_i64)
        .bind("Bob")
        .execute(&sp)
        .await?;

    // 决定回滚 -- Bob 的入账被撤销
    sp.rollback().await?;

    // Alice 的扣款仍然有效
    Ok::<(), OrmError>(())
})?;
```

### `savepoint!` 宏（自动释放/回滚）

`savepoint!` 宏在 `Ok` 时自动释放，在 `Err` 时自动回滚：

```rust
pgorm::transaction!(&mut client, tx, {
    query("UPDATE accounts SET balance = balance - $1 WHERE name = $2")
        .bind(50_i64)
        .bind("Alice")
        .execute(&tx)
        .await?;

    // 命名保存点 -- Ok 时自动提交，Err 时自动回滚
    let result: Result<(), OrmError> = pgorm::savepoint!(tx, "bonus", sp, {
        query("UPDATE accounts SET balance = balance + $1 WHERE name = $2")
            .bind(50_i64)
            .bind("Bob")
            .execute(&sp)
            .await?;
        Ok(())
    });

    Ok::<(), OrmError>(())
})?;
```

### `nested_transaction!` 宏（匿名保存点）

用于可组合的函数，在不需要命名的情况下自动创建嵌套保存点：

```rust
pgorm::transaction!(&mut client, tx, {
    query("UPDATE accounts SET balance = balance - $1 WHERE name = $2")
        .bind(300_i64)
        .bind("Alice")
        .execute(&tx)
        .await?;

    // 匿名保存点 -- Ok 时自动释放，Err 时自动回滚
    pgorm::nested_transaction!(tx, inner, {
        query("UPDATE accounts SET balance = balance + $1 WHERE name = $2")
            .bind(300_i64)
            .bind("Bob")
            .execute(&inner)
            .await?;
        Ok::<(), OrmError>(())
    })?;

    Ok::<(), OrmError>(())
})?;
```

## 4. 带逐条保存点的批量处理

一个常见模式：在事务中处理一批记录，使用保存点处理单条失败而不中止整个批次。

```rust
pgorm::transaction!(&mut client, tx, {
    let rows = query("SELECT id, value FROM items ORDER BY id")
        .fetch_all(&tx)
        .await?;

    let mut success = 0_i32;
    let mut failed = 0_i32;

    for row in &rows {
        let id: i64 = row.try_get_column("id")?;
        let value: i32 = row.try_get_column("value")?;

        // 每条记录都有自己的保存点
        let result: Result<(), OrmError> = pgorm::savepoint!(tx, sp, {
            if value < 0 {
                return Err(OrmError::validation(
                    format!("invalid value {value} for item {id}")
                ));
            }
            query("UPDATE items SET processed = TRUE WHERE id = $1")
                .bind(id)
                .execute(&sp)
                .await?;
            Ok(())
        });

        match result {
            Ok(()) => {
                success += 1;
                println!("Processed item {id}");
            }
            Err(e) => {
                failed += 1;
                println!("Failed item {id}: {e}");
            }
        }
    }

    println!("Batch complete: {success} succeeded, {failed} failed");
    Ok::<(), OrmError>(())
})?;
```

失败的记录会回滚到其保存点，而成功的记录在事务内保持提交状态。

## 5. 使用建议

- **保持事务简短。** 长时间运行的事务会持有锁，可能导致竞争。在事务外做计算，在事务内做写入。
- **使用保存点实现部分失败容忍。** 如果批次中一条记录失败，只有该记录的保存点被回滚。
- **`nested_transaction!` 具有良好的组合性。** 如果一个函数使用了 `nested_transaction!`，无论是在顶层调用还是在另一个事务内部调用，都能正常工作。
- **三种客户端类型都支持。** `transaction!` 接受 `tokio_postgres::Client`、`deadpool_postgres::Client` 和连接池连接。

## 下一步

- 下一章：[多表写入图](/zh/guide/write-graph)
