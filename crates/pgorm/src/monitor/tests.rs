use super::*;
use crate::client::GenericClient;
use crate::error::{OrmError, OrmResult};
use std::sync::Arc;
use std::time::Duration;
use tokio_postgres::Row;
use tokio_postgres::types::ToSql;

// ── Shared DummyClient for tests ──

struct DummyClient;
impl GenericClient for DummyClient {
    async fn query(&self, _: &str, _: &[&(dyn ToSql + Sync)]) -> OrmResult<Vec<Row>> {
        Ok(vec![])
    }
    async fn query_one(&self, _: &str, _: &[&(dyn ToSql + Sync)]) -> OrmResult<Row> {
        Err(OrmError::not_found("no rows"))
    }
    async fn query_opt(&self, _: &str, _: &[&(dyn ToSql + Sync)]) -> OrmResult<Option<Row>> {
        Ok(None)
    }
    async fn execute(&self, _: &str, _: &[&(dyn ToSql + Sync)]) -> OrmResult<u64> {
        Ok(0)
    }
}

#[test]
fn test_query_type_detection() {
    assert_eq!(
        QueryType::from_sql("SELECT * FROM users"),
        QueryType::Select
    );
    assert_eq!(
        QueryType::from_sql("  select * FROM users"),
        QueryType::Select
    );
    assert_eq!(
        QueryType::from_sql("WITH cte AS (SELECT 1) SELECT * FROM cte"),
        QueryType::Select
    );
    assert_eq!(
        QueryType::from_sql("INSERT INTO users (name) VALUES ($1)"),
        QueryType::Insert
    );
    assert_eq!(
        QueryType::from_sql("UPDATE users SET name = $1"),
        QueryType::Update
    );
    assert_eq!(
        QueryType::from_sql("DELETE FROM users WHERE id = $1"),
        QueryType::Delete
    );
    assert_eq!(
        QueryType::from_sql("CREATE TABLE users (id INT)"),
        QueryType::Other
    );
}

#[test]
fn test_logging_monitor_truncation() {
    let monitor = LoggingMonitor::new().max_sql_length(10);
    assert_eq!(monitor.truncate_sql("SELECT * FROM users"), "SELECT * F...");
    assert_eq!(monitor.truncate_sql("SELECT 1"), "SELECT 1");
}

#[test]
fn test_stats_monitor() {
    let monitor = StatsMonitor::new();
    let ctx = QueryContext::new("SELECT * FROM users", 0);

    monitor.on_query_complete(&ctx, Duration::from_millis(10), &QueryResult::Rows(5));
    monitor.on_query_complete(&ctx, Duration::from_millis(20), &QueryResult::Rows(3));

    let stats = monitor.stats();
    assert_eq!(stats.total_queries, 2);
    assert_eq!(stats.select_count, 2);
    assert_eq!(stats.total_duration, Duration::from_millis(30));
    assert_eq!(stats.stmt_cache_hits, 0);
    assert_eq!(stats.stmt_cache_misses, 0);
    assert_eq!(stats.stmt_prepare_count, 0);
    assert_eq!(stats.stmt_prepare_duration, Duration::ZERO);
}

#[test]
fn test_composite_hook_modify() {
    struct AddCommentHook;
    impl QueryHook for AddCommentHook {
        fn before_query(&self, ctx: &QueryContext) -> HookAction {
            HookAction::ModifySql {
                exec_sql: format!("/* instrumented */ {}", ctx.exec_sql),
                canonical_sql: None,
            }
        }
    }

    let hook = CompositeHook::new().add(AddCommentHook);
    let ctx = QueryContext::new("SELECT 1", 0);

    match hook.before_query(&ctx) {
        HookAction::ModifySql {
            exec_sql,
            canonical_sql,
        } => {
            assert_eq!(exec_sql, "/* instrumented */ SELECT 1");
            assert!(canonical_sql.is_none());
        }
        _ => panic!("Expected ModifySql"),
    }
}

#[test]
fn test_composite_hook_abort() {
    struct BlockDeleteHook;
    impl QueryHook for BlockDeleteHook {
        fn before_query(&self, ctx: &QueryContext) -> HookAction {
            if ctx.query_type == QueryType::Delete {
                HookAction::Abort("DELETE not allowed".to_string())
            } else {
                HookAction::Continue
            }
        }
    }

    let hook = CompositeHook::new().add(BlockDeleteHook);
    let ctx = QueryContext::new("DELETE FROM users", 0);

    match hook.before_query(&ctx) {
        HookAction::Abort(reason) => assert_eq!(reason, "DELETE not allowed"),
        _ => panic!("Expected Abort"),
    }
}

#[tokio::test]
async fn tagged_queries_propagate_to_monitor() {
    #[derive(Default)]
    struct TagCapture(std::sync::Mutex<Option<String>>);

    impl QueryMonitor for TagCapture {
        fn on_query_complete(&self, ctx: &QueryContext, _: Duration, _: &QueryResult) {
            *self.0.lock().unwrap() = ctx.tag.clone();
        }
    }

    let capture = Arc::new(TagCapture::default());
    let client = InstrumentedClient::new(DummyClient)
        .with_config(MonitorConfig::new().enable_monitoring())
        .with_monitor_arc(capture.clone());

    client
        .query_tagged("test-tag", "SELECT 1", &[])
        .await
        .unwrap();

    assert_eq!(capture.0.lock().unwrap().as_deref(), Some("test-tag"));
}

#[tokio::test]
async fn timeout_returns_error_and_attempts_cancellation() {
    struct HangingClient;
    impl GenericClient for HangingClient {
        async fn query(&self, _: &str, _: &[&(dyn ToSql + Sync)]) -> OrmResult<Vec<Row>> {
            tokio::time::sleep(Duration::from_secs(60)).await;
            Ok(vec![])
        }
        async fn query_one(&self, _: &str, _: &[&(dyn ToSql + Sync)]) -> OrmResult<Row> {
            Err(OrmError::not_found("unused"))
        }
        async fn query_opt(&self, _: &str, _: &[&(dyn ToSql + Sync)]) -> OrmResult<Option<Row>> {
            Ok(None)
        }
        async fn execute(&self, _: &str, _: &[&(dyn ToSql + Sync)]) -> OrmResult<u64> {
            Ok(0)
        }
    }

    let client = InstrumentedClient::new(HangingClient).with_config(
        MonitorConfig::new()
            .with_query_timeout(Duration::from_millis(10))
            .enable_monitoring(),
    );

    let err = client.query("SELECT pg_sleep(60)", &[]).await.unwrap_err();
    assert!(matches!(err, OrmError::Timeout(_)));
}

// ── I-12: Additional test coverage ──

#[test]
fn stats_monitor_tracks_all_query_types() {
    let monitor = StatsMonitor::new();

    monitor.on_query_complete(
        &QueryContext::new("SELECT 1", 0),
        Duration::from_millis(1),
        &QueryResult::Rows(1),
    );
    monitor.on_query_complete(
        &QueryContext::new("INSERT INTO t (x) VALUES (1)", 0),
        Duration::from_millis(2),
        &QueryResult::Affected(1),
    );
    monitor.on_query_complete(
        &QueryContext::new("UPDATE t SET x = 1", 0),
        Duration::from_millis(3),
        &QueryResult::Affected(1),
    );
    monitor.on_query_complete(
        &QueryContext::new("DELETE FROM t WHERE id = 1", 0),
        Duration::from_millis(4),
        &QueryResult::Affected(1),
    );
    monitor.on_query_complete(
        &QueryContext::new("CREATE TABLE t (id INT)", 0),
        Duration::from_millis(5),
        &QueryResult::Affected(0),
    );
    // Record an error
    monitor.on_query_complete(
        &QueryContext::new("SELECT bad", 0),
        Duration::from_millis(1),
        &QueryResult::error("some error".to_string()),
    );

    let stats = monitor.stats();
    assert_eq!(stats.total_queries, 6);
    assert_eq!(stats.select_count, 2); // SELECT 1 + SELECT bad
    assert_eq!(stats.insert_count, 1);
    assert_eq!(stats.update_count, 1);
    assert_eq!(stats.delete_count, 1);
    assert_eq!(stats.failed_queries, 1);
    assert_eq!(stats.total_duration, Duration::from_millis(16));
}

#[test]
fn stats_monitor_tracks_slowest_query() {
    let monitor = StatsMonitor::new();

    monitor.on_query_complete(
        &QueryContext::new("SELECT fast", 0),
        Duration::from_millis(10),
        &QueryResult::Rows(0),
    );
    monitor.on_query_complete(
        &QueryContext::new("SELECT slow", 0),
        Duration::from_millis(100),
        &QueryResult::Rows(0),
    );
    monitor.on_query_complete(
        &QueryContext::new("SELECT medium", 0),
        Duration::from_millis(50),
        &QueryResult::Rows(0),
    );

    let stats = monitor.stats();
    assert_eq!(stats.max_duration, Duration::from_millis(100));
    assert_eq!(stats.slowest_query.as_deref(), Some("SELECT slow"));
}

#[test]
fn stats_monitor_reset_clears_all() {
    let monitor = StatsMonitor::new();

    monitor.on_query_complete(
        &QueryContext::new("SELECT 1", 0),
        Duration::from_millis(10),
        &QueryResult::Rows(1),
    );
    monitor.on_stmt_cache_hit();
    monitor.on_stmt_cache_miss();
    monitor.on_stmt_prepare(Duration::from_millis(5));

    let stats = monitor.stats();
    assert_eq!(stats.total_queries, 1);
    assert_eq!(stats.stmt_cache_hits, 1);
    assert_eq!(stats.stmt_cache_misses, 1);
    assert_eq!(stats.stmt_prepare_count, 1);

    monitor.reset();
    let stats = monitor.stats();
    assert_eq!(stats.total_queries, 0);
    assert_eq!(stats.failed_queries, 0);
    assert_eq!(stats.total_duration, Duration::ZERO);
    assert_eq!(stats.select_count, 0);
    assert_eq!(stats.max_duration, Duration::ZERO);
    assert!(stats.slowest_query.is_none());
    assert_eq!(stats.stmt_cache_hits, 0);
    assert_eq!(stats.stmt_cache_misses, 0);
    assert_eq!(stats.stmt_prepare_count, 0);
    assert_eq!(stats.stmt_prepare_duration, Duration::ZERO);
}

#[test]
fn stats_monitor_duration_saturates_on_overflow() {
    let monitor = StatsMonitor::new();

    // Report a query with near-max duration
    monitor.on_query_complete(
        &QueryContext::new("SELECT 1", 0),
        Duration::from_nanos(u64::MAX - 10),
        &QueryResult::Rows(0),
    );
    // This should saturate at u64::MAX, not wrap around
    monitor.on_query_complete(
        &QueryContext::new("SELECT 2", 0),
        Duration::from_nanos(100),
        &QueryResult::Rows(0),
    );

    let stats = monitor.stats();
    assert_eq!(stats.total_duration, Duration::from_nanos(u64::MAX));
}

#[test]
fn composite_hook_chains_multiple_modifiers() {
    struct PrefixHook(&'static str);
    impl QueryHook for PrefixHook {
        fn before_query(&self, ctx: &QueryContext) -> HookAction {
            HookAction::ModifySql {
                exec_sql: format!("{}{}", self.0, ctx.exec_sql),
                canonical_sql: None,
            }
        }
    }

    let hook = CompositeHook::new()
        .add(PrefixHook("/* a */ "))
        .add(PrefixHook("/* b */ "));

    let ctx = QueryContext::new("SELECT 1", 0);
    match hook.before_query(&ctx) {
        HookAction::ModifySql {
            exec_sql,
            canonical_sql,
        } => {
            assert_eq!(exec_sql, "/* b */ /* a */ SELECT 1");
            // canonical_sql should be None since no hook modified it
            assert!(canonical_sql.is_none());
        }
        _ => panic!("Expected ModifySql"),
    }
}

#[test]
fn composite_hook_abort_stops_chain() {
    struct AbortHook;
    impl QueryHook for AbortHook {
        fn before_query(&self, _ctx: &QueryContext) -> HookAction {
            HookAction::Abort("blocked".to_string())
        }
    }
    struct PanicHook;
    impl QueryHook for PanicHook {
        fn before_query(&self, _ctx: &QueryContext) -> HookAction {
            panic!("should not be called");
        }
    }

    let hook = CompositeHook::new().add(AbortHook).add(PanicHook);
    let ctx = QueryContext::new("SELECT 1", 0);
    match hook.before_query(&ctx) {
        HookAction::Abort(reason) => assert_eq!(reason, "blocked"),
        _ => panic!("Expected Abort"),
    }
}

#[test]
fn composite_hook_continue_only_returns_continue() {
    struct NoopHook;
    impl QueryHook for NoopHook {
        fn before_query(&self, _ctx: &QueryContext) -> HookAction {
            HookAction::Continue
        }
    }

    let hook = CompositeHook::new().add(NoopHook).add(NoopHook);
    let ctx = QueryContext::new("SELECT 1", 0);
    assert!(matches!(hook.before_query(&ctx), HookAction::Continue));
}

#[test]
fn query_type_cte_insert() {
    assert_eq!(
        QueryType::from_sql("WITH cte AS (SELECT 1) INSERT INTO t SELECT * FROM cte"),
        QueryType::Insert
    );
}

#[test]
fn query_type_cte_update() {
    assert_eq!(
        QueryType::from_sql("WITH cte AS (SELECT 1) UPDATE t SET x = 1"),
        QueryType::Update
    );
}

#[test]
fn query_type_cte_delete() {
    assert_eq!(
        QueryType::from_sql("WITH cte AS (SELECT 1) DELETE FROM t WHERE id = 1"),
        QueryType::Delete
    );
}

#[test]
fn query_type_nested_cte() {
    // Multiple CTEs, final statement is SELECT
    assert_eq!(
        QueryType::from_sql("WITH a AS (SELECT 1), b AS (SELECT 2) SELECT * FROM a JOIN b ON true"),
        QueryType::Select
    );
}

#[test]
fn query_result_error_truncation() {
    let short = QueryResult::error("short error".to_string());
    assert!(matches!(short, QueryResult::Error(s) if s == "short error"));

    let long_msg = "x".repeat(1000);
    let truncated = QueryResult::error(long_msg);
    match truncated {
        QueryResult::Error(s) => {
            assert!(s.len() <= 520); // 512 + "..."
            assert!(s.ends_with("..."));
        }
        _ => panic!("Expected Error"),
    }
}

#[test]
fn query_result_display() {
    assert_eq!(format!("{}", QueryResult::Rows(5)), "5 rows");
    assert_eq!(format!("{}", QueryResult::Affected(3)), "3 affected");
    assert_eq!(format!("{}", QueryResult::OptionalRow(true)), "1 row");
    assert_eq!(format!("{}", QueryResult::OptionalRow(false)), "0 rows");
    assert_eq!(
        format!("{}", QueryResult::Error("oops".to_string())),
        "error: oops"
    );
}

#[tokio::test]
async fn instrumented_client_hook_abort_prevents_execution() {
    struct AlwaysAbort;
    impl QueryHook for AlwaysAbort {
        fn before_query(&self, _ctx: &QueryContext) -> HookAction {
            HookAction::Abort("query blocked".to_string())
        }
    }

    let client = InstrumentedClient::new(DummyClient)
        .with_config(MonitorConfig::new().enable_monitoring())
        .with_hook(AlwaysAbort);

    let err = client.query("SELECT 1", &[]).await.unwrap_err();
    assert!(matches!(err, OrmError::Validation(_)));
}

#[tokio::test]
async fn instrumented_client_hook_modifies_sql() {
    #[derive(Default)]
    struct SqlCapture(std::sync::Mutex<Option<String>>);

    impl QueryMonitor for SqlCapture {
        fn on_query_complete(&self, ctx: &QueryContext, _: Duration, _: &QueryResult) {
            *self.0.lock().unwrap() = Some(ctx.exec_sql.clone());
        }
    }

    struct AddComment;
    impl QueryHook for AddComment {
        fn before_query(&self, ctx: &QueryContext) -> HookAction {
            HookAction::ModifySql {
                exec_sql: format!("/* traced */ {}", ctx.exec_sql),
                canonical_sql: None,
            }
        }
    }

    let capture = Arc::new(SqlCapture::default());
    let client = InstrumentedClient::new(DummyClient)
        .with_config(MonitorConfig::new().enable_monitoring())
        .with_monitor_arc(capture.clone())
        .with_hook(AddComment);

    client.query("SELECT 1", &[]).await.unwrap();

    let exec = capture.0.lock().unwrap().clone().unwrap();
    assert_eq!(exec, "/* traced */ SELECT 1");
}

#[tokio::test]
async fn instrumented_client_monitoring_disabled_skips_monitor() {
    struct FailMonitor;
    impl QueryMonitor for FailMonitor {
        fn on_query_complete(&self, _: &QueryContext, _: Duration, _: &QueryResult) {
            panic!("should not be called when monitoring is disabled");
        }
    }

    let client = InstrumentedClient::new(DummyClient)
        .with_config(MonitorConfig::new()) // monitoring_enabled = false by default
        .with_monitor(FailMonitor);

    // Should succeed without calling the monitor
    client.query("SELECT 1", &[]).await.unwrap();
}

#[tokio::test]
async fn instrumented_client_slow_query_threshold() {
    #[derive(Default)]
    struct SlowCapture(std::sync::Mutex<bool>);

    impl QueryMonitor for SlowCapture {
        fn on_query_complete(&self, _: &QueryContext, _: Duration, _: &QueryResult) {}
        fn on_slow_query(&self, _: &QueryContext, _: Duration) {
            *self.0.lock().unwrap() = true;
        }
    }

    struct SlowClient;
    impl GenericClient for SlowClient {
        async fn query(&self, _: &str, _: &[&(dyn ToSql + Sync)]) -> OrmResult<Vec<Row>> {
            tokio::time::sleep(Duration::from_millis(50)).await;
            Ok(vec![])
        }
        async fn query_one(&self, _: &str, _: &[&(dyn ToSql + Sync)]) -> OrmResult<Row> {
            Err(OrmError::not_found("unused"))
        }
        async fn query_opt(&self, _: &str, _: &[&(dyn ToSql + Sync)]) -> OrmResult<Option<Row>> {
            Ok(None)
        }
        async fn execute(&self, _: &str, _: &[&(dyn ToSql + Sync)]) -> OrmResult<u64> {
            Ok(0)
        }
    }

    let capture = Arc::new(SlowCapture::default());
    let client = InstrumentedClient::new(SlowClient)
        .with_config(
            MonitorConfig::new()
                .with_slow_query_threshold(Duration::from_millis(10))
                .enable_monitoring(),
        )
        .with_monitor_arc(capture.clone());

    client.query("SELECT pg_sleep(0.05)", &[]).await.unwrap();
    assert!(*capture.0.lock().unwrap());
}
