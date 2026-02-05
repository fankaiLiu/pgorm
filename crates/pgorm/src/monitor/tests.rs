use super::*;
use crate::client::GenericClient;
use crate::error::{OrmError, OrmResult};
use std::sync::Arc;
use std::time::Duration;
use tokio_postgres::Row;
use tokio_postgres::types::ToSql;

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
