//! Transaction helpers.
//!
//! Prefer passing a transaction (`tokio_postgres::Transaction` or
//! `deadpool_postgres::Transaction`) into APIs that accept [`GenericClient`].
//! This keeps repository methods easy to compose with or without transactions.
//!
//! For ergonomic commit/rollback handling, use the [`transaction!`] macro.
//!
//! # Example
//!
//! ```ignore
//! use pgorm::{query, OrmResult};
//! use tokio_postgres::NoTls;
//!
//! # async fn demo() -> OrmResult<()> {
//! let (mut client, connection) = tokio_postgres::connect("postgres://...", NoTls).await?;
//! tokio::spawn(async move { let _ = connection.await; });
//!
//! pgorm::transaction!(&mut client, tx, {
//!     query("UPDATE accounts SET balance = balance - $1 WHERE id = $2")
//!         .bind(100_i64)
//!         .bind(1_i64)
//!         .execute(&tx)
//!         .await?;
//!     Ok(())
//! })?;
//! # Ok(()) }
//! ```

/// Runs the given block inside a database transaction.
///
/// - Begins a transaction via `$client.transaction().await`.
/// - Commits on `Ok(_)`.
/// - Rolls back on `Err(_)`.
///
/// The block must evaluate to `pgorm::OrmResult<T>`.
#[macro_export]
macro_rules! transaction {
    ($client:expr, $tx:ident, $body:block) => {{
        let mut $tx = ($client)
            .transaction()
            .await
            .map_err($crate::OrmError::from_db_error)?;

        let __pgorm_tx_body_result = async { $body }.await;
        match __pgorm_tx_body_result {
            Ok(value) => {
                $tx.commit()
                    .await
                    .map_err($crate::OrmError::from_db_error)?;
                Ok(value)
            }
            Err(error) => match $tx.rollback().await {
                Ok(()) => Err(error),
                Err(rollback_err) => Err($crate::OrmError::Other(format!(
                    "{error} (rollback failed: {rollback_err})"
                ))),
            },
        }
    }};
}
