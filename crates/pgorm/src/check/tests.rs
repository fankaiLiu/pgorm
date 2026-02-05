use super::*;

struct TestUser;

impl TableMeta for TestUser {
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

struct TestOrder;

impl TableMeta for TestOrder {
    fn table_name() -> &'static str {
        "orders"
    }

    fn columns() -> &'static [&'static str] {
        &["id", "user_id", "total", "status"]
    }

    fn primary_key() -> Option<&'static str> {
        Some("id")
    }
}

#[test]
fn test_register_table() {
    let mut registry = SchemaRegistry::new();
    registry.register::<TestUser>();
    registry.register::<TestOrder>();

    assert_eq!(registry.len(), 2);
    assert!(registry.has_table("public", "users"));
    assert!(registry.has_table("public", "orders"));
    assert!(!registry.has_table("public", "products"));
}

#[test]
fn test_find_table() {
    let mut registry = SchemaRegistry::new();
    registry.register::<TestUser>();

    let table = registry.find_table("users").unwrap();
    assert_eq!(table.name, "users");
    assert!(table.has_column("id"));
    assert!(table.has_column("name"));
    assert!(!table.has_column("nonexistent"));
}

#[test]
fn test_table_schema_builder() {
    let table = TableSchema::new("public", "products")
        .with_columns(&["id", "name", "price"])
        .with_primary_key("id");

    assert_eq!(table.name, "products");
    assert!(table.has_column("id"));
    assert!(table.has_column("name"));
    assert!(table.has_column("price"));

    let pk_col = table.columns.iter().find(|c| c.is_primary_key).unwrap();
    assert_eq!(pk_col.name, "id");
}

#[cfg(feature = "check")]
mod check_tests {
    use super::*;

    #[test]
    fn test_is_valid_sql() {
        assert!(is_valid_sql("SELECT * FROM users").valid);
        assert!(!is_valid_sql("SELEC * FROM users").valid);
    }

    #[test]
    fn test_detect_statement_kind() {
        assert_eq!(
            detect_statement_kind("SELECT * FROM users"),
            Some(StatementKind::Select)
        );
        assert_eq!(
            detect_statement_kind("DELETE FROM users"),
            Some(StatementKind::Delete)
        );
        assert_eq!(
            detect_statement_kind("UPDATE users SET name = 'foo'"),
            Some(StatementKind::Update)
        );
    }

    #[test]
    fn test_lint_sql() {
        let result = lint_sql("DELETE FROM users");
        assert!(result.has_errors());

        let result = lint_sql("DELETE FROM users WHERE id = 1");
        assert!(!result.has_errors());
    }

    #[test]
    fn test_check_sql_schema() {
        let mut registry = SchemaRegistry::new();
        registry.register::<TestUser>();
        registry.register::<TestOrder>();

        // Valid SQL - tables exist
        let issues = registry.check_sql("SELECT * FROM users");
        assert!(issues.is_empty());

        // Invalid SQL - table doesn't exist
        let issues = registry.check_sql("SELECT * FROM products");
        assert!(!issues.is_empty());
        assert!(matches!(issues[0].kind, SchemaIssueKind::MissingTable));
    }

    #[test]
    fn test_check_sql_alias_and_ambiguous_column() {
        let mut registry = SchemaRegistry::new();
        registry.register::<TestUser>();
        registry.register::<TestOrder>();

        // Alias-qualified columns should resolve via FROM/JOIN alias mapping.
        let issues = registry.check_sql(
            "SELECT u.id FROM users u JOIN orders o ON u.id = o.user_id WHERE o.status = 'paid'",
        );
        assert!(issues.is_empty());

        // Unqualified `id` is ambiguous across `users` and `orders`.
        let issues = registry.check_sql("SELECT id FROM users u JOIN orders o ON u.id = o.user_id");
        assert!(
            issues
                .iter()
                .any(|i| i.kind == SchemaIssueKind::AmbiguousColumn)
        );
    }

    #[test]
    fn test_check_sql_insert_update_on_conflict_columns() {
        let mut registry = SchemaRegistry::new();
        registry.register::<TestUser>();

        // INSERT column list should be validated against the target table.
        let issues = registry.check_sql("INSERT INTO users (id, missing_col) VALUES (1, 'x')");
        assert!(
            issues
                .iter()
                .any(|i| i.kind == SchemaIssueKind::MissingColumn)
        );

        // UPDATE SET column list should be validated against the target table.
        let issues = registry.check_sql("UPDATE users SET missing_col = 1 WHERE id = 1");
        assert!(
            issues
                .iter()
                .any(|i| i.kind == SchemaIssueKind::MissingColumn)
        );

        // ON CONFLICT inference / DO UPDATE SET columns should be validated too.
        let issues = registry.check_sql(
            "INSERT INTO users (id, name) VALUES (1, 'a') ON CONFLICT (id) DO UPDATE SET missing_col = EXCLUDED.name",
        );
        assert!(
            issues
                .iter()
                .any(|i| i.kind == SchemaIssueKind::MissingColumn)
        );
    }

    #[test]
    fn test_check_sql_allows_cte_qualifiers() {
        let mut registry = SchemaRegistry::new();
        registry.register::<TestUser>();

        let issues = registry
            .check_sql("WITH inserted AS (SELECT * FROM users) SELECT inserted.id FROM inserted");
        assert!(issues.is_empty());
    }

    #[test]
    fn test_check_sql_allows_system_columns() {
        let mut registry = SchemaRegistry::new();
        registry.register::<TestUser>();

        // System columns exist on every table (even if they aren't modeled).
        let issues = registry.check_sql("SELECT ctid FROM users");
        assert!(issues.is_empty());

        // Validate INSERT/UPDATE paths also skip system columns.
        let issues = registry.check_sql("INSERT INTO users (ctid) VALUES ('(0,0)')");
        assert!(issues.is_empty());

        let issues = registry.check_sql("UPDATE users SET ctid = ctid");
        assert!(issues.is_empty());
    }
}
