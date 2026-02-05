//! Checked client with automatic schema registration and SQL validation.
//!
//! `CheckedClient` wraps any `GenericClient` and provides automatic SQL checking
//! against registered model schemas. Models are auto-registered via the `inventory`
//! crate when `#[derive(Model)]` is used.
//!
//! # Example
//!
//! ```ignore
//! use pgorm::check::CheckedClient;
//! use pgorm::prelude::*;
//!
//! #[derive(Debug, FromRow, Model)]
//! #[orm(table = "products")]
//! struct Product {
//!     #[orm(id)]
//!     id: i64,
//!     name: String,
//! }
//!
//! let pool = create_pool(&database_url)?;
//! let client = pool.get().await?;
//!
//! // Wrap with CheckedClient - models are auto-registered
//! let checked = CheckedClient::new(client);
//!
//! // SQL is automatically checked against registered schemas
//! let products = Product::select_all(&checked).await?;
//! ```

#[cfg(feature = "check")]
use crate::GenericClient;
#[cfg(feature = "check")]
use crate::check::{SchemaIssueLevel, SchemaRegistry};
#[cfg(feature = "check")]
use crate::error::{OrmError, OrmResult, pgorm_warn};
#[cfg(feature = "check")]
use crate::{RowStream, StreamingClient};
#[cfg(feature = "check")]
use std::sync::Arc;
#[cfg(feature = "check")]
use tokio_postgres::Row;
#[cfg(feature = "check")]
use tokio_postgres::types::ToSql;

/// Registration entry for auto-registering models.
///
/// This is used by the `#[derive(Model)]` macro to automatically register
/// models with the `CheckedClient`'s schema registry.
pub struct ModelRegistration {
    /// Function that registers a model type with a SchemaRegistry.
    pub register_fn: fn(&mut crate::check::SchemaRegistry),
}

inventory::collect!(ModelRegistration);

/// Check mode configuration for `CheckedClient`.
#[cfg(feature = "check")]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CheckMode {
    /// Disable all SQL checking.
    Disabled,
    /// Log warnings but don't block execution (default).
    #[default]
    WarnOnly,
    /// Strict mode: return error if SQL check fails.
    Strict,
}

/// Handle SQL check issues according to the check mode.
///
/// Shared implementation used by both `CheckedClient` and `PgClient`.
#[cfg(feature = "check")]
pub(crate) fn handle_check_issues(
    mode: CheckMode,
    issues: Vec<crate::check::SchemaIssue>,
    prefix: &str,
) -> OrmResult<()> {
    match mode {
        CheckMode::Disabled => Ok(()),
        CheckMode::WarnOnly => {
            for issue in &issues {
                pgorm_warn(&format!("[pgorm warn] {prefix}: {issue}"));
            }
            Ok(())
        }
        CheckMode::Strict => {
            let errors: Vec<_> = issues
                .iter()
                .filter(|i| i.level == SchemaIssueLevel::Error)
                .collect();
            if errors.is_empty() {
                Ok(())
            } else {
                let messages: Vec<String> = errors.iter().map(|i| i.message.clone()).collect();
                Err(OrmError::validation(format!(
                    "SQL check failed: {}",
                    messages.join("; ")
                )))
            }
        }
    }
}

/// A client wrapper that automatically checks SQL against registered schemas.
///
/// `CheckedClient` wraps any type that implements `GenericClient` and provides
/// automatic SQL validation. Models derived with `#[derive(Model)]` are
/// automatically registered via the `inventory` crate.
///
/// # Check Modes
///
/// - `CheckMode::Disabled` - No checking performed
/// - `CheckMode::WarnOnly` (default) - Prints warnings but allows execution
/// - `CheckMode::Strict` - Returns an error if validation fails
///
/// # Example
///
/// ```ignore
/// // Default: warn only
/// let checked = CheckedClient::new(client);
///
/// // Strict mode: errors block execution
/// let checked = CheckedClient::new(client).strict();
///
/// // Disable checking
/// let checked = CheckedClient::new(client).check_mode(CheckMode::Disabled);
/// ```
#[cfg(feature = "check")]
pub struct CheckedClient<C> {
    client: C,
    registry: Arc<SchemaRegistry>,
    check_mode: CheckMode,
}

#[cfg(feature = "check")]
impl<C> CheckedClient<C> {
    /// Create a new `CheckedClient` with auto-registered models.
    ///
    /// All models derived with `#[derive(Model)]` that are linked into the
    /// binary will be automatically registered with the schema registry.
    pub fn new(client: C) -> Self {
        let mut registry = SchemaRegistry::new();
        for reg in inventory::iter::<ModelRegistration> {
            (reg.register_fn)(&mut registry);
        }
        Self {
            client,
            registry: Arc::new(registry),
            check_mode: CheckMode::WarnOnly,
        }
    }

    /// Create a new `CheckedClient` without auto-registration.
    ///
    /// Use this if you want to manually register models or don't want
    /// automatic registration.
    pub fn new_empty(client: C) -> Self {
        Self {
            client,
            registry: Arc::new(SchemaRegistry::new()),
            check_mode: CheckMode::WarnOnly,
        }
    }

    /// Create a new `CheckedClient` with a provided registry.
    pub fn with_registry(client: C, registry: SchemaRegistry) -> Self {
        Self {
            client,
            registry: Arc::new(registry),
            check_mode: CheckMode::WarnOnly,
        }
    }

    /// Set the check mode.
    pub fn check_mode(mut self, mode: CheckMode) -> Self {
        self.check_mode = mode;
        self
    }

    /// Enable strict mode (errors block execution).
    pub fn strict(self) -> Self {
        self.check_mode(CheckMode::Strict)
    }

    /// Disable SQL checking.
    pub fn disabled(self) -> Self {
        self.check_mode(CheckMode::Disabled)
    }

    /// Get a reference to the schema registry.
    pub fn registry(&self) -> &SchemaRegistry {
        &self.registry
    }

    /// Get the inner client.
    pub fn inner(&self) -> &C {
        &self.client
    }

    /// Consume this wrapper and return the inner client.
    pub fn into_inner(self) -> C {
        self.client
    }

    /// Check SQL and handle according to check mode.
    /// Returns Ok(()) if check passes or mode allows continuation.
    /// Returns Err if in strict mode and check fails.
    fn check_sql(&self, sql: &str) -> OrmResult<()> {
        let issues = self.registry.check_sql(sql);
        handle_check_issues(self.check_mode, issues, "SQL check")
    }
}

#[cfg(feature = "check")]
impl<C: GenericClient> GenericClient for CheckedClient<C> {
    async fn query(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> OrmResult<Vec<Row>> {
        self.check_sql(sql)?;
        self.client.query(sql, params).await
    }

    async fn query_tagged(
        &self,
        tag: &str,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> OrmResult<Vec<Row>> {
        self.check_sql(sql)?;
        self.client.query_tagged(tag, sql, params).await
    }

    async fn query_one(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> OrmResult<Row> {
        self.check_sql(sql)?;
        self.client.query_one(sql, params).await
    }

    async fn query_one_tagged(
        &self,
        tag: &str,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> OrmResult<Row> {
        self.check_sql(sql)?;
        self.client.query_one_tagged(tag, sql, params).await
    }

    async fn query_opt(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> OrmResult<Option<Row>> {
        self.check_sql(sql)?;
        self.client.query_opt(sql, params).await
    }

    async fn query_opt_tagged(
        &self,
        tag: &str,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> OrmResult<Option<Row>> {
        self.check_sql(sql)?;
        self.client.query_opt_tagged(tag, sql, params).await
    }

    async fn execute(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> OrmResult<u64> {
        self.check_sql(sql)?;
        self.client.execute(sql, params).await
    }

    async fn execute_tagged(
        &self,
        tag: &str,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> OrmResult<u64> {
        self.check_sql(sql)?;
        self.client.execute_tagged(tag, sql, params).await
    }

    fn cancel_token(&self) -> Option<tokio_postgres::CancelToken> {
        self.client.cancel_token()
    }
}

#[cfg(feature = "check")]
impl<C: GenericClient + StreamingClient> StreamingClient for CheckedClient<C> {
    async fn query_stream(
        &self,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> OrmResult<RowStream> {
        self.check_sql(sql)?;
        self.client.query_stream(sql, params).await
    }

    async fn query_stream_tagged(
        &self,
        tag: &str,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> OrmResult<RowStream> {
        self.check_sql(sql)?;
        self.client.query_stream_tagged(tag, sql, params).await
    }
}

#[cfg(test)]
#[cfg(feature = "check")]
mod tests {
    use super::*;

    #[test]
    fn test_check_mode_default() {
        assert_eq!(CheckMode::default(), CheckMode::WarnOnly);
    }
}
