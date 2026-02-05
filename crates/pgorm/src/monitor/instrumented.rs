use super::config::MonitorConfig;
use super::monitors::{CompositeHook, NoopMonitor};
use super::types::{HookAction, QueryContext, QueryHook, QueryMonitor, QueryResult, QueryType};
use crate::client::GenericClient;
use crate::error::{OrmError, OrmResult};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio_postgres::Row;
use tokio_postgres::types::ToSql;

/// An instrumented database client that wraps a `GenericClient` with monitoring.
///
/// Monitoring must be explicitly enabled via `MonitorConfig::enable_monitoring()`.
pub struct InstrumentedClient<C> {
    pub(super) client: C,
    pub(super) monitor: Arc<dyn QueryMonitor>,
    pub(super) hook: Option<Arc<dyn QueryHook>>,
    pub(super) config: MonitorConfig,
}

impl<C: GenericClient> InstrumentedClient<C> {
    /// Create a new instrumented client with no monitoring.
    pub fn new(client: C) -> Self {
        Self {
            client,
            monitor: Arc::new(NoopMonitor),
            hook: None,
            config: MonitorConfig::default(),
        }
    }

    /// Set the monitor configuration.
    pub fn with_config(mut self, config: MonitorConfig) -> Self {
        self.config = config;
        self
    }

    /// Set the query monitor.
    pub fn with_monitor<M: QueryMonitor + 'static>(mut self, monitor: M) -> Self {
        self.monitor = Arc::new(monitor);
        self
    }

    /// Set the query monitor from an Arc.
    pub fn with_monitor_arc(mut self, monitor: Arc<dyn QueryMonitor>) -> Self {
        self.monitor = monitor;
        self
    }

    /// Set a query hook.
    pub fn with_hook<H: QueryHook + 'static>(mut self, hook: H) -> Self {
        self.hook = Some(Arc::new(hook));
        self
    }

    /// Set a query hook from an Arc.
    pub fn with_hook_arc(mut self, hook: Arc<dyn QueryHook>) -> Self {
        self.hook = Some(hook);
        self
    }

    /// Add a query hook.
    ///
    /// If a hook is already set, this composes it with the new hook (existing first).
    pub fn add_hook<H: QueryHook + 'static>(self, hook: H) -> Self {
        self.add_hook_arc(Arc::new(hook))
    }

    /// Add a query hook from an `Arc`.
    ///
    /// If a hook is already set, this composes it with the new hook (existing first).
    pub fn add_hook_arc(mut self, hook: Arc<dyn QueryHook>) -> Self {
        self.hook = Some(match self.hook.take() {
            None => hook,
            Some(existing) => Arc::new(CompositeHook::new().add_arc(existing).add_arc(hook)),
        });
        self
    }

    /// Set the slow query threshold.
    ///
    /// Queries taking longer than this will trigger `on_slow_query`.
    #[deprecated(
        since = "0.2.0",
        note = "Use `with_config(MonitorConfig::new().with_slow_query_threshold(...))` instead"
    )]
    pub fn with_slow_query_threshold(mut self, threshold: Duration) -> Self {
        self.config.slow_query_threshold = Some(threshold);
        self
    }

    /// Set the query timeout.
    ///
    /// Queries exceeding this duration will be cancelled.
    pub fn with_query_timeout(mut self, timeout: Duration) -> Self {
        self.config.query_timeout = Some(timeout);
        self
    }

    /// Enable monitoring.
    pub fn enable_monitoring(mut self) -> Self {
        self.config.monitoring_enabled = true;
        self
    }

    /// Disable monitoring.
    pub fn disable_monitoring(mut self) -> Self {
        self.config.monitoring_enabled = false;
        self
    }

    /// Check if monitoring is enabled.
    pub fn is_monitoring_enabled(&self) -> bool {
        self.config.monitoring_enabled
    }

    /// Get the current configuration.
    pub fn config(&self) -> &MonitorConfig {
        &self.config
    }

    /// Get a mutable reference to the configuration.
    pub fn config_mut(&mut self) -> &mut MonitorConfig {
        &mut self.config
    }

    /// Get a reference to the inner client.
    pub fn inner(&self) -> &C {
        &self.client
    }

    /// Get the inner client, consuming this wrapper.
    pub fn into_inner(self) -> C {
        self.client
    }

    pub(super) fn apply_hook(&self, ctx: &mut QueryContext) -> Result<(), OrmError> {
        let Some(hook) = &self.hook else {
            return Ok(());
        };

        match hook.before_query(ctx) {
            HookAction::Continue => Ok(()),
            HookAction::ModifySql {
                exec_sql,
                canonical_sql,
            } => {
                ctx.exec_sql = exec_sql;
                if let Some(canonical_sql) = canonical_sql {
                    ctx.canonical_sql = canonical_sql;
                }
                ctx.query_type = QueryType::from_sql(&ctx.canonical_sql);
                Ok(())
            }
            HookAction::Abort(reason) => Err(OrmError::validation(format!(
                "Query aborted by hook: {reason}"
            ))),
        }
    }

    pub(super) fn report_result(
        &self,
        ctx: &QueryContext,
        duration: Duration,
        result: &QueryResult,
    ) {
        if !self.config.monitoring_enabled {
            return;
        }

        if let Some(hook) = &self.hook {
            hook.after_query(ctx, duration, result);
        }

        self.monitor.on_query_complete(ctx, duration, result);

        if let Some(threshold) = self.config.slow_query_threshold {
            if duration > threshold {
                self.monitor.on_slow_query(ctx, duration);
            }
        }
    }

    pub(super) async fn execute_with_timeout<T, F>(&self, future: F) -> OrmResult<T>
    where
        F: std::future::Future<Output = OrmResult<T>> + Send,
    {
        match self.config.query_timeout {
            Some(timeout) => {
                tokio::pin!(future);
                tokio::select! {
                    result = &mut future => result,
                    _ = tokio::time::sleep(timeout) => {
                        if let Some(cancel_token) = self.client.cancel_token() {
                            tokio::spawn(async move {
                                let _ = cancel_token.cancel_query(tokio_postgres::NoTls).await;
                            });
                        }
                        Err(OrmError::Timeout(timeout))
                    }
                }
            }
            None => future.await,
        }
    }

    pub(super) async fn query_inner(
        &self,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
        tag: Option<&str>,
    ) -> OrmResult<Vec<Row>> {
        let mut ctx = QueryContext::new(sql, params.len());
        if let Some(tag) = tag {
            ctx.tag = Some(tag.to_string());
        }

        self.apply_hook(&mut ctx)?;

        if self.config.monitoring_enabled {
            self.monitor.on_query_start(&ctx);
        }

        let start = Instant::now();
        let result = self
            .execute_with_timeout(self.client.query(&ctx.exec_sql, params))
            .await;
        let duration = start.elapsed();

        let query_result = match &result {
            Ok(rows) => QueryResult::Rows(rows.len()),
            Err(OrmError::Timeout(d)) => QueryResult::Error(format!("timeout after {d:?}")),
            Err(e) => QueryResult::Error(e.to_string()),
        };

        self.report_result(&ctx, duration, &query_result);
        result
    }

    pub(super) async fn query_one_inner(
        &self,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
        tag: Option<&str>,
    ) -> OrmResult<Row> {
        let mut ctx = QueryContext::new(sql, params.len());
        if let Some(tag) = tag {
            ctx.tag = Some(tag.to_string());
        }

        self.apply_hook(&mut ctx)?;

        if self.config.monitoring_enabled {
            self.monitor.on_query_start(&ctx);
        }

        let start = Instant::now();
        let result = self
            .execute_with_timeout(self.client.query_one(&ctx.exec_sql, params))
            .await;
        let duration = start.elapsed();

        let query_result = match &result {
            Ok(_) => QueryResult::OptionalRow(true),
            Err(OrmError::NotFound { .. }) => QueryResult::OptionalRow(false),
            Err(OrmError::Timeout(d)) => QueryResult::Error(format!("timeout after {d:?}")),
            Err(e) => QueryResult::Error(e.to_string()),
        };

        self.report_result(&ctx, duration, &query_result);
        result
    }

    pub(super) async fn query_opt_inner(
        &self,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
        tag: Option<&str>,
    ) -> OrmResult<Option<Row>> {
        let mut ctx = QueryContext::new(sql, params.len());
        if let Some(tag) = tag {
            ctx.tag = Some(tag.to_string());
        }

        self.apply_hook(&mut ctx)?;

        if self.config.monitoring_enabled {
            self.monitor.on_query_start(&ctx);
        }

        let start = Instant::now();
        let result = self
            .execute_with_timeout(self.client.query_opt(&ctx.exec_sql, params))
            .await;
        let duration = start.elapsed();

        let query_result = match &result {
            Ok(Some(_)) => QueryResult::OptionalRow(true),
            Ok(None) => QueryResult::OptionalRow(false),
            Err(OrmError::Timeout(d)) => QueryResult::Error(format!("timeout after {d:?}")),
            Err(e) => QueryResult::Error(e.to_string()),
        };

        self.report_result(&ctx, duration, &query_result);
        result
    }

    pub(super) async fn execute_inner(
        &self,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
        tag: Option<&str>,
    ) -> OrmResult<u64> {
        let mut ctx = QueryContext::new(sql, params.len());
        if let Some(tag) = tag {
            ctx.tag = Some(tag.to_string());
        }

        self.apply_hook(&mut ctx)?;

        if self.config.monitoring_enabled {
            self.monitor.on_query_start(&ctx);
        }

        let start = Instant::now();
        let result = self
            .execute_with_timeout(self.client.execute(&ctx.exec_sql, params))
            .await;
        let duration = start.elapsed();

        let query_result = match &result {
            Ok(n) => QueryResult::Affected(*n),
            Err(OrmError::Timeout(d)) => QueryResult::Error(format!("timeout after {d:?}")),
            Err(e) => QueryResult::Error(e.to_string()),
        };

        self.report_result(&ctx, duration, &query_result);
        result
    }
}

impl<C: GenericClient> GenericClient for InstrumentedClient<C> {
    async fn query(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> OrmResult<Vec<Row>> {
        self.query_inner(sql, params, None).await
    }

    async fn query_tagged(
        &self,
        tag: &str,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> OrmResult<Vec<Row>> {
        self.query_inner(sql, params, Some(tag)).await
    }

    async fn query_one(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> OrmResult<Row> {
        self.query_one_inner(sql, params, None).await
    }

    async fn query_one_tagged(
        &self,
        tag: &str,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> OrmResult<Row> {
        self.query_one_inner(sql, params, Some(tag)).await
    }

    async fn query_opt(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> OrmResult<Option<Row>> {
        self.query_opt_inner(sql, params, None).await
    }

    async fn query_opt_tagged(
        &self,
        tag: &str,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> OrmResult<Option<Row>> {
        self.query_opt_inner(sql, params, Some(tag)).await
    }

    async fn execute(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> OrmResult<u64> {
        self.execute_inner(sql, params, None).await
    }

    async fn execute_tagged(
        &self,
        tag: &str,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> OrmResult<u64> {
        self.execute_inner(sql, params, Some(tag)).await
    }

    fn cancel_token(&self) -> Option<tokio_postgres::CancelToken> {
        self.client.cancel_token()
    }
}
