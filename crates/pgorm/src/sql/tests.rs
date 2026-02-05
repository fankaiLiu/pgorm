use super::*;
use crate::condition::Condition;

async fn try_connect() -> Option<tokio_postgres::Client> {
    let database_url = std::env::var("DATABASE_URL").ok()?;
    let (client, connection) = tokio_postgres::connect(&database_url, tokio_postgres::NoTls)
        .await
        .expect("Failed to connect to DATABASE_URL with NoTls");
    tokio::spawn(async move {
        if let Err(e) = connection.await {
            eprintln!("tokio-postgres connection error: {e}");
        }
    });
    Some(client)
}

#[test]
fn builds_placeholders_in_order() {
    let mut q = sql("SELECT * FROM users WHERE a = ");
    q.push_bind(1).push(" AND b = ").push_bind("x");

    assert_eq!(q.to_sql(), "SELECT * FROM users WHERE a = $1 AND b = $2");
    assert_eq!(q.params_ref().len(), 2);
}

#[test]
fn can_compose_fragments() {
    let mut w = Sql::empty();
    w.push(" WHERE id = ").push_bind(42);

    let mut q = sql("SELECT * FROM users");
    q.push_sql(w);

    assert_eq!(q.to_sql(), "SELECT * FROM users WHERE id = $1");
    assert_eq!(q.params_ref().len(), 1);
}

#[test]
fn bind_list_renders_commas() {
    let mut q = sql("SELECT * FROM users WHERE id IN (");
    q.push_bind_list(vec![1, 2, 3]).push(")");
    assert_eq!(q.to_sql(), "SELECT * FROM users WHERE id IN ($1, $2, $3)");
    assert_eq!(q.params_ref().len(), 3);
}

#[test]
fn bind_list_empty_is_valid_sql() {
    let mut q = sql("SELECT * FROM users WHERE id IN (");
    q.push_bind_list(Vec::<i32>::new()).push(")");
    assert_eq!(q.to_sql(), "SELECT * FROM users WHERE id IN (NULL)");
    assert_eq!(q.params_ref().len(), 0);
}

#[test]
fn push_ident_accepts_simple_and_dotted() {
    let mut q = Sql::empty();
    q.push_ident("users").unwrap();
    q.push(", ");
    q.push_ident("public.users").unwrap();
    assert_eq!(q.to_sql(), "users, public.users");
}

#[test]
fn push_ident_rejects_unsafe() {
    let mut q = Sql::empty();
    assert!(q.push_ident("users; drop table users; --").is_err());
    assert!(q.push_ident("1users").is_err());
    assert!(q.push_ident("users..name").is_err());
    assert!(q.push_ident("users name").is_err());
}

#[test]
fn can_append_condition_as_placeholders() {
    let mut q = sql("SELECT * FROM users WHERE ");
    q.push_condition(&Condition::eq("id", 42_i64).unwrap());

    assert_eq!(q.to_sql(), "SELECT * FROM users WHERE id = $1");
    assert_eq!(q.params_ref().len(), 1);
}

#[test]
fn condition_placeholders_compose_with_push_bind() {
    let mut q = sql("SELECT * FROM users WHERE a = ");
    q.push_bind(1_i64);
    q.push(" AND ");
    q.push_condition(&Condition::eq("b", "x").unwrap());

    assert_eq!(q.to_sql(), "SELECT * FROM users WHERE a = $1 AND b = $2");
    assert_eq!(q.params_ref().len(), 2);
}

#[test]
fn empty_in_list_condition_is_valid_sql() {
    let mut q = sql("SELECT * FROM users WHERE ");
    q.push_condition(&Condition::in_list("id", Vec::<i32>::new()).unwrap());

    assert_eq!(q.to_sql(), "SELECT * FROM users WHERE 1=0");
    assert_eq!(q.params_ref().len(), 0);
}

// ==================== Phase 1: Convenience API tests ====================

#[test]
fn limit_appends_with_param() {
    let mut q = sql("SELECT * FROM users ORDER BY id");
    q.limit(10);
    assert_eq!(q.to_sql(), "SELECT * FROM users ORDER BY id LIMIT $1");
    assert_eq!(q.params_ref().len(), 1);
}

#[test]
fn offset_appends_with_param() {
    let mut q = sql("SELECT * FROM users ORDER BY id");
    q.offset(20);
    assert_eq!(q.to_sql(), "SELECT * FROM users ORDER BY id OFFSET $1");
    assert_eq!(q.params_ref().len(), 1);
}

#[test]
fn limit_offset_appends_both_params() {
    let mut q = sql("SELECT * FROM users ORDER BY id");
    q.limit_offset(10, 20);
    assert_eq!(
        q.to_sql(),
        "SELECT * FROM users ORDER BY id LIMIT $1 OFFSET $2"
    );
    assert_eq!(q.params_ref().len(), 2);
}

#[test]
fn page_converts_to_limit_offset() {
    let mut q = sql("SELECT * FROM users ORDER BY id");
    q.page(3, 25).unwrap();
    // page 3 with 25 per page = OFFSET 50
    assert_eq!(
        q.to_sql(),
        "SELECT * FROM users ORDER BY id LIMIT $1 OFFSET $2"
    );
    assert_eq!(q.params_ref().len(), 2);
}

#[test]
fn page_rejects_zero() {
    let mut q = sql("SELECT * FROM users ORDER BY id");
    assert!(q.page(0, 25).is_err());
}

#[test]
fn page_rejects_negative() {
    let mut q = sql("SELECT * FROM users ORDER BY id");
    assert!(q.page(-1, 25).is_err());
}

#[test]
fn pagination_composes_with_conditions() {
    let mut q = sql("SELECT * FROM users WHERE ");
    q.push_condition(&Condition::eq("status", "active").unwrap());
    q.push(" ORDER BY id");
    q.limit_offset(10, 0);
    assert_eq!(
        q.to_sql(),
        "SELECT * FROM users WHERE status = $1 ORDER BY id LIMIT $2 OFFSET $3"
    );
    assert_eq!(q.params_ref().len(), 3);
}

#[tokio::test]
async fn fetch_one_multi_rows_returns_first_row() {
    let Some(client) = try_connect().await else {
        eprintln!("DATABASE_URL not set; skipping");
        return;
    };

    let row = query("SELECT n FROM (VALUES (1), (2)) AS t(n) ORDER BY n")
        .fetch_one(&client)
        .await
        .unwrap();
    let n: i32 = row.get(0);
    assert_eq!(n, 1);
}

#[tokio::test]
async fn fetch_one_strict_zero_rows_is_not_found() {
    let Some(client) = try_connect().await else {
        eprintln!("DATABASE_URL not set; skipping");
        return;
    };

    let err = query("SELECT 1 WHERE FALSE")
        .fetch_one_strict(&client)
        .await
        .unwrap_err();
    assert!(err.is_not_found());
}

#[tokio::test]
async fn fetch_one_strict_multi_rows_is_too_many_rows() {
    use crate::error::OrmError;

    let Some(client) = try_connect().await else {
        eprintln!("DATABASE_URL not set; skipping");
        return;
    };

    let err = query("SELECT n FROM (VALUES (1), (2)) AS t(n)")
        .fetch_one_strict(&client)
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        OrmError::TooManyRows {
            expected: 1,
            got: 2
        }
    ));
}
