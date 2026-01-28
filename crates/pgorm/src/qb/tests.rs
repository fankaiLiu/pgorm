//! Integration tests for the qb module.

use crate::qb::expr::{Expr, ExprGroup};
use crate::qb::param::ParamList;
use crate::qb::{delete, insert, select, update};

#[test]
fn test_select_basic() {
    let qb = select("users");
    assert_eq!(qb.to_sql(), "SELECT * FROM users");
}

#[test]
fn test_select_with_conditions() {
    let qb = select("users")
        .eq("status", "active")
        .gt("age", 18i32)
        .limit(10);

    let sql = qb.to_sql();
    assert!(sql.contains("SELECT * FROM users"));
    assert!(sql.contains("WHERE"));
    assert!(sql.contains("status = $1"));
    assert!(sql.contains("age > $2"));
    assert!(sql.contains("LIMIT 10"));
}

#[test]
fn test_insert_basic() {
    let qb = insert("users")
        .set("username", "alice")
        .set("email", "alice@example.com");

    let sql = qb.to_sql();
    assert_eq!(sql, "INSERT INTO users (username, email) VALUES ($1, $2)");
}

#[test]
fn test_update_basic() {
    let qb = update("users")
        .set("status", "inactive")
        .eq("id", 1i64);

    let sql = qb.to_sql();
    assert_eq!(sql, "UPDATE users SET status = $1 WHERE id = $2");
}

#[test]
fn test_delete_basic() {
    let qb = delete("users").eq("id", 1i64);
    let sql = qb.to_sql();
    assert_eq!(sql, "DELETE FROM users WHERE id = $1");
}

#[test]
fn test_delete_safe_default() {
    let qb = delete("users");
    let sql = qb.to_sql();
    // Without WHERE, should generate safe no-op
    assert_eq!(sql, "DELETE FROM users WHERE 1=0");
}

#[test]
fn test_complex_where_expr() {
    // Test complex nested expressions
    let expr = Expr::and(vec![
        Expr::eq("status", "active"),
        Expr::or(vec![
            Expr::eq("role", "admin"),
            Expr::and(vec![
                Expr::eq("role", "user"),
                Expr::gt("reputation", 100i32),
            ]),
        ]),
    ]);

    let mut params = ParamList::new();
    let sql = expr.build(&mut params);

    // Should generate: status = $1 AND (role = $2 OR (role = $3 AND reputation > $4))
    assert!(sql.contains("status = $1"));
    assert!(sql.contains("role = $2 OR"));
    assert!(sql.contains("role = $3 AND reputation > $4"));
    assert_eq!(params.len(), 4);
}

#[test]
fn test_expr_group_multi_ilike() {
    let mut group = ExprGroup::new();
    group.multi_ilike(&["name", "email", "username"], "%test%");

    let (sql, params) = group.build();

    // Should generate OR group for all columns
    assert!(sql.contains("name ILIKE $1"));
    assert!(sql.contains("email ILIKE $2"));
    assert!(sql.contains("username ILIKE $3"));
    assert!(sql.contains(" OR "));
    assert_eq!(params.len(), 3);
}

#[test]
fn test_select_with_join() {
    let qb = select("users u")
        .select("u.*, COUNT(o.id) as order_count")
        .left_join("orders o", "u.id = o.user_id")
        .eq("u.status", "active")
        .group_by("u.id")
        .having_gt("COUNT(o.id)", 5i64)
        .order_by_desc("order_count")
        .limit(10);

    let sql = qb.to_sql();

    assert!(sql.contains("SELECT u.*, COUNT(o.id) as order_count"));
    assert!(sql.contains("FROM users u"));
    assert!(sql.contains("LEFT JOIN orders o ON u.id = o.user_id"));
    assert!(sql.contains("WHERE u.status = $1"));
    assert!(sql.contains("GROUP BY u.id"));
    assert!(sql.contains("HAVING COUNT(o.id) > $2"));
    assert!(sql.contains("ORDER BY order_count DESC"));
    assert!(sql.contains("LIMIT 10"));
}

#[test]
fn test_insert_on_conflict() {
    let qb = insert("users")
        .set("username", "alice")
        .set("email", "alice@example.com")
        .on_conflict("(username)")
        .do_update()
        .set_excluded("email")
        .finish()
        .returning("id");

    let sql = qb.to_sql();

    assert!(sql.contains("INSERT INTO users"));
    assert!(sql.contains("ON CONFLICT (username)"));
    assert!(sql.contains("DO UPDATE SET email = EXCLUDED.email"));
    assert!(sql.contains("RETURNING id"));
}

#[test]
fn test_update_with_complex_where() {
    let qb = update("products")
        .set("price", 99.99f64)
        .set_raw("updated_at", "NOW()")
        .eq("category", "electronics")
        .in_list("brand", vec!["Apple", "Samsung", "Google"])
        .gt("stock", 0i32)
        .returning("id, name, price");

    let sql = qb.to_sql();

    assert!(sql.contains("UPDATE products SET"));
    assert!(sql.contains("price = $1"));
    assert!(sql.contains("updated_at = NOW()"));
    assert!(sql.contains("WHERE"));
    assert!(sql.contains("category = $2"));
    assert!(sql.contains("brand IN ($3, $4, $5)"));
    assert!(sql.contains("stock > $6"));
    assert!(sql.contains("RETURNING id, name, price"));
}

#[test]
fn test_empty_in_list_semantics() {
    // Empty IN should generate FALSE (1=0)
    let qb = select("users").in_list::<i32>("id", vec![]);
    let sql = qb.to_sql();
    assert!(sql.contains("1=0"));

    // Empty NOT IN should generate TRUE (1=1)
    let qb = select("users").not_in::<i32>("id", vec![]);
    let sql = qb.to_sql();
    assert!(sql.contains("1=1"));
}

#[test]
fn test_between() {
    let qb = select("products")
        .between("price", 10.0f64, 100.0f64)
        .not_between("stock", 0i32, 5i32);

    let sql = qb.to_sql();
    assert!(sql.contains("price BETWEEN $1 AND $2"));
    assert!(sql.contains("stock NOT BETWEEN $3 AND $4"));
}

#[test]
fn test_optional_conditions() {
    let status: Option<&str> = Some("active");
    let name: Option<&str> = None;
    let min_age: Option<i32> = Some(18);

    let qb = select("users")
        .eq_opt("status", status)
        .eq_opt("name", name)
        .gte_opt("age", min_age);

    let sql = qb.to_sql();

    // status and age should be in the query, name should not
    assert!(sql.contains("status = $1"));
    assert!(sql.contains("age >= $2"));
    assert!(!sql.contains("name"));
}

#[test]
fn test_pagination() {
    let qb = select("users")
        .order_by("created_at DESC")
        .paginate(3, 25);

    let sql = qb.to_sql();
    assert!(sql.contains("LIMIT 25"));
    assert!(sql.contains("OFFSET 50")); // (3-1) * 25 = 50
}

#[test]
fn test_count_query() {
    let qb = select("users")
        .eq("status", "active")
        .gt("age", 18i32);

    let count_sql = qb.to_count_sql();
    assert!(count_sql.contains("SELECT COUNT(*)"));
    assert!(count_sql.contains("status = $1"));
    assert!(count_sql.contains("age > $2"));
    assert!(!count_sql.contains("LIMIT"));
    assert!(!count_sql.contains("OFFSET"));
}

#[test]
fn test_count_with_group_by() {
    let qb = select("orders")
        .select("user_id, COUNT(*) as order_count")
        .group_by("user_id")
        .having_gt("COUNT(*)", 5i64);

    let count_sql = qb.to_count_sql();

    // Should wrap in subquery for correct counting with GROUP BY
    assert!(count_sql.starts_with("SELECT COUNT(*) FROM ("));
    assert!(count_sql.contains("GROUP BY user_id"));
}
