use super::config::{DangerousDmlPolicy, SelectWithoutLimitPolicy, handle_dangerous_dml};
use super::statement_cache::{StmtCacheProbe, is_retryable_prepared_error};
use crate::GenericClient;
use crate::error::{OrmError, OrmResult};
use crate::monitor::{HookAction, QueryContext, QueryMonitor, QueryResult, QueryType};
use crate::row::FromRow;
use std::time::{Duration, Instant};
use tokio_postgres::Row;
use tokio_postgres::types::ToSql;

// ============================================================================
// Internal helpers
// ============================================================================

impl<C: GenericClient> super::PgClient<C> {
    #[cfg(not(feature = "tracing"))]
    pub(super) fn emit_tracing_sql(&self, _ctx: &QueryContext) {}

    #[cfg(feature = "tracing")]
    pub(super) fn emit_tracing_sql(&self, ctx: &QueryContext) {
        if let Some(hook) = &self.tracing_sql_hook {
            let _ = hook.before_query(ctx);
        }
    }

    pub(super) fn apply_sql_policy(&self, ctx: &mut QueryContext) -> OrmResult<()> {
        use crate::check::StatementKind;

        let policy = &self.config.sql_policy;
        // Fast path: default policy is "Allow" everywhere, so avoid parsing/analyzing SQL.
        if policy.select_without_limit == SelectWithoutLimitPolicy::Allow
            && policy.delete_without_where == DangerousDmlPolicy::Allow
            && policy.update_without_where == DangerousDmlPolicy::Allow
            && policy.truncate == DangerousDmlPolicy::Allow
            && policy.drop_table == DangerousDmlPolicy::Allow
        {
            return Ok(());
        }

        let analysis = self.registry.analyze_sql(&ctx.canonical_sql);

        if !analysis.parse_result.valid {
            // Leave parse errors to schema checks or database errors depending on configuration.
            return Ok(());
        }

        match analysis.statement_kind {
            Some(StatementKind::Select) => {
                if analysis.select_has_limit == Some(false) {
                    match policy.select_without_limit {
                        SelectWithoutLimitPolicy::Allow => {}
                        SelectWithoutLimitPolicy::Warn => {
                            crate::error::pgorm_warn(&format!(
                                "[pgorm warn] SQL policy: SELECT without LIMIT/OFFSET: {}",
                                ctx.canonical_sql
                            ));
                        }
                        SelectWithoutLimitPolicy::Error => {
                            return Err(OrmError::validation(format!(
                                "SQL policy violation: SELECT without LIMIT/OFFSET: {}",
                                ctx.canonical_sql
                            )));
                        }
                        SelectWithoutLimitPolicy::AutoLimit(limit) => {
                            let old_canonical = ctx.canonical_sql.clone();
                            match pgorm_check::ensure_select_limit(&old_canonical, limit) {
                                Ok(Some(new_sql)) => {
                                    ctx.canonical_sql = new_sql.clone();
                                    ctx.query_type = QueryType::from_sql(&ctx.canonical_sql);

                                    if ctx.exec_sql == old_canonical {
                                        ctx.exec_sql = new_sql;
                                    } else if let Some(pos) = ctx.exec_sql.rfind(&old_canonical) {
                                        let mut rewritten = String::with_capacity(
                                            ctx.exec_sql.len() - old_canonical.len()
                                                + ctx.canonical_sql.len(),
                                        );
                                        rewritten.push_str(&ctx.exec_sql[..pos]);
                                        rewritten.push_str(&ctx.canonical_sql);
                                        rewritten
                                            .push_str(&ctx.exec_sql[pos + old_canonical.len()..]);
                                        ctx.exec_sql = rewritten;
                                    } else {
                                        // Fallback: drop exec_sql modifications (e.g. comments) to ensure LIMIT is applied.
                                        ctx.exec_sql = ctx.canonical_sql.clone();
                                    }
                                }
                                Ok(None) => {
                                    // Shouldn't happen if analysis says no limit; treat as unsupported rewrite.
                                    return Err(OrmError::validation(format!(
                                        "SQL policy rewrite failed: unable to add LIMIT to: {}",
                                        ctx.canonical_sql
                                    )));
                                }
                                Err(e) => return Err(OrmError::validation(e.to_string())),
                            }
                        }
                    }
                }
            }
            Some(StatementKind::Delete) => {
                if analysis.delete_has_where == Some(false) {
                    handle_dangerous_dml(
                        policy.delete_without_where,
                        "DELETE without WHERE",
                        &ctx.canonical_sql,
                    )?;
                }
            }
            Some(StatementKind::Update) => {
                if analysis.update_has_where == Some(false) {
                    handle_dangerous_dml(
                        policy.update_without_where,
                        "UPDATE without WHERE",
                        &ctx.canonical_sql,
                    )?;
                }
            }
            Some(StatementKind::Truncate) => {
                handle_dangerous_dml(policy.truncate, "TRUNCATE", &ctx.canonical_sql)?;
            }
            Some(StatementKind::DropTable) => {
                handle_dangerous_dml(policy.drop_table, "DROP TABLE", &ctx.canonical_sql)?;
            }
            _ => {}
        }

        Ok(())
    }

    /// Check SQL against the registry.
    pub(super) fn check_sql(&self, sql: &str) -> OrmResult<()> {
        let issues = self.registry.check_sql(sql);
        crate::checked_client::handle_check_issues(self.config.check_mode, issues, "SQL check")
    }

    /// Process hook before query.
    pub(super) fn apply_hook(&self, ctx: &mut QueryContext) -> Result<(), OrmError> {
        if let Some(hook) = &self.hook {
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
        } else {
            Ok(())
        }
    }

    /// Report query result to monitors.
    pub(super) fn report_result(
        &self,
        ctx: &QueryContext,
        duration: Duration,
        result: &QueryResult,
    ) {
        // Always report to stats monitor if enabled
        if self.config.stats_enabled {
            self.stats.on_query_complete(ctx, duration, result);
        }

        // Report to logging monitor if enabled
        if let Some(ref logging) = self.logging_monitor {
            logging.on_query_complete(ctx, duration, result);
        }

        // Report to custom monitor if set
        if let Some(ref monitor) = self.custom_monitor {
            monitor.on_query_complete(ctx, duration, result);
        }

        // Check slow query threshold
        if let Some(threshold) = self.config.slow_query_threshold {
            if duration > threshold {
                if let Some(ref logging) = self.logging_monitor {
                    logging.on_slow_query(ctx, duration);
                }
                if let Some(ref monitor) = self.custom_monitor {
                    monitor.on_slow_query(ctx, duration);
                }
            }
        }

        // Hook after query
        if let Some(ref hook) = self.hook {
            hook.after_query(ctx, duration, result);
        }
    }

    /// Execute with timeout if configured.
    pub(super) async fn execute_with_timeout<T, F>(&self, future: F) -> OrmResult<T>
    where
        F: std::future::Future<Output = OrmResult<T>> + Send,
    {
        match self.config.query_timeout {
            Some(timeout) => tokio::time::timeout(timeout, future).await.map_err(|_| {
                if let Some(cancel_token) = self.client.cancel_token() {
                    tokio::spawn(async move {
                        let _ = cancel_token.cancel_query(tokio_postgres::NoTls).await;
                    });
                }
                OrmError::Timeout(timeout)
            })?,
            None => future.await,
        }
    }

    pub(super) fn probe_stmt_cache(&self, ctx: &QueryContext) -> StmtCacheProbe {
        if !self.config.statement_cache.enabled {
            return StmtCacheProbe::Disabled;
        }
        let Some(cache) = &self.statement_cache else {
            return StmtCacheProbe::Disabled;
        };
        if !self.client.supports_prepared_statements() {
            return StmtCacheProbe::Disabled;
        }
        // Only use canonical_sql as cache key when it matches the executed SQL.
        if ctx.exec_sql != ctx.canonical_sql {
            return StmtCacheProbe::Disabled;
        }

        match cache.get(&ctx.canonical_sql) {
            Some(stmt) => StmtCacheProbe::Hit(stmt),
            None => StmtCacheProbe::Miss,
        }
    }
}

// ============================================================================
// Dynamic SQL execution methods
// ============================================================================

impl<C: GenericClient> super::PgClient<C> {
    /// Execute a dynamic SQL query and return all rows mapped to type T.
    ///
    /// This method is monitored and uses the same configuration as the `PgClient`.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let users: Vec<User> = pg.sql_query_as(
    ///     "SELECT * FROM users WHERE status = $1",
    ///     &[&"active"]
    /// ).await?;
    /// ```
    pub async fn sql_query_as<T: FromRow>(
        &self,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> OrmResult<Vec<T>> {
        let rows = self.query(sql, params).await?;
        rows.iter().map(T::from_row).collect()
    }

    /// Execute a dynamic SQL query and return exactly one row mapped to type T.
    ///
    /// Returns an error if zero or more than one row is returned.
    pub async fn sql_query_one_as<T: FromRow>(
        &self,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> OrmResult<T> {
        let row = self.query_one(sql, params).await?;
        T::from_row(&row)
    }

    /// Execute a dynamic SQL query and return at most one row mapped to type T.
    ///
    /// Returns `Ok(None)` if no rows are found.
    pub async fn sql_query_opt_as<T: FromRow>(
        &self,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> OrmResult<Option<T>> {
        let row = self.query_opt(sql, params).await?;
        row.as_ref().map(T::from_row).transpose()
    }

    /// Execute a dynamic SQL statement and return the number of affected rows.
    ///
    /// Use this for INSERT, UPDATE, DELETE statements.
    pub async fn sql_execute(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> OrmResult<u64> {
        self.execute(sql, params).await
    }

    /// Execute a dynamic SQL query and return all raw rows.
    pub async fn sql_query(
        &self,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> OrmResult<Vec<Row>> {
        self.query(sql, params).await
    }

    /// Execute a dynamic SQL query and return exactly one raw row.
    pub async fn sql_query_one(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> OrmResult<Row> {
        self.query_one(sql, params).await
    }

    /// Execute a dynamic SQL query and return at most one raw row.
    pub async fn sql_query_opt(
        &self,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> OrmResult<Option<Row>> {
        self.query_opt(sql, params).await
    }
}

// ============================================================================
// GenericClient implementation
// ============================================================================

impl<C: GenericClient> GenericClient for super::PgClient<C> {
    async fn query(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> OrmResult<Vec<Row>> {
        self.query_impl(None, sql, params).await
    }

    async fn query_tagged(
        &self,
        tag: &str,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> OrmResult<Vec<Row>> {
        self.query_impl(Some(tag), sql, params).await
    }

    async fn query_one(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> OrmResult<Row> {
        self.query_one_impl(None, sql, params).await
    }

    async fn query_one_tagged(
        &self,
        tag: &str,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> OrmResult<Row> {
        self.query_one_impl(Some(tag), sql, params).await
    }

    async fn query_opt(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> OrmResult<Option<Row>> {
        self.query_opt_impl(None, sql, params).await
    }

    async fn query_opt_tagged(
        &self,
        tag: &str,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> OrmResult<Option<Row>> {
        self.query_opt_impl(Some(tag), sql, params).await
    }

    async fn execute(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> OrmResult<u64> {
        self.execute_impl(None, sql, params).await
    }

    async fn execute_tagged(
        &self,
        tag: &str,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> OrmResult<u64> {
        self.execute_impl(Some(tag), sql, params).await
    }

    fn cancel_token(&self) -> Option<tokio_postgres::CancelToken> {
        self.client.cancel_token()
    }
}

// ============================================================================
// Core query implementations
// ============================================================================

impl<C: GenericClient> super::PgClient<C> {
    pub(super) async fn query_impl(
        &self,
        tag: Option<&str>,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> OrmResult<Vec<Row>> {
        let mut ctx = QueryContext::new(sql, params.len());
        if let Some(tag) = tag {
            ctx.tag = Some(tag.to_string());
        }

        // Process hook first, then check the canonical SQL.
        self.apply_hook(&mut ctx)?;
        self.apply_sql_policy(&mut ctx)?;
        self.check_sql(&ctx.canonical_sql)?;

        let probe = self.probe_stmt_cache(&ctx);
        match &probe {
            StmtCacheProbe::Disabled => {
                ctx.fields
                    .insert("stmt_cache".to_string(), "disabled".to_string());
                ctx.fields
                    .insert("prepared".to_string(), "false".to_string());
            }
            StmtCacheProbe::Hit(_) => {
                ctx.fields
                    .insert("stmt_cache".to_string(), "hit".to_string());
                ctx.fields
                    .insert("prepared".to_string(), "true".to_string());
            }
            StmtCacheProbe::Miss => {
                ctx.fields
                    .insert("stmt_cache".to_string(), "miss".to_string());
                ctx.fields
                    .insert("prepared".to_string(), "true".to_string());
            }
        }
        self.emit_tracing_sql(&ctx);

        let start = Instant::now();
        let result = match probe {
            StmtCacheProbe::Disabled => {
                self.execute_with_timeout(self.client.query(&ctx.exec_sql, params))
                    .await
            }
            StmtCacheProbe::Hit(stmt) => {
                if self.config.stats_enabled {
                    self.stats.on_stmt_cache_hit();
                }

                let mut result = self
                    .execute_with_timeout(self.client.query_prepared(&stmt, params))
                    .await;

                if let Err(ref err) = result {
                    if is_retryable_prepared_error(err) {
                        if let Some(cache) = &self.statement_cache {
                            let _ = cache.remove(&ctx.canonical_sql);
                        }

                        if let Some(cache) = &self.statement_cache {
                            let prep_start = Instant::now();
                            let stmt = self
                                .execute_with_timeout(
                                    self.client.prepare_statement(&ctx.canonical_sql),
                                )
                                .await;
                            let prep_dur = prep_start.elapsed();
                            if self.config.stats_enabled {
                                self.stats.on_stmt_prepare(prep_dur);
                            }

                            let stmt = cache.insert_if_absent(ctx.canonical_sql.clone(), stmt?);
                            result = self
                                .execute_with_timeout(self.client.query_prepared(&stmt, params))
                                .await;
                        }
                    }
                }

                result
            }
            StmtCacheProbe::Miss => {
                if self.config.stats_enabled {
                    self.stats.on_stmt_cache_miss();
                }

                match &self.statement_cache {
                    Some(cache) => {
                        let prep_start = Instant::now();
                        let stmt = self
                            .execute_with_timeout(self.client.prepare_statement(&ctx.canonical_sql))
                            .await;
                        let prep_dur = prep_start.elapsed();
                        if self.config.stats_enabled {
                            self.stats.on_stmt_prepare(prep_dur);
                        }

                        let stmt = cache.insert_if_absent(ctx.canonical_sql.clone(), stmt?);
                        self.execute_with_timeout(self.client.query_prepared(&stmt, params))
                            .await
                    }
                    None => {
                        self.execute_with_timeout(self.client.query(&ctx.exec_sql, params))
                            .await
                    }
                }
            }
        };
        let duration = start.elapsed();

        // Report
        let query_result = match &result {
            Ok(rows) => QueryResult::Rows(rows.len()),
            Err(OrmError::Timeout(d)) => QueryResult::Error(format!("timeout after {d:?}")),
            Err(e) => QueryResult::Error(e.to_string()),
        };
        self.report_result(&ctx, duration, &query_result);

        result
    }

    pub(super) async fn query_one_impl(
        &self,
        tag: Option<&str>,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> OrmResult<Row> {
        let mut ctx = QueryContext::new(sql, params.len());
        if let Some(tag) = tag {
            ctx.tag = Some(tag.to_string());
        }
        self.apply_hook(&mut ctx)?;
        self.apply_sql_policy(&mut ctx)?;
        self.check_sql(&ctx.canonical_sql)?;
        let probe = self.probe_stmt_cache(&ctx);
        match &probe {
            StmtCacheProbe::Disabled => {
                ctx.fields
                    .insert("stmt_cache".to_string(), "disabled".to_string());
                ctx.fields
                    .insert("prepared".to_string(), "false".to_string());
            }
            StmtCacheProbe::Hit(_) => {
                ctx.fields
                    .insert("stmt_cache".to_string(), "hit".to_string());
                ctx.fields
                    .insert("prepared".to_string(), "true".to_string());
            }
            StmtCacheProbe::Miss => {
                ctx.fields
                    .insert("stmt_cache".to_string(), "miss".to_string());
                ctx.fields
                    .insert("prepared".to_string(), "true".to_string());
            }
        }
        self.emit_tracing_sql(&ctx);

        let start = Instant::now();
        let result = match probe {
            StmtCacheProbe::Disabled => {
                self.execute_with_timeout(self.client.query_one(&ctx.exec_sql, params))
                    .await
            }
            StmtCacheProbe::Hit(stmt) => {
                if self.config.stats_enabled {
                    self.stats.on_stmt_cache_hit();
                }

                let mut result = self
                    .execute_with_timeout(self.client.query_one_prepared(&stmt, params))
                    .await;

                if let Err(ref err) = result {
                    if is_retryable_prepared_error(err) {
                        if let Some(cache) = &self.statement_cache {
                            let _ = cache.remove(&ctx.canonical_sql);
                        }

                        if let Some(cache) = &self.statement_cache {
                            let prep_start = Instant::now();
                            let stmt = self
                                .execute_with_timeout(
                                    self.client.prepare_statement(&ctx.canonical_sql),
                                )
                                .await;
                            let prep_dur = prep_start.elapsed();
                            if self.config.stats_enabled {
                                self.stats.on_stmt_prepare(prep_dur);
                            }

                            let stmt = cache.insert_if_absent(ctx.canonical_sql.clone(), stmt?);
                            result = self
                                .execute_with_timeout(self.client.query_one_prepared(&stmt, params))
                                .await;
                        }
                    }
                }

                result
            }
            StmtCacheProbe::Miss => {
                if self.config.stats_enabled {
                    self.stats.on_stmt_cache_miss();
                }

                match &self.statement_cache {
                    Some(cache) => {
                        let prep_start = Instant::now();
                        let stmt = self
                            .execute_with_timeout(self.client.prepare_statement(&ctx.canonical_sql))
                            .await;
                        let prep_dur = prep_start.elapsed();
                        if self.config.stats_enabled {
                            self.stats.on_stmt_prepare(prep_dur);
                        }

                        let stmt = cache.insert_if_absent(ctx.canonical_sql.clone(), stmt?);
                        self.execute_with_timeout(self.client.query_one_prepared(&stmt, params))
                            .await
                    }
                    None => {
                        self.execute_with_timeout(self.client.query_one(&ctx.exec_sql, params))
                            .await
                    }
                }
            }
        };
        let duration = start.elapsed();

        let query_result = match &result {
            Ok(_) => QueryResult::OptionalRow(true),
            Err(OrmError::NotFound(_)) => QueryResult::OptionalRow(false),
            Err(OrmError::Timeout(d)) => QueryResult::Error(format!("timeout after {d:?}")),
            Err(e) => QueryResult::Error(e.to_string()),
        };
        self.report_result(&ctx, duration, &query_result);

        result
    }

    pub(super) async fn query_opt_impl(
        &self,
        tag: Option<&str>,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> OrmResult<Option<Row>> {
        let mut ctx = QueryContext::new(sql, params.len());
        if let Some(tag) = tag {
            ctx.tag = Some(tag.to_string());
        }
        self.apply_hook(&mut ctx)?;
        self.apply_sql_policy(&mut ctx)?;
        self.check_sql(&ctx.canonical_sql)?;
        let probe = self.probe_stmt_cache(&ctx);
        match &probe {
            StmtCacheProbe::Disabled => {
                ctx.fields
                    .insert("stmt_cache".to_string(), "disabled".to_string());
                ctx.fields
                    .insert("prepared".to_string(), "false".to_string());
            }
            StmtCacheProbe::Hit(_) => {
                ctx.fields
                    .insert("stmt_cache".to_string(), "hit".to_string());
                ctx.fields
                    .insert("prepared".to_string(), "true".to_string());
            }
            StmtCacheProbe::Miss => {
                ctx.fields
                    .insert("stmt_cache".to_string(), "miss".to_string());
                ctx.fields
                    .insert("prepared".to_string(), "true".to_string());
            }
        }
        self.emit_tracing_sql(&ctx);

        let start = Instant::now();
        let result = match probe {
            StmtCacheProbe::Disabled => {
                self.execute_with_timeout(self.client.query_opt(&ctx.exec_sql, params))
                    .await
            }
            StmtCacheProbe::Hit(stmt) => {
                if self.config.stats_enabled {
                    self.stats.on_stmt_cache_hit();
                }

                let mut result = self
                    .execute_with_timeout(self.client.query_opt_prepared(&stmt, params))
                    .await;

                if let Err(ref err) = result {
                    if is_retryable_prepared_error(err) {
                        if let Some(cache) = &self.statement_cache {
                            let _ = cache.remove(&ctx.canonical_sql);
                        }

                        if let Some(cache) = &self.statement_cache {
                            let prep_start = Instant::now();
                            let stmt = self
                                .execute_with_timeout(
                                    self.client.prepare_statement(&ctx.canonical_sql),
                                )
                                .await;
                            let prep_dur = prep_start.elapsed();
                            if self.config.stats_enabled {
                                self.stats.on_stmt_prepare(prep_dur);
                            }

                            let stmt = cache.insert_if_absent(ctx.canonical_sql.clone(), stmt?);
                            result = self
                                .execute_with_timeout(self.client.query_opt_prepared(&stmt, params))
                                .await;
                        }
                    }
                }

                result
            }
            StmtCacheProbe::Miss => {
                if self.config.stats_enabled {
                    self.stats.on_stmt_cache_miss();
                }

                match &self.statement_cache {
                    Some(cache) => {
                        let prep_start = Instant::now();
                        let stmt = self
                            .execute_with_timeout(self.client.prepare_statement(&ctx.canonical_sql))
                            .await;
                        let prep_dur = prep_start.elapsed();
                        if self.config.stats_enabled {
                            self.stats.on_stmt_prepare(prep_dur);
                        }

                        let stmt = cache.insert_if_absent(ctx.canonical_sql.clone(), stmt?);
                        self.execute_with_timeout(self.client.query_opt_prepared(&stmt, params))
                            .await
                    }
                    None => {
                        self.execute_with_timeout(self.client.query_opt(&ctx.exec_sql, params))
                            .await
                    }
                }
            }
        };
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

    pub(super) async fn execute_impl(
        &self,
        tag: Option<&str>,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> OrmResult<u64> {
        let mut ctx = QueryContext::new(sql, params.len());
        if let Some(tag) = tag {
            ctx.tag = Some(tag.to_string());
        }
        self.apply_hook(&mut ctx)?;
        self.apply_sql_policy(&mut ctx)?;
        self.check_sql(&ctx.canonical_sql)?;
        let probe = self.probe_stmt_cache(&ctx);
        match &probe {
            StmtCacheProbe::Disabled => {
                ctx.fields
                    .insert("stmt_cache".to_string(), "disabled".to_string());
                ctx.fields
                    .insert("prepared".to_string(), "false".to_string());
            }
            StmtCacheProbe::Hit(_) => {
                ctx.fields
                    .insert("stmt_cache".to_string(), "hit".to_string());
                ctx.fields
                    .insert("prepared".to_string(), "true".to_string());
            }
            StmtCacheProbe::Miss => {
                ctx.fields
                    .insert("stmt_cache".to_string(), "miss".to_string());
                ctx.fields
                    .insert("prepared".to_string(), "true".to_string());
            }
        }
        self.emit_tracing_sql(&ctx);

        let start = Instant::now();
        let result = match probe {
            StmtCacheProbe::Disabled => {
                self.execute_with_timeout(self.client.execute(&ctx.exec_sql, params))
                    .await
            }
            StmtCacheProbe::Hit(stmt) => {
                if self.config.stats_enabled {
                    self.stats.on_stmt_cache_hit();
                }

                let mut result = self
                    .execute_with_timeout(self.client.execute_prepared(&stmt, params))
                    .await;

                if let Err(ref err) = result {
                    if is_retryable_prepared_error(err) {
                        if let Some(cache) = &self.statement_cache {
                            let _ = cache.remove(&ctx.canonical_sql);
                        }

                        if let Some(cache) = &self.statement_cache {
                            let prep_start = Instant::now();
                            let stmt = self
                                .execute_with_timeout(
                                    self.client.prepare_statement(&ctx.canonical_sql),
                                )
                                .await;
                            let prep_dur = prep_start.elapsed();
                            if self.config.stats_enabled {
                                self.stats.on_stmt_prepare(prep_dur);
                            }

                            let stmt = cache.insert_if_absent(ctx.canonical_sql.clone(), stmt?);
                            result = self
                                .execute_with_timeout(self.client.execute_prepared(&stmt, params))
                                .await;
                        }
                    }
                }

                result
            }
            StmtCacheProbe::Miss => {
                if self.config.stats_enabled {
                    self.stats.on_stmt_cache_miss();
                }

                match &self.statement_cache {
                    Some(cache) => {
                        let prep_start = Instant::now();
                        let stmt = self
                            .execute_with_timeout(self.client.prepare_statement(&ctx.canonical_sql))
                            .await;
                        let prep_dur = prep_start.elapsed();
                        if self.config.stats_enabled {
                            self.stats.on_stmt_prepare(prep_dur);
                        }

                        let stmt = cache.insert_if_absent(ctx.canonical_sql.clone(), stmt?);
                        self.execute_with_timeout(self.client.execute_prepared(&stmt, params))
                            .await
                    }
                    None => {
                        self.execute_with_timeout(self.client.execute(&ctx.exec_sql, params))
                            .await
                    }
                }
            }
        };
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
