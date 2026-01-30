# Transactions

All query execution in pgorm takes `&impl GenericClient`, so the same code works with a plain client connection or inside a transaction.

## Using the transaction! Macro

```rust
use pgorm::{query, OrmResult};

// Works with `tokio_postgres::Client` and `deadpool_postgres::Client`.
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

## Manual Transaction Management

You can also manage transactions manually:

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

## Nested Transactions (Savepoints)

PostgreSQL savepoints are supported for nested transactions:

```rust
pgorm::transaction!(&mut client, tx, {
    query("INSERT INTO orders (user_id) VALUES ($1)")
        .bind(1_i64)
        .execute(&tx)
        .await?;

    // Savepoint for order items
    let savepoint = tx.savepoint("items").await?;

    // If this fails, only rollback to savepoint
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
