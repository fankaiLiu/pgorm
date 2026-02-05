//! Transaction helpers: macros and Savepoint API.
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

use crate::error::{OrmError, OrmResult};
use std::sync::atomic::{AtomicU64, Ordering};
use tokio_postgres::Row;
use tokio_postgres::Statement;
use tokio_postgres::types::ToSql;

/// Global counter for anonymous savepoint naming.
static SAVEPOINT_COUNTER: AtomicU64 = AtomicU64::new(0);

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

/// Runs the given block inside a savepoint within an existing transaction.
///
/// - Creates a savepoint on `$tx`.
/// - Releases (commits) on `Ok(_)`.
/// - Rolls back to savepoint on `Err(_)`.
///
/// The block must evaluate to `pgorm::OrmResult<T>`.
///
/// # Example
///
/// ```ignore
/// pgorm::transaction!(&mut client, tx, {
///     // main operation
///     let order = create_order(&tx, &data).await?;
///
///     // savepoint: notification failure won't affect order
///     let notify_result = pgorm::savepoint!(tx, "notify", sp, {
///         send_notification(&sp, order.id).await?;
///         Ok(())
///     });
///
///     if let Err(e) = notify_result {
///         log::warn!("Notification failed: {}", e);
///     }
///
///     Ok(order)
/// })?;
/// ```
#[macro_export]
macro_rules! savepoint {
    // Named savepoint
    ($tx:expr, $name:expr, $sp:ident, $body:block) => {{
        let mut $sp = ($tx)
            .savepoint($name)
            .await
            .map_err($crate::OrmError::from_db_error)?;

        let __pgorm_sp_body_result = async { $body }.await;
        match __pgorm_sp_body_result {
            Ok(value) => {
                $sp.commit()
                    .await
                    .map_err($crate::OrmError::from_db_error)?;
                Ok(value)
            }
            Err(error) => match $sp.rollback().await {
                Ok(()) => Err(error),
                Err(rollback_err) => Err($crate::OrmError::Other(format!(
                    "{error} (savepoint rollback failed: {rollback_err})"
                ))),
            },
        }
    }};
    // Anonymous savepoint
    ($tx:expr, $sp:ident, $body:block) => {{
        let __pgorm_sp_name = $crate::__next_savepoint_name();
        $crate::savepoint!($tx, &__pgorm_sp_name, $sp, $body)
    }};
}

/// Runs the given block inside a nested transaction (savepoint).
///
/// Use this when you want to create a sub-transaction within an existing transaction.
/// The inner block gets its own savepoint that can be rolled back without
/// affecting the outer transaction.
///
/// # Example
///
/// ```ignore
/// pgorm::transaction!(&mut client, tx, {
///     create_user(&tx, &user_data).await?;
///
///     // Inner savepoint — failure here won't roll back create_user
///     pgorm::nested_transaction!(tx, inner, {
///         create_user_profile(&inner, &profile_data).await?;
///         create_user_settings(&inner, &settings_data).await?;
///         Ok(())
///     })?;
///
///     Ok(())
/// })?;
/// ```
#[macro_export]
macro_rules! nested_transaction {
    ($tx:expr, $inner:ident, $body:block) => {{
        let __pgorm_sp_name = $crate::__next_savepoint_name();
        $crate::savepoint!($tx, &__pgorm_sp_name, $inner, $body)
    }};
}

/// Generate a unique anonymous savepoint name.
///
/// This is a public helper used by the `savepoint!` and `nested_transaction!` macros.
/// Not intended for direct use.
#[doc(hidden)]
pub fn __next_savepoint_name() -> String {
    let n = SAVEPOINT_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("pgorm_sp_{n}")
}

// ─── Savepoint wrapper ──────────────────────────────────────────────────────

/// A named savepoint within a transaction.
///
/// Wraps a nested `tokio_postgres::Transaction` created via `savepoint()`.
/// Provides explicit `release()` and `rollback()` methods, and implements
/// [`GenericClient`](crate::GenericClient) for query execution within the
/// savepoint scope.
///
/// # Example
///
/// ```ignore
/// use pgorm::TransactionExt;
///
/// pgorm::transaction!(&mut client, tx, {
///     let order = NewOrder { user_id: 1, total: 100 }.insert_returning(&tx).await?;
///
///     let mut sp = tx.pgorm_savepoint("before_items").await?;
///
///     match insert_order_items(&sp, order.id, &items).await {
///         Ok(_) => sp.release().await?,
///         Err(e) => {
///             sp.rollback().await?;
///             log::warn!("Failed to insert items: {}", e);
///         }
///     }
///
///     Ok(())
/// })?;
/// ```
pub struct Savepoint<'a> {
    inner: Option<tokio_postgres::Transaction<'a>>,
    name: String,
}

impl<'a> Savepoint<'a> {
    fn new(inner: tokio_postgres::Transaction<'a>, name: String) -> Self {
        Self {
            inner: Some(inner),
            name,
        }
    }

    /// Returns the savepoint name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Release the savepoint (make changes permanent within the transaction).
    ///
    /// Equivalent to `RELEASE SAVEPOINT name`.
    pub async fn release(mut self) -> OrmResult<()> {
        if let Some(tx) = self.inner.take() {
            tx.commit().await.map_err(OrmError::from_db_error)?;
        }
        Ok(())
    }

    /// Roll back to this savepoint (undo changes made since the savepoint).
    ///
    /// Equivalent to `ROLLBACK TO SAVEPOINT name`.
    pub async fn rollback(mut self) -> OrmResult<()> {
        if let Some(tx) = self.inner.take() {
            tx.rollback().await.map_err(OrmError::from_db_error)?;
        }
        Ok(())
    }
}

impl Drop for Savepoint<'_> {
    fn drop(&mut self) {
        if self.inner.is_some() {
            // tokio_postgres::Transaction::drop already handles rollback
            // when dropped without commit. We just log a warning.
            #[cfg(feature = "tracing")]
            tracing::warn!(
                "Savepoint '{}' dropped without explicit release or rollback",
                self.name,
            );
        }
    }
}

// GenericClient delegation for Savepoint
impl crate::GenericClient for Savepoint<'_> {
    async fn query(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> OrmResult<Vec<Row>> {
        let tx = self.inner.as_ref().ok_or_else(|| {
            OrmError::Other("savepoint already consumed".to_string())
        })?;
        crate::GenericClient::query(tx, sql, params).await
    }

    async fn query_one(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> OrmResult<Row> {
        let tx = self.inner.as_ref().ok_or_else(|| {
            OrmError::Other("savepoint already consumed".to_string())
        })?;
        crate::GenericClient::query_one(tx, sql, params).await
    }

    async fn query_opt(
        &self,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> OrmResult<Option<Row>> {
        let tx = self.inner.as_ref().ok_or_else(|| {
            OrmError::Other("savepoint already consumed".to_string())
        })?;
        crate::GenericClient::query_opt(tx, sql, params).await
    }

    async fn execute(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> OrmResult<u64> {
        let tx = self.inner.as_ref().ok_or_else(|| {
            OrmError::Other("savepoint already consumed".to_string())
        })?;
        crate::GenericClient::execute(tx, sql, params).await
    }

    fn cancel_token(&self) -> Option<tokio_postgres::CancelToken> {
        self.inner.as_ref().and_then(|tx| crate::GenericClient::cancel_token(tx))
    }

    fn supports_prepared_statements(&self) -> bool {
        self.inner
            .as_ref()
            .map_or(false, |tx| crate::GenericClient::supports_prepared_statements(tx))
    }

    async fn prepare_statement(&self, sql: &str) -> OrmResult<Statement> {
        let tx = self.inner.as_ref().ok_or_else(|| {
            OrmError::Other("savepoint already consumed".to_string())
        })?;
        crate::GenericClient::prepare_statement(tx, sql).await
    }

    async fn query_prepared(
        &self,
        stmt: &Statement,
        params: &[&(dyn ToSql + Sync)],
    ) -> OrmResult<Vec<Row>> {
        let tx = self.inner.as_ref().ok_or_else(|| {
            OrmError::Other("savepoint already consumed".to_string())
        })?;
        crate::GenericClient::query_prepared(tx, stmt, params).await
    }

    async fn execute_prepared(
        &self,
        stmt: &Statement,
        params: &[&(dyn ToSql + Sync)],
    ) -> OrmResult<u64> {
        let tx = self.inner.as_ref().ok_or_else(|| {
            OrmError::Other("savepoint already consumed".to_string())
        })?;
        crate::GenericClient::execute_prepared(tx, stmt, params).await
    }
}

// ─── TransactionExt ─────────────────────────────────────────────────────────

/// Extension trait adding savepoint support to transactions.
///
/// # Example
///
/// ```ignore
/// use pgorm::TransactionExt;
///
/// pgorm::transaction!(&mut client, tx, {
///     let sp = tx.pgorm_savepoint("before_risky_op").await?;
///     // ... do work ...
///     sp.release().await?;
///     Ok(())
/// })?;
/// ```
pub trait TransactionExt {
    /// Create a named savepoint within this transaction.
    fn pgorm_savepoint(
        &mut self,
        name: &str,
    ) -> impl std::future::Future<Output = OrmResult<Savepoint<'_>>> + Send;

    /// Create an anonymous savepoint (auto-numbered) within this transaction.
    fn pgorm_savepoint_anon(
        &mut self,
    ) -> impl std::future::Future<Output = OrmResult<Savepoint<'_>>> + Send;
}

impl TransactionExt for tokio_postgres::Transaction<'_> {
    async fn pgorm_savepoint(&mut self, name: &str) -> OrmResult<Savepoint<'_>> {
        let inner = self
            .savepoint(name)
            .await
            .map_err(OrmError::from_db_error)?;
        Ok(Savepoint::new(inner, name.to_string()))
    }

    async fn pgorm_savepoint_anon(&mut self) -> OrmResult<Savepoint<'_>> {
        let name = __next_savepoint_name();
        let inner = self
            .savepoint(&name)
            .await
            .map_err(OrmError::from_db_error)?;
        Ok(Savepoint::new(inner, name))
    }
}

#[cfg(feature = "pool")]
impl TransactionExt for deadpool_postgres::Transaction<'_> {
    async fn pgorm_savepoint(&mut self, name: &str) -> OrmResult<Savepoint<'_>> {
        // Access the inner tokio_postgres::Transaction via DerefMut to get a
        // tokio_postgres::Transaction savepoint (not the deadpool wrapper).
        let inner_tx: &mut tokio_postgres::Transaction<'_> =
            std::ops::DerefMut::deref_mut(self);
        let inner = inner_tx
            .savepoint(name)
            .await
            .map_err(OrmError::from_db_error)?;
        Ok(Savepoint::new(inner, name.to_string()))
    }

    async fn pgorm_savepoint_anon(&mut self) -> OrmResult<Savepoint<'_>> {
        let name = __next_savepoint_name();
        let inner_tx: &mut tokio_postgres::Transaction<'_> =
            std::ops::DerefMut::deref_mut(self);
        let inner = inner_tx
            .savepoint(&name)
            .await
            .map_err(OrmError::from_db_error)?;
        Ok(Savepoint::new(inner, name))
    }
}
