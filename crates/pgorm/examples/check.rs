//! SQL checking and linting example
//!
//! Run with:
//! `cargo run --example check -p pgorm --features check`

use pgorm::{
    delete_has_where, detect_statement_kind, get_table_names, is_valid_sql, lint_select_many,
    lint_sql, select_has_limit, select_has_star, update_has_where, LintLevel, SchemaRegistry,
    StatementKind, TableMeta, TableSchema,
};

// Define some model structs with TableMeta
struct User;

impl TableMeta for User {
    fn table_name() -> &'static str {
        "users"
    }

    fn columns() -> &'static [&'static str] {
        &["id", "name", "email", "created_at"]
    }

    fn primary_key() -> Option<&'static str> {
        Some("id")
    }
}

struct Order;

impl TableMeta for Order {
    fn table_name() -> &'static str {
        "orders"
    }

    fn columns() -> &'static [&'static str] {
        &["id", "user_id", "total", "status", "created_at"]
    }

    fn primary_key() -> Option<&'static str> {
        Some("id")
    }
}

fn main() {
    println!("=== SQL Checking & Linting Example ===\n");

    // 1. Register tables with SchemaRegistry
    println!("1. Schema Registry");
    let mut registry = SchemaRegistry::new();
    registry.register::<User>();
    registry.register::<Order>();

    // Can also register tables manually
    registry.register_table(
        TableSchema::new("public", "products")
            .with_columns(&["id", "name", "price", "category_id"])
            .with_primary_key("id"),
    );

    println!("   Registered {} tables:", registry.len());
    for table in registry.tables() {
        println!("   - {}.{} ({} columns)", table.schema, table.name, table.columns.len());
    }
    println!();

    // 2. SQL Syntax Validation
    println!("2. SQL Syntax Validation");
    check_syntax("SELECT * FROM users");
    check_syntax("SELEC * FROM users");
    check_syntax("SELECT id, name FROM users WHERE id = $1");
    println!();

    // 3. Statement Type Detection
    println!("3. Statement Type Detection");
    detect_type("SELECT * FROM users");
    detect_type("INSERT INTO users (name) VALUES ('foo')");
    detect_type("UPDATE users SET name = 'bar' WHERE id = 1");
    detect_type("DELETE FROM users WHERE id = 1");
    detect_type("TRUNCATE users");
    println!();

    // 4. Schema Validation
    println!("4. Schema Validation (check tables exist)");
    check_schema(&registry, "SELECT * FROM users");
    check_schema(&registry, "SELECT * FROM users JOIN orders ON users.id = orders.user_id");
    check_schema(&registry, "SELECT * FROM nonexistent_table");
    check_schema(&registry, "SELECT * FROM users, products, missing_table");
    println!();

    // 5. SQL Linting
    println!("5. SQL Linting");
    lint("SELECT * FROM users");
    lint("SELECT id, name FROM users WHERE active = true");
    lint("DELETE FROM users");
    lint("DELETE FROM users WHERE id = 1");
    lint("UPDATE users SET name = 'foo'");
    lint("UPDATE users SET name = 'foo' WHERE id = 1");
    lint("TRUNCATE users");
    println!();

    // 6. Select Many Linting
    println!("6. Select Many Linting (for queries returning multiple rows)");
    lint_many("SELECT * FROM users");
    lint_many("SELECT * FROM users LIMIT 100");
    lint_many("SELECT id, name FROM users ORDER BY created_at DESC LIMIT 50");
    println!();

    // 7. Individual Checks
    println!("7. Individual Check Functions");
    println!("   select_has_limit('SELECT * FROM users'): {:?}", select_has_limit("SELECT * FROM users"));
    println!("   select_has_limit('SELECT * FROM users LIMIT 10'): {:?}", select_has_limit("SELECT * FROM users LIMIT 10"));
    println!("   select_has_star('SELECT * FROM users'): {:?}", select_has_star("SELECT * FROM users"));
    println!("   select_has_star('SELECT id FROM users'): {:?}", select_has_star("SELECT id FROM users"));
    println!("   delete_has_where('DELETE FROM users'): {:?}", delete_has_where("DELETE FROM users"));
    println!("   delete_has_where('DELETE FROM users WHERE id = 1'): {:?}", delete_has_where("DELETE FROM users WHERE id = 1"));
    println!("   update_has_where('UPDATE users SET x = 1'): {:?}", update_has_where("UPDATE users SET x = 1"));
    println!("   update_has_where('UPDATE users SET x = 1 WHERE id = 1'): {:?}", update_has_where("UPDATE users SET x = 1 WHERE id = 1"));
    println!();

    // 8. Table Name Extraction
    println!("8. Table Name Extraction");
    extract_tables("SELECT * FROM users");
    extract_tables("SELECT * FROM users u JOIN orders o ON u.id = o.user_id");
    extract_tables("SELECT * FROM public.users, public.orders");
    extract_tables("WITH active AS (SELECT * FROM users WHERE active) SELECT * FROM active");
}

fn check_syntax(sql: &str) {
    let result = is_valid_sql(sql);
    let status = if result.valid { "✓" } else { "✗" };
    println!("   {} {}", status, sql);
    if let Some(err) = result.error {
        println!("     Error: {}", err);
    }
}

fn detect_type(sql: &str) {
    let kind = detect_statement_kind(sql);
    let kind_str = match kind {
        Some(StatementKind::Select) => "SELECT",
        Some(StatementKind::Insert) => "INSERT",
        Some(StatementKind::Update) => "UPDATE",
        Some(StatementKind::Delete) => "DELETE",
        Some(StatementKind::Truncate) => "TRUNCATE",
        Some(k) => return println!("   {:?}: {}", k, sql),
        None => "Unknown",
    };
    println!("   {}: {}", kind_str, sql);
}

fn check_schema(registry: &SchemaRegistry, sql: &str) {
    let issues = registry.check_sql(sql);
    let status = if issues.is_empty() { "✓" } else { "✗" };
    println!("   {} {}", status, sql);
    for issue in &issues {
        println!("     {:?}: {}", issue.kind, issue.message);
    }
}

fn lint(sql: &str) {
    let result = lint_sql(sql);
    let status = if result.has_errors() {
        "✗"
    } else if result.has_warnings() {
        "⚠"
    } else if result.is_ok() {
        "✓"
    } else {
        "ℹ"
    };
    println!("   {} {}", status, sql);
    for issue in &result.issues {
        let level = match issue.level {
            LintLevel::Error => "ERROR",
            LintLevel::Warning => "WARN",
            LintLevel::Info => "INFO",
        };
        println!("     {} [{}] {}", level, issue.code, issue.message);
    }
}

fn lint_many(sql: &str) {
    let result = lint_select_many(sql);
    let status = if result.has_errors() {
        "✗"
    } else if result.has_warnings() {
        "⚠"
    } else {
        "✓"
    };
    println!("   {} {}", status, sql);
    for issue in &result.issues {
        let level = match issue.level {
            LintLevel::Error => "ERROR",
            LintLevel::Warning => "WARN",
            LintLevel::Info => "INFO",
        };
        println!("     {} [{}] {}", level, issue.code, issue.message);
    }
}

fn extract_tables(sql: &str) {
    let tables = get_table_names(sql);
    println!("   [{}] {}", tables.join(", "), sql);
}
