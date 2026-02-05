# Transactions & Savepoints

All query execution in pgorm takes `&impl GenericClient`, so the same code works with a plain client connection or inside a transaction. pgorm provides macros for transactions, savepoints, and nested transactions.

## 1. `transaction!` macro

The `transaction!` macro auto-commits on `Ok` and rolls back on `Err`. It works with both `tokio_postgres::Client` and `deadpool_postgres::Client`.

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

The macro expands to `BEGIN` / `COMMIT` / `ROLLBACK` calls. If the block returns `Err`, the transaction is rolled back automatically.

## 2. Manual transactions

If you need more control, manage transactions yourself:

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

## 3. Savepoints

PostgreSQL savepoints let you create "checkpoints" within a transaction. If something fails after a savepoint, you can roll back to that point without aborting the entire transaction.

### Explicit savepoints: `pgorm_savepoint`, `.release()`, `.rollback()`

```rust
use pgorm::{OrmError, TransactionExt, query};

pgorm::transaction!(&mut client, tx, {
    // Work before the savepoint
    query("UPDATE accounts SET balance = balance - $1 WHERE name = $2")
        .bind(200_i64)
        .bind("Alice")
        .execute(&tx)
        .await?;

    // Create a named savepoint
    let sp = tx.pgorm_savepoint("credit_bob").await?;

    query("UPDATE accounts SET balance = balance + $1 WHERE name = $2")
        .bind(200_i64)
        .bind("Bob")
        .execute(&sp)
        .await?;

    // Make it permanent within the transaction
    sp.release().await?;
    // Or roll it back: sp.rollback().await?;

    Ok::<(), OrmError>(())
})?;
```

### Savepoint rollback

When you roll back a savepoint, only the work done after the savepoint is undone. The rest of the transaction survives:

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

    // Decide to roll back -- Bob's credit is undone
    sp.rollback().await?;

    // Alice's debit still stands
    Ok::<(), OrmError>(())
})?;
```

### `savepoint!` macro (auto release/rollback)

The `savepoint!` macro automatically releases on `Ok` and rolls back on `Err`:

```rust
pgorm::transaction!(&mut client, tx, {
    query("UPDATE accounts SET balance = balance - $1 WHERE name = $2")
        .bind(50_i64)
        .bind("Alice")
        .execute(&tx)
        .await?;

    // Named savepoint -- auto commits on Ok, rolls back on Err
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

### `nested_transaction!` macro (anonymous savepoint)

For composable functions where you want automatic savepoint nesting without naming:

```rust
pgorm::transaction!(&mut client, tx, {
    query("UPDATE accounts SET balance = balance - $1 WHERE name = $2")
        .bind(300_i64)
        .bind("Alice")
        .execute(&tx)
        .await?;

    // Anonymous savepoint -- auto release on Ok, rollback on Err
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

## 4. Batch processing with per-record savepoints

A common pattern: process a batch of records inside a transaction, using savepoints to handle individual failures without aborting the whole batch.

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

        // Each record gets its own savepoint
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

Failed records are rolled back to their savepoint, while successful records remain committed within the transaction.

## 5. Tips

- **Keep transactions short.** Long-running transactions hold locks and can cause contention. Do your computation outside the transaction, then write inside it.
- **Use savepoints for partial failure tolerance.** If one record in a batch fails, only that record's savepoint is rolled back.
- **`nested_transaction!` composes well.** If a function uses `nested_transaction!`, it works correctly whether called at the top level or inside another transaction.
- **All three client types work.** `transaction!` accepts `tokio_postgres::Client`, `deadpool_postgres::Client`, and pool connections.

## Next

- Next: [Multi-Table Write Graph](/en/guide/write-graph)
