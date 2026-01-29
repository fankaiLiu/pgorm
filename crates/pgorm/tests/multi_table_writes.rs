//! Compile-time tests for multi-table write (graph) macros.
//!
//! These tests verify that the macro-generated code compiles correctly.
//! They do not run actual database operations.

use pgorm::{FromRow, InsertModel, Model, UpdateModel, WriteReport, WriteStepReport};

// ============================================
// Test: Basic InsertModel with has_many
// ============================================

#[derive(Debug, FromRow, Model)]
#[orm(table = "orders")]
struct Order {
    #[orm(id)]
    id: i64,
    user_id: i64,
    total_cents: i64,
}

#[derive(Clone, InsertModel)]
#[orm(table = "order_items")]
struct NewOrderItem {
    order_id: Option<i64>,
    sku: String,
    qty: i32,
}

// Note: has_many requires returning type to get root ID for FK injection
// This test verifies the macro compiles for the basic InsertModel without graph
#[derive(InsertModel)]
#[orm(table = "orders")]
struct NewOrderBasic {
    user_id: i64,
    total_cents: i64,
}

#[test]
fn test_insert_model_basic_compiles() {
    let _order = NewOrderBasic {
        user_id: 1,
        total_cents: 1000,
    };

    assert_eq!(NewOrderBasic::TABLE, "orders");
    assert_eq!(NewOrderItem::TABLE, "order_items");
}

// ============================================
// Test: InsertModel with has_one (requires returning)
// ============================================

#[derive(Debug, FromRow, Model)]
#[orm(table = "users")]
struct User {
    #[orm(id)]
    id: i64,
    username: String,
}

#[derive(Clone, InsertModel)]
#[orm(table = "user_profiles")]
struct NewUserProfile {
    user_id: Option<i64>,
    bio: String,
}

// Basic user without graph
#[derive(InsertModel)]
#[orm(table = "users")]
struct NewUserBasic {
    username: String,
}

#[test]
fn test_insert_model_user_basic_compiles() {
    let _user = NewUserBasic {
        username: "alice".into(),
    };

    assert_eq!(NewUserBasic::TABLE, "users");
}

// ============================================
// Test: InsertModel with belongs_to (pre-insert dependency)
// ============================================

#[derive(Debug, FromRow, Model)]
#[orm(table = "categories")]
struct Category {
    #[orm(id)]
    id: i64,
    name: String,
}

#[derive(Clone, InsertModel)]
#[orm(table = "categories", returning = "Category")]
struct NewCategory {
    name: String,
}

#[derive(Debug, FromRow, Model)]
#[orm(table = "products")]
struct Product {
    #[orm(id)]
    id: i64,
    name: String,
    category_id: Option<i64>,
}

// Product with belongs_to for category
#[derive(InsertModel)]
#[orm(table = "products", returning = "Product")]
#[orm(graph_root_key = "id")]
#[orm(belongs_to(NewCategory, field = "category", set_fk_field = "category_id", mode = "insert_returning", required = false))]
struct NewProduct {
    name: String,
    category_id: Option<i64>,
    category: Option<NewCategory>,
}

#[test]
fn test_insert_model_with_belongs_to_compiles() {
    // Use existing category_id
    let _product1 = NewProduct {
        name: "Widget".into(),
        category_id: Some(1),
        category: None,
    };

    // Create new category via belongs_to
    let _product2 = NewProduct {
        name: "Gadget".into(),
        category_id: None,
        category: Some(NewCategory {
            name: "Electronics".into(),
        }),
    };

    assert_eq!(NewProduct::TABLE, "products");
}

// ============================================
// Test: InsertModel with before/after_insert steps
// ============================================

#[derive(Clone, InsertModel)]
#[orm(table = "audit_logs")]
struct NewAuditLog {
    action: String,
    entity: String,
}

#[derive(InsertModel)]
#[orm(table = "users", returning = "User")]
#[orm(graph_root_key = "id")]
#[orm(after_insert(NewAuditLog, field = "audit", mode = "insert"))]
struct NewUserWithAudit {
    username: String,
    audit: Option<NewAuditLog>,
}

#[test]
fn test_insert_model_with_after_insert_compiles() {
    let _user = NewUserWithAudit {
        username: "bob".into(),
        audit: Some(NewAuditLog {
            action: "CREATE".into(),
            entity: "user".into(),
        }),
    };

    assert_eq!(NewUserWithAudit::TABLE, "users");
}

// ============================================
// Test: InsertModel with conflict_target (custom upsert key)
// ============================================

#[derive(Clone, InsertModel)]
#[orm(table = "tags", conflict_target = "name")]
struct NewTag {
    name: String,
    color: Option<String>,
}

#[test]
fn test_insert_model_with_conflict_target_compiles() {
    let _tag = NewTag {
        name: "rust".into(),
        color: Some("orange".into()),
    };

    assert_eq!(NewTag::TABLE, "tags");
}

// ============================================
// Test: Basic UpdateModel (without graph)
// ============================================

#[derive(UpdateModel)]
#[orm(table = "orders", model = "Order", returning = "Order")]
struct OrderPatchBasic {
    total_cents: Option<i64>,
}

#[test]
fn test_update_model_basic_compiles() {
    let _patch = OrderPatchBasic {
        total_cents: Some(1500),
    };

    assert_eq!(OrderPatchBasic::TABLE, "orders");
}

// ============================================
// Test: UpdateModel for users
// ============================================

#[derive(UpdateModel)]
#[orm(table = "users", model = "User", returning = "User")]
struct UserPatchBasic {
    username: Option<String>,
}

#[test]
fn test_update_model_user_basic_compiles() {
    let _patch = UserPatchBasic {
        username: Some("new_name".into()),
    };

    assert_eq!(UserPatchBasic::TABLE, "users");
}

// ============================================
// Test: WriteReport type
// ============================================

#[test]
fn test_write_report_type_exists() {
    let report: WriteReport<i32> = WriteReport {
        affected: 5,
        steps: vec![
            WriteStepReport {
                tag: "graph:root:test",
                affected: 1,
            },
            WriteStepReport {
                tag: "graph:has_many:items",
                affected: 4,
            },
        ],
        root: Some(42),
    };

    assert_eq!(report.affected, 5);
    assert_eq!(report.steps.len(), 2);
    assert_eq!(report.root, Some(42));
}

// ============================================
// Test: InsertModel with has_many (with returning for FK injection)
// ============================================

#[derive(InsertModel)]
#[orm(table = "orders", returning = "Order")]
#[orm(graph_root_key = "id")]
#[orm(has_many(NewOrderItem, field = "items", fk_field = "order_id", fk_wrap = "some"))]
struct NewOrderWithItems {
    user_id: i64,
    total_cents: i64,
    items: Option<Vec<NewOrderItem>>,
}

#[test]
fn test_insert_model_with_has_many_compiles() {
    let _order = NewOrderWithItems {
        user_id: 1,
        total_cents: 1000,
        items: Some(vec![
            NewOrderItem {
                order_id: None,
                sku: "A".into(),
                qty: 1,
            },
            NewOrderItem {
                order_id: None,
                sku: "B".into(),
                qty: 2,
            },
        ]),
    };

    assert_eq!(NewOrderWithItems::TABLE, "orders");
}

// ============================================
// Test: InsertModel with has_one
// ============================================

#[derive(InsertModel)]
#[orm(table = "users", returning = "User")]
#[orm(graph_root_key = "id")]
#[orm(has_one(NewUserProfile, field = "profile", fk_field = "user_id", fk_wrap = "some"))]
struct NewUserWithProfile {
    username: String,
    profile: Option<NewUserProfile>,
}

#[test]
fn test_insert_model_with_has_one_compiles() {
    let _user = NewUserWithProfile {
        username: "alice".into(),
        profile: Some(NewUserProfile {
            user_id: None,
            bio: "Hello world".into(),
        }),
    };

    assert_eq!(NewUserWithProfile::TABLE, "users");
}
