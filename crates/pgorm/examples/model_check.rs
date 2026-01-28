//! Example demonstrating Model::check_schema() with macros
//!
//! Run with: cargo run --example model_check -p pgorm --features "derive,check"
//!
//! This example shows how to use macros to simplify schema validation.

use pgorm::{assert_models_valid, check_models, print_model_check, FromRow, Model, SchemaRegistry};

// ============================================
// Define models
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

// Model with wrong columns (for testing)
#[derive(Debug, FromRow, Model)]
#[orm(table = "users")]
#[allow(dead_code)]
struct UserWithWrongColumns {
    #[orm(id)]
    id: i64,
    name: String,
    phone: String,   // doesn't exist
    address: String, // doesn't exist
}

// Model with non-existent table
#[derive(Debug, FromRow, Model)]
#[orm(table = "nonexistent_table")]
#[allow(dead_code)]
struct NonExistentModel {
    #[orm(id)]
    id: i64,
    data: String,
}

fn main() {
    println!("=== Model Schema Check with Macros ===\n");

    // Create registry with correct schema
    let mut registry = SchemaRegistry::new();
    registry.register::<User>();
    registry.register::<Order>();

    // ============================================
    // Method 1: print_model_check! - prints results
    // ============================================
    println!("--- Using print_model_check! ---\n");

    // Check valid models
    let all_valid = print_model_check!(registry, User, Order);
    println!("\nAll valid: {}\n", all_valid);

    // Check including invalid models
    println!("--- Including invalid models ---\n");
    let all_valid = print_model_check!(registry, User, Order, UserWithWrongColumns, NonExistentModel);
    println!("\nAll valid: {}\n", all_valid);

    // ============================================
    // Method 2: check_models! - returns results
    // ============================================
    println!("--- Using check_models! ---\n");

    let results = check_models!(registry, User, Order, UserWithWrongColumns);

    for (name, issues) in &results {
        if issues.is_empty() {
            println!("  {} - OK", name);
        } else {
            let count: usize = issues.values().map(|v| v.len()).sum();
            println!("  {} - {} issues", name, count);
        }
    }

    // ============================================
    // Method 3: assert_models_valid! - panics on error
    // ============================================
    println!("\n--- Using assert_models_valid! ---\n");

    // This will succeed
    println!("Checking valid models...");
    assert_models_valid!(registry, User, Order);
    println!("✓ Valid models passed!\n");

    // Uncomment to see panic:
    // println!("Checking with invalid model (will panic)...");
    // assert_models_valid!(registry, User, UserWithWrongColumns);

    println!("=== Done ===");
}
