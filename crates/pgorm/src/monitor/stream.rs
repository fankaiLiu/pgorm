use super::config::MonitorConfig;
use super::types::{QueryContext, QueryHook, QueryMonitor, QueryResult};
use crate::client::{GenericClient, RowStream, StreamingClient};
use crate::error::{OrmError, OrmResult};
use futures_core::Stream;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};
use tokio_postgres::Row;
use tokio_postgres::types::ToSql;

struct InstrumentedRowStream {
    inner: RowStream,
    monitor: Arc<dyn QueryMonitor>,
    hook: Option<Arc<dyn QueryHook>>,
    config: MonitorConfig,
    ctx: QueryContext,
    start: Instant,
    rows: usize,
    cancel_token: Option<tokio_postgres::CancelToken>,
    timeout_sleep: Option<Pin<Box<tokio::time::Sleep>>>,
    finished: bool,
    terminated: bool,
}

impl InstrumentedRowStream {
    #[allow(clippy::too_many_arguments)]
    fn new(
        inner: RowStream,
        monitor: Arc<dyn QueryMonitor>,
        hook: Option<Arc<dyn QueryHook>>,
        config: MonitorConfig,
        mut ctx: QueryContext,
        start: Instant,
        cancel_token: Option<tokio_postgres::CancelToken>,
        timeout_remaining: Option<Duration>,
    ) -> Self {
        ctx.fields.insert("stream".to_string(), "true".to_string());

        let timeout_sleep = timeout_remaining.map(|d| Box::pin(tokio::time::sleep(d)));

        Self {
            inner,
            monitor,
            hook,
            config,
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

        if !self.config.monitoring_enabled {
            return;
        }

        let duration = self.start.elapsed();
        let query_result = match err {
            None => QueryResult::Rows(self.rows),
            Some(OrmError::Timeout(d)) => QueryResult::Error(format!("timeout after {d:?}")),
            Some(e) => QueryResult::Error(e.to_string()),
        };

        if let Some(hook) = &self.hook {
            hook.after_query(&self.ctx, duration, &query_result);
        }

        self.monitor
            .on_query_complete(&self.ctx, duration, &query_result);

        if let Some(threshold) = self.config.slow_query_threshold {
            if duration > threshold {
                self.monitor.on_slow_query(&self.ctx, duration);
            }
        }
    }
}

impl Stream for InstrumentedRowStream {
    type Item = OrmResult<Row>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.terminated {
            return Poll::Ready(None);
        }

        if let (Some(timeout), Some(sleep)) =
            (self.config.query_timeout, self.timeout_sleep.as_mut())
        {
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

impl Drop for InstrumentedRowStream {
    fn drop(&mut self) {
        if self.terminated {
            return;
        }
        self.finalize(true, None);
    }
}

impl<C: GenericClient + StreamingClient> super::instrumented::InstrumentedClient<C> {
    async fn query_stream_inner(
        &self,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
        tag: Option<&str>,
    ) -> OrmResult<RowStream> {
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
            .execute_with_timeout(self.client.query_stream(&ctx.exec_sql, params))
            .await;

        match result {
            Ok(stream) => {
                let needs_wrap =
                    self.config.monitoring_enabled || self.config.query_timeout.is_some();
                if !needs_wrap {
                    return Ok(stream);
                }

                let timeout_remaining = self
                    .config
                    .query_timeout
                    .map(|t| t.saturating_sub(start.elapsed()));

                Ok(RowStream::new(InstrumentedRowStream::new(
                    stream,
                    self.monitor.clone(),
                    self.hook.clone(),
                    self.config.clone(),
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

impl<C: GenericClient + StreamingClient> StreamingClient
    for super::instrumented::InstrumentedClient<C>
{
    async fn query_stream(
        &self,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> OrmResult<RowStream> {
        self.query_stream_inner(sql, params, None).await
    }

    async fn query_stream_tagged(
        &self,
        tag: &str,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> OrmResult<RowStream> {
        self.query_stream_inner(sql, params, Some(tag)).await
    }
}
