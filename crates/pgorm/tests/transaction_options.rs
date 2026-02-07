//! Compile-only checks for transaction option APIs.

#![allow(dead_code)]

use pgorm::{OrmError, OrmResult, TransactionIsolation, TransactionOptions, query};

async fn _transaction_with_options_macro_compiles(
    client: &mut tokio_postgres::Client,
) -> OrmResult<()> {
    let opts = TransactionOptions::new()
        .isolation_level(TransactionIsolation::ReadCommitted)
        .read_only(false);

    pgorm::transaction_with!(client, tx, opts, {
        query("SELECT 1").execute(&tx).await?;
        Ok::<(), OrmError>(())
    })?;

    Ok(())
}

async fn _begin_transaction_with_compiles(client: &mut tokio_postgres::Client) -> OrmResult<()> {
    let opts = TransactionOptions::new()
        .isolation_level(TransactionIsolation::Serializable)
        .read_only(true)
        .deferrable(true);
    let tx = pgorm::begin_transaction_with(client, opts).await?;
    tx.rollback().await.map_err(OrmError::from_db_error)?;
    Ok(())
}

#[cfg(feature = "pool")]
async fn _pool_begin_transaction_with_compiles(
    client: &mut deadpool_postgres::Client,
) -> OrmResult<()> {
    let opts = TransactionOptions::new()
        .isolation_level(TransactionIsolation::RepeatableRead)
        .read_only(false);
    let tx = pgorm::begin_transaction_with(client, opts).await?;
    tx.rollback().await.map_err(OrmError::from_db_error)?;
    Ok(())
}
