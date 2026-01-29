//! Example demonstrating UpdateModel derive macro
//!
//! Run with: cargo run --example update_model -p pgorm
//!
//! Set DATABASE_URL in .env file or environment variable:
//! DATABASE_URL=postgres://postgres:postgres@localhost/pgorm_example

use colored::Colorize;
use comfy_table::{Attribute, Cell, Color, ContentArrangement, Table, presets::UTF8_FULL};
use pgorm::{FromRow, InsertModel, Model, OrmError, UpdateModel, create_pool};
use std::env;

// ============================================
// Model definitions
// ============================================

/// The main Product model (used for SELECT queries)
#[derive(Debug, FromRow, Model)]
#[orm(table = "products")]
#[allow(dead_code)]
struct Product {
    #[orm(id)]
    id: i64,
    name: String,
    description: Option<String>,
    price_cents: i64,
    in_stock: bool,
}

/// InsertModel for creating new products
#[derive(Debug, InsertModel)]
#[orm(table = "products", returning = "Product")]
struct NewProduct {
    name: String,
    description: Option<String>,
    price_cents: i64,
    in_stock: bool,
}

/// UpdateModel for partial updates (patch-style)
///
/// Key features:
/// - `Option<T>`: Some(v) updates the field, None skips the field
/// - `Option<Option<T>>`: Some(Some(v)) sets value, Some(None) sets NULL, None skips
/// - `#[orm(skip_update)]`: field is never included in UPDATE
/// - `#[orm(default)]`: sets field to DEFAULT value
#[derive(Debug, UpdateModel)]
#[orm(table = "products", model = "Product", returning = "Product")]
struct ProductPatch {
    /// Update name (None = skip, Some(v) = update)
    name: Option<String>,
    /// Update description (None = skip, Some(None) = set NULL, Some(Some(v)) = set value)
    description: Option<Option<String>>,
    /// Update price (None = skip, Some(v) = update)
    price_cents: Option<i64>,
    /// Update stock status (None = skip, Some(v) = update)
    in_stock: Option<bool>,
}

// ============================================
// Helper functions
// ============================================

fn print_header(title: &str) {
    println!();
    println!("{}", "─".repeat(70).bright_black());
    println!("{}", title.bold().cyan());
    println!("{}", "─".repeat(70).bright_black());
}

fn print_success(msg: &str) {
    println!("  {} {}", "✓".green().bold(), msg);
}

fn print_info(msg: &str) {
    println!("  {} {}", "ℹ".blue(), msg);
}

fn create_products_table(products: &[Product]) -> Table {
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec![
            Cell::new("ID")
                .add_attribute(Attribute::Bold)
                .fg(Color::Cyan),
            Cell::new("Name")
                .add_attribute(Attribute::Bold)
                .fg(Color::Cyan),
            Cell::new("Description")
                .add_attribute(Attribute::Bold)
                .fg(Color::Cyan),
            Cell::new("Price")
                .add_attribute(Attribute::Bold)
                .fg(Color::Cyan),
            Cell::new("In Stock")
                .add_attribute(Attribute::Bold)
                .fg(Color::Cyan),
        ]);

    for p in products {
        let price = format!("${:.2}", p.price_cents as f64 / 100.0);
        let stock_status = if p.in_stock { "Yes" } else { "No" };
        let stock_color = if p.in_stock { Color::Green } else { Color::Red };

        table.add_row(vec![
            Cell::new(p.id.to_string()).fg(Color::Yellow),
            Cell::new(&p.name).fg(Color::White),
            Cell::new(p.description.as_deref().unwrap_or("(null)")).fg(Color::DarkGrey),
            Cell::new(&price).fg(Color::Green),
            Cell::new(stock_status).fg(stock_color),
        ]);
    }

    table
}

#[tokio::main]
async fn main() -> Result<(), OrmError> {
    dotenvy::dotenv().ok();

    println!();
    println!(
        "{}",
        "╔══════════════════════════════════════════════════════════════════════╗".cyan()
    );
    println!(
        "{}",
        "║           UpdateModel Demo - Partial Updates (Patch Style)          ║"
            .cyan()
            .bold()
    );
    println!(
        "{}",
        "╚══════════════════════════════════════════════════════════════════════╝".cyan()
    );

    let database_url =
        env::var("DATABASE_URL").expect("DATABASE_URL must be set in .env or environment");

    let pool = create_pool(&database_url)?;
    let client = pool.get().await?;

    // ============================================
    // Setup: Create table and insert test data
    // ============================================
    print_header("Setup: Creating Table and Test Data");

    client
        .execute("DROP TABLE IF EXISTS products CASCADE", &[])
        .await
        .map_err(OrmError::from_db_error)?;

    client
        .execute(
            "CREATE TABLE products (
                id BIGSERIAL PRIMARY KEY,
                name TEXT NOT NULL,
                description TEXT,
                price_cents BIGINT NOT NULL,
                in_stock BOOLEAN NOT NULL DEFAULT true
            )",
            &[],
        )
        .await
        .map_err(OrmError::from_db_error)?;

    // Insert test products one by one using insert_returning
    let p1 = NewProduct {
        name: "Laptop".to_string(),
        description: Some("High-performance laptop".to_string()),
        price_cents: 99999,
        in_stock: true,
    }
    .insert_returning(&client)
    .await?;

    let p2 = NewProduct {
        name: "Mouse".to_string(),
        description: Some("Wireless mouse".to_string()),
        price_cents: 2999,
        in_stock: true,
    }
    .insert_returning(&client)
    .await?;

    let p3 = NewProduct {
        name: "Keyboard".to_string(),
        description: None,
        price_cents: 7999,
        in_stock: false,
    }
    .insert_returning(&client)
    .await?;

    print_success(&format!(
        "Inserted 3 products (IDs: {}, {}, {})",
        p1.id, p2.id, p3.id
    ));

    println!();
    println!("{}", "Initial state:".bold());
    let all_products = Product::select_all(&client).await?;
    println!("{}", create_products_table(&all_products));

    // ============================================
    // Example 1: Update a single field
    // ============================================
    print_header("1. Update Single Field (price only)");

    print_info("Updating Laptop price from $999.99 to $899.99");

    let patch = ProductPatch {
        name: None,        // skip
        description: None, // skip
        price_cents: Some(89999),
        in_stock: None, // skip
    };

    let affected = patch.update_by_id(&client, 1_i64).await?;
    print_success(&format!("Updated {} row(s)", affected));

    println!();
    let all_products = Product::select_all(&client).await?;
    println!("{}", create_products_table(&all_products));

    // ============================================
    // Example 2: Update multiple fields
    // ============================================
    print_header("2. Update Multiple Fields");

    print_info("Updating Mouse: new name, price, and stock status");

    let patch = ProductPatch {
        name: Some("Gaming Mouse Pro".to_string()),
        description: None, // keep existing
        price_cents: Some(4999),
        in_stock: Some(true),
    };

    let affected = patch.update_by_id(&client, 2_i64).await?;
    print_success(&format!("Updated {} row(s)", affected));

    println!();
    let all_products = Product::select_all(&client).await?;
    println!("{}", create_products_table(&all_products));

    // ============================================
    // Example 3: Set field to NULL using Option<Option<T>>
    // ============================================
    print_header("3. Set Field to NULL (Option<Option<T>>)");

    print_info("Setting Laptop description to NULL");
    print_info("Using Some(None) to explicitly set NULL");

    let patch = ProductPatch {
        name: None,
        description: Some(None), // explicitly set to NULL
        price_cents: None,
        in_stock: None,
    };

    let affected = patch.update_by_id(&client, 1_i64).await?;
    print_success(&format!("Updated {} row(s)", affected));

    println!();
    let all_products = Product::select_all(&client).await?;
    println!("{}", create_products_table(&all_products));

    // ============================================
    // Example 4: Set field value using Option<Option<T>>
    // ============================================
    print_header("4. Set Field Value (Option<Option<T>>)");

    print_info("Setting Keyboard description using Some(Some(value))");

    let patch = ProductPatch {
        name: None,
        description: Some(Some("Mechanical RGB keyboard".to_string())),
        price_cents: None,
        in_stock: Some(true), // also mark as in stock
    };

    let affected = patch.update_by_id(&client, 3_i64).await?;
    print_success(&format!("Updated {} row(s)", affected));

    println!();
    let all_products = Product::select_all(&client).await?;
    println!("{}", create_products_table(&all_products));

    // ============================================
    // Example 5: Update multiple rows at once
    // ============================================
    print_header("5. Bulk Update (update_by_ids)");

    print_info("Applying same price to products 1 and 2");

    let patch = ProductPatch {
        name: None,
        description: None,
        price_cents: Some(79999), // set same price
        in_stock: None,
    };

    let affected = patch.update_by_ids(&client, vec![1_i64, 2_i64]).await?;
    print_success(&format!("Updated {} row(s)", affected));

    println!();
    let all_products = Product::select_all(&client).await?;
    println!("{}", create_products_table(&all_products));

    // ============================================
    // Example 6: Update with RETURNING
    // ============================================
    print_header("6. Update with RETURNING");

    print_info("Updating and returning the modified row");

    let patch = ProductPatch {
        name: Some("Ultra Laptop".to_string()),
        description: Some(Some("Premium ultra-thin laptop".to_string())),
        price_cents: Some(129999),
        in_stock: None,
    };

    let updated_product = patch.update_by_id_returning(&client, 1_i64).await?;
    print_success("Updated and retrieved product in one query");

    println!();
    println!("{}", "Updated product:".bold());
    println!("{}", create_products_table(&[updated_product]));

    // ============================================
    // Example 7: Bulk update with RETURNING
    // ============================================
    print_header("7. Bulk Update with RETURNING");

    print_info("Marking all products as out of stock and returning updated rows");

    let patch = ProductPatch {
        name: None,
        description: None,
        price_cents: None,
        in_stock: Some(false),
    };

    let updated_products = patch
        .update_by_ids_returning(&client, vec![1_i64, 2_i64, 3_i64])
        .await?;
    print_success(&format!(
        "Updated and retrieved {} products",
        updated_products.len()
    ));

    println!();
    println!("{}", "Updated products:".bold());
    println!("{}", create_products_table(&updated_products));

    // ============================================
    // Summary
    // ============================================
    print_header("Summary: UpdateModel Field Types");

    let mut summary_table = Table::new();
    summary_table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec![
            Cell::new("Field Type")
                .add_attribute(Attribute::Bold)
                .fg(Color::Cyan),
            Cell::new("Value")
                .add_attribute(Attribute::Bold)
                .fg(Color::Cyan),
            Cell::new("Behavior")
                .add_attribute(Attribute::Bold)
                .fg(Color::Cyan),
        ]);

    summary_table.add_row(vec![
        Cell::new("Option<T>").fg(Color::Green),
        Cell::new("None"),
        Cell::new("Skip field (no update)"),
    ]);
    summary_table.add_row(vec![
        Cell::new("Option<T>").fg(Color::Green),
        Cell::new("Some(v)"),
        Cell::new("Update field to v"),
    ]);
    summary_table.add_row(vec![
        Cell::new("Option<Option<T>>").fg(Color::Yellow),
        Cell::new("None"),
        Cell::new("Skip field (no update)"),
    ]);
    summary_table.add_row(vec![
        Cell::new("Option<Option<T>>").fg(Color::Yellow),
        Cell::new("Some(None)"),
        Cell::new("Set field to NULL"),
    ]);
    summary_table.add_row(vec![
        Cell::new("Option<Option<T>>").fg(Color::Yellow),
        Cell::new("Some(Some(v))"),
        Cell::new("Update field to v"),
    ]);
    summary_table.add_row(vec![
        Cell::new("T (non-optional)").fg(Color::Magenta),
        Cell::new("value"),
        Cell::new("Always update field"),
    ]);
    summary_table.add_row(vec![
        Cell::new("#[orm(default)]").fg(Color::Blue),
        Cell::new("-"),
        Cell::new("Set field to DEFAULT"),
    ]);
    summary_table.add_row(vec![
        Cell::new("#[orm(skip_update)]").fg(Color::Red),
        Cell::new("-"),
        Cell::new("Never include in UPDATE"),
    ]);

    println!();
    println!("{}", summary_table);

    println!();
    println!("{}", "═".repeat(70).cyan());
    println!("{}", "  Demo completed successfully!".green().bold());
    println!("{}", "═".repeat(70).cyan());
    println!();

    Ok(())
}
