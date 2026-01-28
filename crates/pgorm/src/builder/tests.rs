use super::*;

#[test]
fn test_simple_select() {
    let qb = QueryBuilder::new("users");
    assert_eq!(qb.to_sql(), "SELECT * FROM users");
}

#[test]
fn test_select_columns() {
    let mut qb = QueryBuilder::new("users");
    qb.select("id, username, email");
    assert_eq!(qb.to_sql(), "SELECT id, username, email FROM users");
}

#[test]
fn test_join() {
    let mut qb = QueryBuilder::new("users u");
    qb.select("u.*, r.name as role_name")
        .left_join("roles r", "u.role_id = r.id");
    assert_eq!(
        qb.to_sql(),
        "SELECT u.*, r.name as role_name FROM users u LEFT JOIN roles r ON u.role_id = r.id"
    );
}

#[test]
fn test_where_conditions() {
    let mut qb = QueryBuilder::new("users");
    qb.and_eq("status", "active").and_eq("role_id", 1);
    assert_eq!(
        qb.to_sql(),
        "SELECT * FROM users WHERE status = $1 AND role_id = $2"
    );
    // params checks are harder since we can't easily access private params unless exposed
    // But SqlBuilder exposes params_ref()
    assert_eq!(qb.params_ref().len(), 2);
}

#[test]
fn test_and_in() {
    let mut qb = QueryBuilder::new("users");
    qb.and_in("role_id", vec![1, 2, 3]);
    assert_eq!(
        qb.to_sql(),
        "SELECT * FROM users WHERE role_id IN ($1, $2, $3)"
    );
}

#[test]
fn test_and_in_empty() {
    let mut qb = QueryBuilder::new("users");
    qb.and_in("role_id", Vec::<i32>::new());
    assert_eq!(qb.to_sql(), "SELECT * FROM users WHERE 1=0");
}

#[test]
fn test_and_where_custom() {
    let mut qb = QueryBuilder::new("users");
    qb.and_where("a = ? OR b = ?", vec![1, 2]);
    assert_eq!(qb.to_sql(), "SELECT * FROM users WHERE (a = $1 OR b = $2)");
}

#[test]
fn test_order_and_pagination() {
    let mut qb = QueryBuilder::new("users");
    qb.order_by("created_at DESC").paginate(2, 20);
    assert_eq!(
        qb.to_sql(),
        "SELECT * FROM users ORDER BY created_at DESC LIMIT 20 OFFSET 20"
    );
}

#[test]
fn test_count_sql() {
    let mut qb = QueryBuilder::new("users");
    qb.and_eq("status", "active")
        .order_by("created_at DESC")
        .paginate(1, 20);
    assert_eq!(
        qb.to_count_sql(),
        "SELECT COUNT(*) FROM users WHERE status = $1"
    );
}

#[test]
fn test_complex_query() {
    let mut qb = QueryBuilder::new("users u");
    qb.select("u.id, u.username, r.name as role")
        .left_join("roles r", "u.role_id = r.id")
        .and_eq("u.status", "active")
        .and_ilike("u.username", "%admin%")
        .and_is_null("u.deleted_at")
        .order_by("u.created_at DESC")
        .paginate(1, 20);

    let sql = qb.to_sql();
    assert!(sql.contains("SELECT u.id, u.username, r.name as role"));
    assert!(sql.contains("LEFT JOIN roles r ON u.role_id = r.id"));
    assert!(sql.contains("WHERE u.status = $1 AND u.username ILIKE $2 AND u.deleted_at IS NULL"));
    assert!(sql.contains("ORDER BY u.created_at DESC"));
    assert!(sql.contains("LIMIT 20 OFFSET 0"));
}

#[test]
fn test_multi_ilike() {
    let mut qb = QueryBuilder::new("users");
    qb.and_multi_ilike(&["username", "email", "full_name"], "%test%");
    assert_eq!(
        qb.to_sql(),
        "SELECT * FROM users WHERE (username ILIKE $1 OR email ILIKE $2 OR full_name ILIKE $3)"
    );
}

#[test]
fn test_group_by_having() {
    let mut qb = QueryBuilder::new("orders");
    qb.select("user_id, COUNT(*) as order_count")
        .group_by("user_id")
        .having("COUNT(*) > ?", 5);
    assert_eq!(
        qb.to_sql(),
        "SELECT user_id, COUNT(*) as order_count FROM orders GROUP BY user_id HAVING COUNT(*) > $1"
    );
}

#[test]
fn test_between() {
    let mut qb = QueryBuilder::new("orders");
    qb.and_between("amount", 100, 500);
    assert_eq!(
        qb.to_sql(),
        "SELECT * FROM orders WHERE amount BETWEEN $1 AND $2"
    );
}

#[test]
fn test_select_cols_array() {
    const USER_COLS: &[&str] = &["id", "username", "email", "created_at"];
    let mut qb = QueryBuilder::new("users");
    qb.select_cols(USER_COLS);
    assert_eq!(
        qb.to_sql(),
        "SELECT id, username, email, created_at FROM users"
    );
}

#[test]
fn test_add_select() {
    let mut qb = QueryBuilder::new("users");
    qb.add_select("id").add_select("username");
    assert_eq!(qb.to_sql(), "SELECT id, username FROM users");
}

#[test]
fn test_add_select_cols() {
    const BASE_COLS: &[&str] = &["id", "name"];
    let mut qb = QueryBuilder::new("users");
    qb.select_cols(BASE_COLS)
        .add_select_cols(&["created_at", "updated_at"]);
    assert_eq!(
        qb.to_sql(),
        "SELECT id, name, created_at, updated_at FROM users"
    );
}

// ==================== InsertBuilder Tests ====================

#[test]
fn test_insert_basic() {
    let mut ib = InsertBuilder::new("users");
    ib.set("username", "test").set("age", 25);
    assert_eq!(
        ib.to_sql(),
        "INSERT INTO users (username, age) VALUES ($1, $2)"
    );
}

#[test]
fn test_insert_with_uuidv7() {
    let mut ib = InsertBuilder::new("users");
    ib.set_uuidv7("id", None::<uuid::Uuid>)
        .set("username", "test");
    assert_eq!(
        ib.to_sql(),
        "INSERT INTO users (id, username) VALUES (COALESCE($1, uuidv7()), $2)"
    );
}

#[test]
fn test_insert_with_returning() {
    const USER_COLS: &[&str] = &["id", "username", "created_at"];
    let mut ib = InsertBuilder::new("users");
    ib.set("username", "test").returning_cols(USER_COLS);
    assert_eq!(
        ib.to_sql(),
        "INSERT INTO users (username) VALUES ($1) RETURNING id, username, created_at"
    );
}

#[test]
fn test_insert_with_on_conflict() {
    let mut ib = InsertBuilder::new("users");
    ib.set("email", "test@example.com")
        .on_conflict("(email)")
        .do_nothing();
    assert_eq!(
        ib.to_sql(),
        "INSERT INTO users (email) VALUES ($1) ON CONFLICT (email) DO NOTHING"
    );
}

#[test]
fn test_insert_on_conflict_do_update() {
    let mut ib = InsertBuilder::new("users");
    ib.set("email", "test@example.com")
        .on_conflict("(email)")
        .do_update()
        .set_raw("login_count", "users.login_count + 1")
        .set("last_login", "2024-01-01")
        .and_eq("status", "active");

    // Expected SQL:
    // INSERT params: $1 (email)
    // UPDATE params starts at offset 1:
    //   login_count = users.login_count + 1 (Raw)
    //   last_login = $2
    //   WHERE status = $3

    assert_eq!(
        ib.to_sql(),
        "INSERT INTO users (email) VALUES ($1) ON CONFLICT (email) DO UPDATE SET login_count = users.login_count + 1, last_login = $2 WHERE status = $3"
    );

    // Check params
    let params = ib.params_ref();
    assert_eq!(params.len(), 3);
}

#[test]
fn test_insert_set_raw() {
    let mut ib = InsertBuilder::new("users");
    ib.set("username", "test").set_raw("created_at", "NOW()");
    assert_eq!(
        ib.to_sql(),
        "INSERT INTO users (username, created_at) VALUES ($1, NOW())"
    );
}

#[test]
fn test_insert_unnest() {
    let mut ib = InsertBuilder::new("users");
    ib.unnest_list("username", vec!["Alice", "Bob"])
        .unnest_list("age", vec![25, 30]);

    assert_eq!(
        ib.to_sql(),
        "INSERT INTO users (username, age) SELECT * FROM UNNEST($1, $2)"
    );
}

// ==================== UpdateBuilder Tests ====================

#[test]
fn test_update_basic() {
    let mut ub = UpdateBuilder::new("users");
    ub.set("username", "new_name").and_eq("id", 1);
    assert_eq!(ub.to_sql(), "UPDATE users SET username = $1 WHERE id = $2");
}

#[test]
fn test_update_with_returning() {
    let mut ub = UpdateBuilder::new("users");
    ub.set("username", "new_name")
        .and_eq("id", 1)
        .returning("id, username");
    assert_eq!(
        ub.to_sql(),
        "UPDATE users SET username = $1 WHERE id = $2 RETURNING id, username"
    );
}

#[test]
fn test_update_set_raw() {
    let mut ub = UpdateBuilder::new("users");
    ub.set_raw("updated_at", "NOW()").and_eq("id", 1);
    assert_eq!(
        ub.to_sql(),
        "UPDATE users SET updated_at = NOW() WHERE id = $1"
    );
}

// ==================== DeleteBuilder Tests ====================

#[test]
fn test_delete_basic() {
    let mut db = DeleteBuilder::new("users");
    db.and_eq("id", 1);
    assert_eq!(db.to_sql(), "DELETE FROM users WHERE id = $1");
}

#[test]
fn test_delete_with_in() {
    let mut db = DeleteBuilder::new("users");
    db.and_in("id", vec![1, 2, 3]);
    assert_eq!(db.to_sql(), "DELETE FROM users WHERE id IN ($1, $2, $3)");
}

#[test]
fn test_delete_with_returning() {
    let mut db = DeleteBuilder::new("users");
    db.and_eq("id", 1).returning("id, username");
    assert_eq!(
        db.to_sql(),
        "DELETE FROM users WHERE id = $1 RETURNING id, username"
    );
}

#[test]
fn test_count_with_group_by_subquery() {
    let mut qb = QueryBuilder::new("orders");
    qb.select("user_id, COUNT(*) as c")
        .group_by("user_id")
        .having("COUNT(*) > ?", 5);

    // Existing to_sql checks normal query
    assert_eq!(
        qb.to_sql(),
        "SELECT user_id, COUNT(*) as c FROM orders GROUP BY user_id HAVING COUNT(*) > $1"
    );

    // New to_count_sql checks subquery structure
    assert_eq!(
        qb.to_count_sql(),
        "SELECT COUNT(*) FROM (SELECT 1 FROM orders GROUP BY user_id HAVING COUNT(*) > $1) AS t"
    );
}

#[test]
fn test_and_where_param_mismatch_no_panic() {
    let mut qb = QueryBuilder::new("users");
    // 2 ? placeholders, 1 value -> mismatch
    qb.and_where("a = ? AND b = ?", vec![1]);
    // Should not panic, but sets error state.
}

#[test]
fn test_and_where_param_mismatch_too_many_no_panic() {
    let mut qb = QueryBuilder::new("users");
    // 1 ? placeholder, 2 values -> mismatch
    qb.and_where("a = ?", vec![1, 2]);
    // Should not panic.
}

// ==================== Table Struct Tests ====================

#[test]
fn test_table_struct() {
    const USERS: Table = Table::new("users")
        .with_select_cols(&["id", "username"])
        .with_returning_cols(&["id", "created_at"]);

    // Test select (should use select_cols)
    let qb = USERS.select();
    assert_eq!(qb.to_sql(), "SELECT id, username FROM users");

    // Test insert (should use returning_cols)
    let mut ib = USERS.insert();
    ib.set("username", "test");
    assert_eq!(
        ib.to_sql(),
        "INSERT INTO users (username) VALUES ($1) RETURNING id, created_at"
    );

    // Test update (should use returning_cols)
    let mut ub = USERS.update();
    ub.set("username", "updated").and_eq("id", 1);
    assert_eq!(
        ub.to_sql(),
        "UPDATE users SET username = $1 WHERE id = $2 RETURNING id, created_at"
    );

    // Test delete (should use returning_cols)
    let mut db = USERS.delete();
    db.and_eq("id", 1);
    assert_eq!(
        db.to_sql(),
        "DELETE FROM users WHERE id = $1 RETURNING id, created_at"
    );

    // Test count
    let count_qb = USERS.count();
    assert_eq!(count_qb.to_count_sql(), "SELECT COUNT(*) FROM users");
}

#[derive(serde::Serialize)]
struct TestJson {
    foo: String,
    bar: i32,
}

#[test]
fn test_insert_json() -> serde_json::Result<()> {
    let mut ib = InsertBuilder::new("users");
    let data = TestJson {
        foo: "hello".to_string(),
        bar: 42,
    };
    ib.set_json("metadata", &data)?;

    assert_eq!(ib.to_sql(), "INSERT INTO users (metadata) VALUES ($1)");
    // params check would require downcasting or similar, but basic SQL check confirms structure
    Ok(())
}

#[test]
fn test_update_json() -> serde_json::Result<()> {
    let mut ub = UpdateBuilder::new("users");
    let data = TestJson {
        foo: "world".to_string(),
        bar: 100,
    };
    ub.set_json("metadata", &data)?.and_eq("id", 1);

    assert_eq!(ub.to_sql(), "UPDATE users SET metadata = $1 WHERE id = $2");
    Ok(())
}

#[test]
fn test_table_id_ops() {
    // Default id_col is "id"
    const USERS: Table = Table::new("users");

    // Restore delete test
    let db = USERS.delete_by_id(100);
    assert_eq!(db.to_sql(), "DELETE FROM users WHERE id = $1");

    let mut ub = USERS.update_by_id(100);
    ub.set("status", "active");
    // SET params ($1) come before WHERE params ($2) in generation order
    assert_eq!(ub.to_sql(), "UPDATE users SET status = $1 WHERE id = $2");

    // Empty update should fail
    let ub_empty = USERS.update_by_id(100);
    assert!(ub_empty.to_sql().contains("_error_no_set_fields"));
}

#[test]
fn test_table_custom_id() {
    const ORDERS: Table = Table::new("orders").with_id_col("order_id");

    let db = ORDERS.delete_by_id("abc");
    assert_eq!(db.to_sql(), "DELETE FROM orders WHERE order_id = $1");

    let mut ub = ORDERS.update_by_id("abc");
    ub.set("status", "shipped");
    assert_eq!(
        ub.to_sql(),
        "UPDATE orders SET status = $1 WHERE order_id = $2"
    );
}

#[test]
fn test_table_insert_with_json() -> serde_json::Result<()> {
    const EVENTS: Table = Table::new("events");
    let data = TestJson {
        foo: "login".to_string(),
        bar: 1,
    };

    let ib = EVENTS.insert_with_json("payload", &data)?;

    assert_eq!(ib.to_sql(), "INSERT INTO events (payload) VALUES ($1)");
    Ok(())
}

#[test]
fn test_delete_all_safety() {
    let mut db = DeleteBuilder::new("users");
    // Default: Safe no-op
    assert_eq!(db.to_sql(), "DELETE FROM users WHERE 1=0");

    db.allow_delete_all(true);
    assert_eq!(db.to_sql(), "DELETE FROM users");
}

#[test]
fn test_update_empty_safety() {
    let ub = UpdateBuilder::new("users");
    let sql = ub.to_sql();
    assert!(sql.contains("_error_no_set_fields = 1 WHERE 1=0"));
}

#[test]
fn test_and_where_param_mismatch_check() {
    let mut qb = QueryBuilder::new("users");
    // 2 ? placeholders, 1 value -> mismatch
    qb.and_where("a = ? AND b = ?", vec![1]);

    // Should NOT panic, but validate() should fail
    assert!(qb.validate().is_err());
}

#[test]
fn test_and_where_param_mismatch_too_many_check() {
    let mut qb = QueryBuilder::new("users");
    // 1 ? placeholder, 2 values -> mismatch
    qb.and_where("a = ?", vec![1, 2]);

    assert!(qb.validate().is_err());
}

// ... existing tests ...

#[test]
fn test_insert_conflict_update_validation() {
    let mut ib = InsertBuilder::new("users");
    ib.set("col", 1);

    // Empty update builder in DoUpdate
    ib.on_conflict("(id)").do_update();

    // Validation should fail recursively
    let result = ib.validate();
    let Err(crate::error::OrmError::Validation(message)) = result else {
        panic!("Expected validation error");
    };
    assert!(message.contains("UpdateBuilder: SET clause cannot be empty"));
}

#[test]
fn test_insert_unnest_mixed_validation() {
    let mut ib = InsertBuilder::new("users");
    // Mixed usage: unnest_list AND set/set_raw
    ib.unnest_list("col1", vec![1, 2]);
    ib.set("col2", 3); // invalid

    let result = ib.validate();
    assert!(result.is_err());
    assert!(
        matches!(result, Err(crate::error::OrmError::Validation(message)) if message.contains("cannot mix"))
    );
}

#[test]
fn test_having_mismatch() {
    let mut qb = QueryBuilder::new("users");
    // Mismatch: 2 ? but 1 value
    qb.having("count > ? AND sum < ?", 10);

    assert!(qb.validate().is_err());
}

#[test]
fn test_paginate_clamp() {
    let mut qb = QueryBuilder::new("users");
    // Invalid page/per_page should be clamped
    qb.paginate(0, -5);

    // limit should be 1, offset should be 0
    let sql = qb.to_sql();
    assert!(sql.contains("LIMIT 1 OFFSET 0"));
}

#[test]
fn test_count_consistency() {
    let mut qb = QueryBuilder::new("users");
    qb.group_by("id");

    // build_count() should now use to_count_sql logic (wrapping subquery)
    let count_query = qb.build_count();
    let sql = count_query.sql();

    assert!(sql.contains("SELECT COUNT(*) FROM (SELECT 1 FROM users"));
}

#[test]
fn test_insert_default_values() {
    let ib = InsertBuilder::new("users");
    // No cols set
    assert_eq!(ib.to_sql(), "INSERT INTO users DEFAULT VALUES");
}

#[test]
fn test_insert_unnest_validation() {
    let ib = InsertBuilder::new("users");
    // Standard usage should be valid
    assert!(ib.validate().is_ok());
}

#[test]
fn test_update_validation_empty() {
    let ub = UpdateBuilder::new("users");
    // No set fields
    let result = ub.validate();
    assert!(match result {
        Err(crate::error::OrmError::Validation(message)) => message.contains("cannot be empty"),
        _ => false,
    });
}

#[test]
fn test_and_where_mismatch_state_preservation() {
    let mut qb = QueryBuilder::new("users");
    qb.and_eq("id", 1);

    // Mismatch: 1 placeholder, 2 params
    qb.and_where("status = ?", vec![1, 2]);

    // Should NOT have added the broken where clause
    assert_eq!(qb.to_sql(), "SELECT * FROM users WHERE id = $1");
    // Should refer to params correctly
    assert_eq!(qb.params_ref().len(), 1);

    // Error should be set (though we can't inspect private field easily, validate() should fail)
    let res = qb.validate();
    assert!(res.is_err());
}

#[test]
fn test_update_is_null() {
    let mut ub = UpdateBuilder::new("users");
    ub.set("status", "inactive")
        .and_is_null("deleted_at")
        .and_is_not_null("created_at");

    assert_eq!(
        ub.to_sql(),
        "UPDATE users SET status = $1 WHERE deleted_at IS NULL AND created_at IS NOT NULL"
    );
}
