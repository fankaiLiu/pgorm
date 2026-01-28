//! Example demonstrating Model::check_schema() for automatic SQL validation
//!
//! Run with: cargo run --example model_check -p pgorm --features "derive,pool,check"
//!
//! This example shows how to use the auto-generated `generated_sql()` and `check_schema()`
//! methods to validate that a Model's SQL matches the registered schema.

use pgorm::{FromRow, Model, SchemaRegistry};

// ============================================
// Define models that match the schema
// ============================================

#[derive(Debug, FromRow, Model)]
#[orm(table = "users")]
#[allow(dead_code)]
struct User {
    #[orm(id)]
    id: i64,
    name: String,
    email: String,
    created_at: String,
}

#[derive(Debug, FromRow, Model)]
#[orm(table = "orders")]
#[allow(dead_code)]
struct Order {
    #[orm(id)]
    id: i64,
    user_id: i64,
    total: i64,
    status: String,
}

// ============================================
// Define a model with WRONG columns (mismatch)
// ============================================

#[derive(Debug, FromRow, Model)]
#[orm(table = "users")]
#[allow(dead_code)]
struct UserWithWrongColumns {
    #[orm(id)]
    id: i64,
    name: String,
    // These columns don't exist in the real schema!
    phone: String,          // wrong column
    address: String,        // wrong column
}

// ============================================
// Define a model pointing to non-existent table
// ============================================

#[derive(Debug, FromRow, Model)]
#[orm(table = "nonexistent_table")]
#[allow(dead_code)]
struct NonExistentModel {
    #[orm(id)]
    id: i64,
    data: String,
}

fn main() {
    println!("=== Model Schema Check Demo ===\n");

    // Create a registry with the "real" database schema
    // In production, this would be auto-populated from #[derive(Model)]
    // Here we manually create it to simulate the "expected" schema
    let mut registry = SchemaRegistry::new();

    // Register the correct schema for "users" table
    registry.register::<User>();
    // Register the correct schema for "orders" table
    registry.register::<Order>();

    println!("Registered tables in schema:");
    for table in registry.tables() {
        println!(
            "  - {}: {:?}",
            table.name,
            table.columns.iter().map(|c| &c.name).collect::<Vec<_>>()
        );
    }

    // ============================================
    // Example 1: Check a correct model
    // ============================================
    println!("\n--- Example 1: Correct Model (User) ---");

    println!("\nUser::generated_sql():");
    for (name, sql) in User::generated_sql() {
        println!("  {}: {}", name, sql);
    }

    let issues = User::check_schema(&registry);
    if issues.is_empty() {
        println!("\n✓ User model: All SQL checks passed!");
    } else {
        println!("\n✗ User model: Found issues:");
        for (name, issue_list) in &issues {
            println!("  {} ({} issues):", name, issue_list.len());
            for issue in issue_list {
                println!("    - {:?}: {}", issue.kind, issue.message);
            }
        }
    }

    // ============================================
    // Example 2: Check a model with wrong columns
    // ============================================
    println!("\n--- Example 2: Model with Wrong Columns ---");

    println!("\nUserWithWrongColumns::generated_sql():");
    for (name, sql) in UserWithWrongColumns::generated_sql() {
        println!("  {}: {}", name, sql);
    }

    let issues = UserWithWrongColumns::check_schema(&registry);
    if issues.is_empty() {
        println!("\n✓ UserWithWrongColumns: All SQL checks passed!");
    } else {
        println!("\n✗ UserWithWrongColumns: Found issues:");
        for (name, issue_list) in &issues {
            println!("  {} ({} issues):", name, issue_list.len());
            for issue in issue_list {
                println!("    - {:?}: {}", issue.kind, issue.message);
            }
        }
    }

    // ============================================
    // Example 3: Check a model with non-existent table
    // ============================================
    println!("\n--- Example 3: Model with Non-existent Table ---");

    println!("\nNonExistentModel::generated_sql():");
    for (name, sql) in NonExistentModel::generated_sql() {
        println!("  {}: {}", name, sql);
    }

    let issues = NonExistentModel::check_schema(&registry);
    if issues.is_empty() {
        println!("\n✓ NonExistentModel: All SQL checks passed!");
    } else {
        println!("\n✗ NonExistentModel: Found issues:");
        for (name, issue_list) in &issues {
            println!("  {} ({} issues):", name, issue_list.len());
            for issue in issue_list {
                println!("    - {:?}: {}", issue.kind, issue.message);
            }
        }
    }

    // ============================================
    // Example 4: Check all models at startup
    // ============================================
    println!("\n--- Example 4: Check All Models at Startup ---");

    fn check_all_models(registry: &SchemaRegistry) -> bool {
        let mut all_ok = true;

        // Check each model
        let models: Vec<(&str, std::collections::HashMap<&str, Vec<pgorm::SchemaIssue>>)> = vec![
            ("User", User::check_schema(registry)),
            ("Order", Order::check_schema(registry)),
            ("UserWithWrongColumns", UserWithWrongColumns::check_schema(registry)),
            ("NonExistentModel", NonExistentModel::check_schema(registry)),
        ];

        for (model_name, issues) in models {
            if issues.is_empty() {
                println!("  ✓ {}", model_name);
            } else {
                all_ok = false;
                let total_issues: usize = issues.values().map(|v| v.len()).sum();
                println!("  ✗ {} ({} issues)", model_name, total_issues);
            }
        }

        all_ok
    }

    println!("\nModel validation results:");
    let all_valid = check_all_models(&registry);

    if all_valid {
        println!("\n✓ All models are valid!");
    } else {
        println!("\n✗ Some models have schema issues!");
    }

    println!("\n=== Done ===");
}
