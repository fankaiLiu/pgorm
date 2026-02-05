use super::*;
use crate::GenericClient;
use crate::error::{OrmError, OrmResult};
use crate::monitor::{QueryContext, QueryMonitor, QueryResult};
use std::time::Duration;
use tokio_postgres::Row;
use tokio_postgres::types::ToSql;

#[test]
fn test_config_defaults() {
    let config = PgClientConfig::default();
    assert_eq!(config.check_mode, CheckMode::WarnOnly);
    assert_eq!(
        config.sql_policy.select_without_limit,
        SelectWithoutLimitPolicy::Allow
    );
    assert_eq!(
        config.sql_policy.delete_without_where,
        DangerousDmlPolicy::Allow
    );
    assert_eq!(
        config.sql_policy.update_without_where,
        DangerousDmlPolicy::Allow
    );
    assert!(config.stats_enabled);
    assert!(!config.logging_enabled);
    assert!(!config.statement_cache.enabled);
    assert_eq!(config.statement_cache.capacity, 0);
}

#[test]
fn test_config_builder() {
    let config = PgClientConfig::new()
        .strict()
        .timeout(Duration::from_secs(30))
        .with_logging()
        .statement_cache(64);

    assert_eq!(config.check_mode, CheckMode::Strict);
    assert_eq!(config.query_timeout, Some(Duration::from_secs(30)));
    assert!(config.logging_enabled);
    assert!(config.statement_cache.enabled);
    assert_eq!(config.statement_cache.capacity, 64);
}

#[test]
fn test_config_no_statement_cache() {
    let config = PgClientConfig::new()
        .statement_cache(16)
        .no_statement_cache();
    assert!(!config.statement_cache.enabled);
}

#[tokio::test]
async fn sql_policy_select_without_limit_errors() {
    #[derive(Default)]
    struct Capture(std::sync::Mutex<Option<String>>);

    #[derive(Clone)]
    struct DummyClient(std::sync::Arc<Capture>);

    impl GenericClient for DummyClient {
        async fn query(&self, sql: &str, _: &[&(dyn ToSql + Sync)]) -> OrmResult<Vec<Row>> {
            *self.0.0.lock().unwrap() = Some(sql.to_string());
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

    let capture = std::sync::Arc::new(Capture::default());
    let config = PgClientConfig::new()
        .no_check()
        .select_without_limit(SelectWithoutLimitPolicy::Error);
    let pg = PgClient::with_config(DummyClient(capture.clone()), config);

    let err = pg.query("SELECT * FROM users", &[]).await.unwrap_err();
    assert!(matches!(err, OrmError::Validation(_)));
    assert!(capture.0.lock().unwrap().is_none());
}

#[tokio::test]
async fn sql_policy_select_without_limit_auto_limit_rewrites_exec_sql() {
    #[derive(Default)]
    struct Capture(std::sync::Mutex<Option<String>>);

    #[derive(Clone)]
    struct DummyClient(std::sync::Arc<Capture>);

    impl GenericClient for DummyClient {
        async fn query(&self, sql: &str, _: &[&(dyn ToSql + Sync)]) -> OrmResult<Vec<Row>> {
            *self.0.0.lock().unwrap() = Some(sql.to_string());
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

    let capture = std::sync::Arc::new(Capture::default());
    let config = PgClientConfig::new()
        .no_check()
        .select_without_limit(SelectWithoutLimitPolicy::AutoLimit(10));
    let pg = PgClient::with_config(DummyClient(capture.clone()), config);

    pg.query("SELECT * FROM users", &[]).await.unwrap();

    let executed = capture.0.lock().unwrap().clone().unwrap();
    assert!(executed.to_uppercase().contains("LIMIT 10"));
}

#[tokio::test]
async fn sql_policy_delete_without_where_errors() {
    #[derive(Default)]
    struct Capture(std::sync::Mutex<Option<String>>);

    #[derive(Clone)]
    struct DummyClient(std::sync::Arc<Capture>);

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
        async fn execute(&self, sql: &str, _: &[&(dyn ToSql + Sync)]) -> OrmResult<u64> {
            *self.0.0.lock().unwrap() = Some(sql.to_string());
            Ok(0)
        }
    }

    let capture = std::sync::Arc::new(Capture::default());
    let config = PgClientConfig::new()
        .no_check()
        .delete_without_where(DangerousDmlPolicy::Error);
    let pg = PgClient::with_config(DummyClient(capture.clone()), config);

    let err = pg.execute("DELETE FROM users", &[]).await.unwrap_err();
    assert!(matches!(err, OrmError::Validation(_)));
    assert!(capture.0.lock().unwrap().is_none());
}

#[tokio::test]
async fn tagged_queries_propagate_to_custom_monitor() {
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

    let capture = std::sync::Arc::new(TagCapture::default());
    let pg = PgClient::with_config(DummyClient, PgClientConfig::new().no_check())
        .with_monitor_arc(capture.clone());

    pg.query_tagged("test-tag", "SELECT 1", &[]).await.unwrap();

    assert_eq!(capture.0.lock().unwrap().as_deref(), Some("test-tag"));
}
