//! Compile-only tests for core API patterns.
//!
//! These tests verify that key API surfaces compile correctly.
//! They do NOT execute against a database — they only check types and signatures.

#![allow(dead_code)]

use pgorm::prelude::*;
use pgorm::{OrmResult, query, sql};

// ── Model definitions ────────────────────────────────────────────────────────

#[derive(Debug, Clone, FromRow, Model)]
#[orm(table = "compile_users")]
#[orm(has_many(CompilePost, foreign_key = "user_id", as = "posts"))]
struct CompileUser {
    #[orm(id)]
    id: i64,
    name: String,
    email: String,
}

#[derive(Debug, Clone, FromRow, Model)]
#[orm(table = "compile_posts")]
#[orm(belongs_to(CompileUser, foreign_key = "user_id", as = "author"))]
struct CompilePost {
    #[orm(id)]
    id: i64,
    user_id: i64,
    title: String,
}

#[derive(Debug, InsertModel)]
#[orm(table = "compile_users", returning = "CompileUser")]
struct NewCompileUser {
    name: String,
    email: String,
}

#[derive(Debug, UpdateModel)]
#[orm(
    table = "compile_users",
    model = "CompileUser",
    returning = "CompileUser"
)]
struct CompileUserPatch {
    name: Option<String>,
    email: Option<String>,
}

// ── Compile checks ──────────────────────────────────────────────────────────

#[test]
fn compile_condition_builders() {
    let _ = || -> OrmResult<()> {
        let _ = Condition::eq("status", "active")?;
        let _ = Condition::ne("role", "guest")?;
        let _ = Condition::lt("age", 18_i32)?;
        let _ = Condition::gt("score", 90_i32)?;
        let _ = Condition::ilike("name", "%test%")?;
        let _ = Condition::is_null("deleted_at")?;
        let _ = Condition::is_not_null("email")?;
        let _ = Condition::new("id", Op::between(1_i64, 100_i64))?;
        Ok(())
    };
}

#[test]
fn compile_where_expr() {
    let _ = || -> OrmResult<()> {
        let where_expr = WhereExpr::and(vec![
            Condition::eq("status", "active")?.into(),
            WhereExpr::or(vec![
                Condition::eq("role", "admin")?.into(),
                Condition::eq("role", "owner")?.into(),
            ]),
        ]);
        assert!(!where_expr.is_trivially_true());

        let empty = WhereExpr::and(Vec::new());
        assert!(empty.is_trivially_true());
        Ok(())
    };
}

#[test]
fn compile_sql_builder() {
    let _ = || -> OrmResult<()> {
        let mut q = sql("SELECT * FROM users");
        q.push(" WHERE ");
        let cond = Condition::eq("status", "active")?;
        WhereExpr::atom(cond).append_to_sql(&mut q);

        OrderBy::new().desc("created_at")?.append_to_sql(&mut q);
        Pagination::page(1, 20)?.append_to_sql(&mut q);

        // Verify SQL generation
        let _sql_str = q.to_sql();
        let _params_count = q.params_ref().len();
        Ok(())
    };
}

#[test]
fn compile_keyset_pagination() {
    let _ = || -> OrmResult<()> {
        let keyset = Keyset2::desc("created_at", "id")?.limit(20);
        let mut q = sql("SELECT * FROM users");
        keyset.append_order_by_limit_to_sql(&mut q)?;
        Ok(())
    };
}

#[test]
fn compile_bulk_operations() {
    let _ = || -> OrmResult<()> {
        let _ = SetExpr::set("status", "inactive")?;
        let _ = SetExpr::raw("updated_at = NOW()");
        Ok(())
    };
}

#[test]
fn compile_cte() {
    let _ = || -> OrmResult<()> {
        let mut cte_sql = sql("SELECT id, name FROM users WHERE status = ");
        cte_sql.push_bind("active");
        let _ = sql("")
            .with("active_users", cte_sql)?
            .select(sql("SELECT * FROM active_users"));
        Ok(())
    };
}

#[test]
fn compile_query_bind() {
    let _ = || {
        let _ = query("SELECT * FROM users WHERE id = $1").bind(1_i64);
        let _ = query("SELECT * FROM users WHERE name = $1 AND age > $2")
            .bind("alice")
            .bind(18_i32);
    };
}

#[cfg(feature = "check")]
#[test]
fn compile_pg_client() {
    use pgorm::PgClientConfig;
    use std::time::Duration;

    // Verify PgClientConfig builder compiles
    let _ = PgClientConfig::new()
        .strict()
        .timeout(Duration::from_secs(30))
        .slow_threshold(Duration::from_secs(1))
        .statement_cache(128)
        .with_logging()
        .log_slow_queries(Duration::from_millis(50));

    let _ = PgClientConfig::new()
        .no_check()
        .no_statement_cache()
        .no_stats();
}

#[cfg(feature = "check")]
#[test]
fn compile_sql_policy() {
    use pgorm::{DangerousDmlPolicy, PgClientConfig, SelectWithoutLimitPolicy};

    let _ = PgClientConfig::new()
        .delete_without_where(DangerousDmlPolicy::Error)
        .update_without_where(DangerousDmlPolicy::Warn)
        .truncate_policy(DangerousDmlPolicy::Error)
        .drop_table_policy(DangerousDmlPolicy::Error)
        .select_without_limit(SelectWithoutLimitPolicy::AutoLimit(1000));
}

#[test]
fn compile_query_builder_opt_variants() {
    let _ = || -> OrmResult<()> {
        type Q = CompileUserQuery;

        // Basic opt variants
        let _q = CompileUser::query()
            .eq_opt(Q::name, Some("alice".to_string()))?
            .eq_opt(Q::name, None::<String>)?
            .eq_opt_str(Q::name, Some("bob"))?
            .ne_opt(Q::id, Some(1_i64))?
            .ne_opt(Q::id, None::<i64>)?
            .gt_opt(Q::id, Some(10_i64))?
            .gt_opt(Q::id, None::<i64>)?
            .lt_opt(Q::id, Some(100_i64))?
            .lt_opt(Q::id, None::<i64>)?
            .gte_opt(Q::id, Some(5_i64))?
            .lte_opt(Q::id, Some(50_i64))?
            .like_opt(Q::name, Some("%ali%".to_string()))?
            .like_opt(Q::name, None::<String>)?
            .ilike_opt(Q::name, Some("%ALI%".to_string()))?
            .ilike_opt(Q::name, None::<String>)?
            .in_list_opt(Q::id, Some(vec![1_i64, 2, 3]))?
            .in_list_opt(Q::id, None::<Vec<i64>>)?
            .between_opt(Q::id, Some(1_i64), Some(100_i64))?
            .between_opt(Q::id, None::<i64>, None::<i64>)?;
        Ok(())
    };
}

#[test]
fn compile_query_builder_ilike_any() {
    let _ = || -> OrmResult<()> {
        type Q = CompileUserQuery;

        let _q = CompileUser::query().ilike_any(&[Q::name, Q::email], "%search%")?;

        // opt variant
        let _q = CompileUser::query()
            .ilike_any_opt(&[Q::name, Q::email], Some("%search%".to_string()))?
            .ilike_any_opt(&[Q::name, Q::email], None::<String>)?;
        Ok(())
    };
}

#[test]
fn compile_query_builder_raw_bind() {
    let _ = || -> OrmResult<()> {
        let user_id = 42_i64;

        // Verify raw_bind compiles on query builder
        let _q = CompileUser::query().raw_bind("(id = ? OR ? > 0)", vec![user_id, user_id]);

        Ok(())
    };
}

#[test]
fn compile_where_expr_raw_bind() {
    let _ = || -> OrmResult<()> {
        let expr = WhereExpr::raw_bind(
            "(user_id = ? OR ? = ANY(collaborators))",
            vec![42_i64, 42_i64],
        );
        let mut q = sql("SELECT * FROM users WHERE ");
        expr.append_to_sql(&mut q);
        let sql_str = q.to_sql();
        assert!(sql_str.contains("$1"), "should contain $1, got: {sql_str}");
        assert!(sql_str.contains("$2"), "should contain $2, got: {sql_str}");
        assert!(
            sql_str.contains("(user_id ="),
            "should contain template text, got: {sql_str}"
        );
        Ok(())
    };
}

#[test]
fn compile_ident_validation() {
    // Valid identifiers
    assert!(pgorm::Ident::parse("user_name").is_ok());
    assert!(pgorm::Ident::parse("created_at").is_ok());
    assert!(pgorm::Ident::parse("schema1").is_ok());

    // Invalid identifiers (contain special characters)
    assert!(pgorm::Ident::parse("name; DROP TABLE").is_err());
    assert!(pgorm::Ident::parse("col -- comment").is_err());
    assert!(pgorm::Ident::parse("").is_err());
}
