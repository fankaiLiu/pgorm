use super::config::PgClientConfig;
use crate::error::{OrmError, OrmResult};
use crate::monitor::{QueryContext, QueryHook, QueryMonitor, QueryResult};
use crate::{GenericClient, RowStream, StreamingClient};
use futures_core::Stream;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};
use tokio_postgres::Row;
use tokio_postgres::types::ToSql;

use crate::monitor::{LoggingMonitor, StatsMonitor};

#[derive(Clone)]
pub(super) struct PgClientStreamReporter {
    pub(super) stats: Arc<StatsMonitor>,
    pub(super) logging_monitor: Option<LoggingMonitor>,
    pub(super) custom_monitor: Option<Arc<dyn QueryMonitor>>,
    pub(super) hook: Option<Arc<dyn QueryHook>>,
    pub(super) config: PgClientConfig,
}

impl PgClientStreamReporter {
    fn report(&self, ctx: &QueryContext, duration: Duration, result: &QueryResult) {
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
}

pub(super) struct PgClientRowStream {
    inner: RowStream,
    reporter: PgClientStreamReporter,
    ctx: QueryContext,
    start: Instant,
    rows: usize,
    cancel_token: Option<tokio_postgres::CancelToken>,
    timeout_sleep: Option<Pin<Box<tokio::time::Sleep>>>,
    finished: bool,
    terminated: bool,
}

impl PgClientRowStream {
    pub(super) fn new(
        inner: RowStream,
        reporter: PgClientStreamReporter,
        mut ctx: QueryContext,
        start: Instant,
        cancel_token: Option<tokio_postgres::CancelToken>,
        timeout_remaining: Option<Duration>,
    ) -> Self {
        ctx.fields.insert("stream".to_string(), "true".to_string());

        let timeout_sleep = timeout_remaining.map(|d| Box::pin(tokio::time::sleep(d)));

        Self {
            inner,
            reporter,
            ctx,
            start,
            rows: 0,
            cancel_token,
            timeout_sleep,
            finished: false,
            terminated: false,
        }
    }

    fn finalize(&mut self, dropped: bool, err: Option<&OrmError>) {
        if self.finished {
            return;
        }
        self.finished = true;
        self.ctx
            .fields
            .insert("stream_dropped".to_string(), dropped.to_string());

        let duration = self.start.elapsed();
        let query_result = match err {
            None => QueryResult::Rows(self.rows),
            Some(OrmError::Timeout(d)) => QueryResult::Error(format!("timeout after {d:?}")),
            Some(e) => QueryResult::Error(e.to_string()),
        };
        self.reporter.report(&self.ctx, duration, &query_result);
    }
}

impl Stream for PgClientRowStream {
    type Item = OrmResult<Row>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.terminated {
            return Poll::Ready(None);
        }

        if let (Some(timeout), Some(sleep)) = (
            self.reporter.config.query_timeout,
            self.timeout_sleep.as_mut(),
        ) {
            if Pin::new(sleep).poll(cx).is_ready() {
                self.timeout_sleep = None;
                self.terminated = true;

                if let Some(cancel_token) = self.cancel_token.take() {
                    tokio::spawn(async move {
                        let _ = cancel_token.cancel_query(tokio_postgres::NoTls).await;
                    });
                }

                let err = OrmError::Timeout(timeout);
                self.finalize(false, Some(&err));
                return Poll::Ready(Some(Err(err)));
            }
        }

        match Pin::new(&mut self.inner).poll_next(cx) {
            Poll::Ready(Some(Ok(row))) => {
                self.rows += 1;
                Poll::Ready(Some(Ok(row)))
            }
            Poll::Ready(Some(Err(e))) => {
                self.terminated = true;
                self.finalize(false, Some(&e));
                Poll::Ready(Some(Err(e)))
            }
            Poll::Ready(None) => {
                self.terminated = true;
                self.finalize(false, None);
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

impl Drop for PgClientRowStream {
    fn drop(&mut self) {
        if self.terminated {
            return;
        }
        self.finalize(true, None);
    }
}

impl<C: GenericClient> super::PgClient<C>
where
    C: StreamingClient,
{
    async fn query_stream_impl(
        &self,
        tag: Option<&str>,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> OrmResult<RowStream> {
        let mut ctx = QueryContext::new(sql, params.len());
        if let Some(tag) = tag {
            ctx.tag = Some(tag.to_string());
        }

        // Process hook first, then check the canonical SQL.
        self.apply_hook(&mut ctx)?;
        self.apply_sql_policy(&mut ctx)?;
        self.check_sql(&ctx.canonical_sql)?;
        self.emit_tracing_sql(&ctx);

        let start = Instant::now();
        let result = self
            .execute_with_timeout(self.client.query_stream(&ctx.exec_sql, params))
            .await;

        match result {
            Ok(stream) => {
                let needs_wrap = self.config.query_timeout.is_some()
                    || self.config.stats_enabled
                    || self.logging_monitor.is_some()
                    || self.custom_monitor.is_some()
                    || self.hook.is_some()
                    || self.config.slow_query_threshold.is_some();

                if !needs_wrap {
                    return Ok(stream);
                }

                let timeout_remaining = self
                    .config
                    .query_timeout
                    .map(|t| t.saturating_sub(start.elapsed()));

                let reporter = PgClientStreamReporter {
                    stats: self.stats.clone(),
                    logging_monitor: self.logging_monitor.clone(),
                    custom_monitor: self.custom_monitor.clone(),
                    hook: self.hook.clone(),
                    config: self.config.clone(),
                };

                Ok(RowStream::new(PgClientRowStream::new(
                    stream,
                    reporter,
                    ctx,
                    start,
                    self.client.cancel_token(),
                    timeout_remaining,
                )))
            }
            Err(e) => {
                let mut ctx = ctx;
                ctx.fields.insert("stream".to_string(), "true".to_string());

                let duration = start.elapsed();
                let query_result = match &e {
                    OrmError::Timeout(d) => QueryResult::Error(format!("timeout after {d:?}")),
                    other => QueryResult::Error(other.to_string()),
                };
                self.report_result(&ctx, duration, &query_result);
                Err(e)
            }
        }
    }
}

impl<C: GenericClient + StreamingClient> StreamingClient for super::PgClient<C> {
    async fn query_stream(
        &self,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> OrmResult<RowStream> {
        self.query_stream_impl(None, sql, params).await
    }

    async fn query_stream_tagged(
        &self,
        tag: &str,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> OrmResult<RowStream> {
        self.query_stream_impl(Some(tag), sql, params).await
    }
}
