//! Example demonstrating optimistic locking with `#[orm(version)]`
//!
//! Run with: cargo run --example optimistic_locking -p pgorm
//!
//! Set DATABASE_URL in .env file or environment variable:
//! DATABASE_URL=postgres://postgres:postgres@localhost/pgorm_example

mod common;

use colored::Colorize;
use comfy_table::{Attribute, Cell, Color, ContentArrangement, Table, presets::UTF8_FULL};
use common::{
    print_banner, print_done, print_header, print_info, print_success, print_warning,
    setup_articles_schema,
};
use pgorm::{FromRow, InsertModel, Model, OrmError, UpdateModel, create_pool};
use std::env;

// ============================================
// Model definitions
// ============================================

/// The main Article model
#[derive(Debug, FromRow, Model)]
#[orm(table = "articles")]
#[allow(dead_code)]
struct Article {
    #[orm(id)]
    id: i64,
    title: String,
    body: String,
    version: i32,
}

/// InsertModel for creating new articles
#[derive(Debug, InsertModel)]
#[orm(table = "articles", returning = "Article")]
struct NewArticle {
    title: String,
    body: String,
}

/// UpdateModel with `#[orm(version)]` for optimistic locking
///
/// When updating:
/// - The current version is checked in the WHERE clause: `WHERE id = $1 AND version = $2`
/// - The version is auto-incremented in the SET clause: `SET version = version + 1`
/// - If version mismatch (concurrent modification), returns `OrmError::StaleRecord`
#[derive(Debug, UpdateModel)]
#[orm(table = "articles", model = "Article", returning = "Article")]
#[allow(dead_code)]
struct ArticlePatch {
    title: Option<String>,
    body: Option<String>,
    #[orm(version)]
    version: i32,
}

// ============================================
// Helper functions
// ============================================

fn create_articles_table(articles: &[Article]) -> Table {
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec![
            Cell::new("ID")
                .add_attribute(Attribute::Bold)
                .fg(Color::Cyan),
            Cell::new("Title")
                .add_attribute(Attribute::Bold)
                .fg(Color::Cyan),
            Cell::new("Body")
                .add_attribute(Attribute::Bold)
                .fg(Color::Cyan),
            Cell::new("Version")
                .add_attribute(Attribute::Bold)
                .fg(Color::Cyan),
        ]);

    for a in articles {
        table.add_row(vec![
            Cell::new(a.id.to_string()).fg(Color::Yellow),
            Cell::new(&a.title).fg(Color::White),
            Cell::new(&a.body).fg(Color::DarkGrey),
            Cell::new(a.version.to_string()).fg(Color::Magenta),
        ]);
    }

    table
}

#[tokio::main]
async fn main() -> Result<(), OrmError> {
    dotenvy::dotenv().ok();

    print_banner("Optimistic Locking Demo - #[orm(version)]");

    let database_url =
        env::var("DATABASE_URL").expect("DATABASE_URL must be set in .env or environment");

    let pool = create_pool(&database_url)?;
    let client = pool.get().await?;

    // ============================================
    // Setup: Create table and insert test data
    // ============================================
    print_header("Setup: Creating Table and Test Data");

    setup_articles_schema(&client).await?;

    let a1 = NewArticle {
        title: "Introduction to Rust".to_string(),
        body: "Rust is a systems programming language...".to_string(),
    }
    .insert_returning(&client)
    .await?;

    let a2 = NewArticle {
        title: "PostgreSQL Tips".to_string(),
        body: "Use EXPLAIN ANALYZE to debug queries...".to_string(),
    }
    .insert_returning(&client)
    .await?;

    print_success(&format!("Inserted 2 articles (IDs: {}, {})", a1.id, a2.id));
    print_info(&format!("Initial version for all articles: {}", a1.version));

    println!();
    println!("{}", "Initial state:".bold());
    let all = Article::select_all(&client).await?;
    println!("{}", create_articles_table(&all));

    // ============================================
    // Example 1: Successful version-checked update
    // ============================================
    print_header("1. Successful Version-Checked Update");

    print_info("Updating article 1 with current version (0)");
    print_info(
        "Generated SQL: UPDATE articles SET title=$1, version=version+1 WHERE id=$2 AND version=$3",
    );

    let patch = ArticlePatch {
        title: Some("Introduction to Rust (2nd Edition)".to_string()),
        body: None,
        version: a1.version, // pass current version (0)
    };

    let affected = patch.update_by_id(&client, a1.id).await?;
    print_success(&format!("Updated {affected} row(s) - version is now 1"));

    println!();
    let all = Article::select_all(&client).await?;
    println!("{}", create_articles_table(&all));

    // ============================================
    // Example 2: Version conflict detection (StaleRecord)
    // ============================================
    print_header("2. Version Conflict Detection (StaleRecord)");

    print_info("Simulating concurrent modification:");
    print_info("  User A reads article (version=1)");
    print_info("  User B updates article (version becomes 2)");
    print_info("  User A tries to update with stale version (1)");

    // User B's update succeeds first
    let user_b_patch = ArticlePatch {
        title: Some("Introduction to Rust (3rd Edition)".to_string()),
        body: None,
        version: 1, // current version
    };
    user_b_patch.update_by_id(&client, a1.id).await?;
    print_success("User B: Updated successfully (version 1 -> 2)");

    // User A's update with stale version should fail
    let user_a_patch = ArticlePatch {
        title: Some("Introduction to Rust (User A Edition)".to_string()),
        body: None,
        version: 1, // stale version!
    };

    match user_a_patch.update_by_id(&client, a1.id).await {
        Ok(_) => {
            print_warning("Unexpected success - this should have failed!");
        }
        Err(OrmError::StaleRecord {
            table,
            expected_version,
            ..
        }) => {
            print_warning(&format!(
                "User A: StaleRecord error! Table '{table}', expected version {expected_version}",
            ));
            print_info("The record was modified by another user. Re-fetch and retry.");
        }
        Err(e) => return Err(e),
    }

    println!();
    let all = Article::select_all(&client).await?;
    println!("{}", create_articles_table(&all));

    // ============================================
    // Example 3: Update with RETURNING (get new version)
    // ============================================
    print_header("3. Update with RETURNING (Get New Version)");

    print_info("Using update_by_id_returning to get the updated row back");

    let patch = ArticlePatch {
        title: None,
        body: Some(
            "Rust is a modern systems programming language focused on safety...".to_string(),
        ),
        version: 2, // current version after User B's update
    };

    let updated = patch.update_by_id_returning(&client, a1.id).await?;
    print_success(&format!(
        "Updated and returned: version {} -> {}",
        2, updated.version
    ));

    println!();
    println!("{}", "Updated article:".bold());
    println!("{}", create_articles_table(&[updated]));

    // ============================================
    // Example 4: Force update (skip version check)
    // ============================================
    print_header("4. Force Update (Skip Version Check)");

    print_info("Admin override: using update_by_id_force to bypass version check");
    print_info("Version is still incremented, but no WHERE version=N check");

    let admin_patch = ArticlePatch {
        title: Some("Introduction to Rust (Admin Override)".to_string()),
        body: None,
        version: 0, // this value is ignored for force updates
    };

    let affected = admin_patch.update_by_id_force(&client, a1.id).await?;
    print_success(&format!("Force updated {affected} row(s)"));

    println!();
    let all = Article::select_all(&client).await?;
    println!("{}", create_articles_table(&all));

    // ============================================
    // Example 5: Force update with RETURNING
    // ============================================
    print_header("5. Force Update with RETURNING");

    print_info("Admin force update that returns the modified row");

    let admin_patch = ArticlePatch {
        title: Some("PostgreSQL Tips & Tricks".to_string()),
        body: None,
        version: 0, // ignored
    };

    let updated = admin_patch
        .update_by_id_force_returning(&client, a2.id)
        .await?;
    print_success(&format!(
        "Force updated and returned article {}: version is now {}",
        updated.id, updated.version
    ));

    println!();
    println!("{}", "Updated article:".bold());
    println!("{}", create_articles_table(&[updated]));

    // ============================================
    // Example 6: Retry pattern after StaleRecord
    // ============================================
    print_header("6. Retry Pattern After StaleRecord");

    print_info("Demonstrating the recommended retry pattern");

    let target_id = a2.id;
    let max_retries = 3;

    for attempt in 1..=max_retries {
        // Re-fetch the latest version
        let current: Article = pgorm::query("SELECT * FROM articles WHERE id = $1")
            .bind(target_id)
            .fetch_one_as::<Article>(&client)
            .await?;

        print_info(&format!(
            "Attempt {attempt}: fetched version {}",
            current.version
        ));

        let patch = ArticlePatch {
            title: Some(format!("PostgreSQL Tips (attempt {attempt})")),
            body: None,
            version: current.version,
        };

        match patch.update_by_id_returning(&client, target_id).await {
            Ok(updated) => {
                print_success(&format!(
                    "Success on attempt {attempt}! Version: {} -> {}",
                    current.version, updated.version
                ));
                break;
            }
            Err(OrmError::StaleRecord { .. }) => {
                print_warning(&format!(
                    "Attempt {attempt} failed: version conflict, retrying..."
                ));
            }
            Err(e) => return Err(e),
        }
    }

    // ============================================
    // Final state
    // ============================================
    print_header("Final State");

    let all = Article::select_all(&client).await?;
    println!("{}", create_articles_table(&all));

    // ============================================
    // Summary
    // ============================================
    print_header("Summary: Optimistic Locking API");

    let mut summary_table = Table::new();
    summary_table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec![
            Cell::new("Method")
                .add_attribute(Attribute::Bold)
                .fg(Color::Cyan),
            Cell::new("Version Check")
                .add_attribute(Attribute::Bold)
                .fg(Color::Cyan),
            Cell::new("Description")
                .add_attribute(Attribute::Bold)
                .fg(Color::Cyan),
        ]);

    summary_table.add_row(vec![
        Cell::new("update_by_id").fg(Color::Green),
        Cell::new("Yes").fg(Color::Green),
        Cell::new("Update with version check, returns affected rows"),
    ]);
    summary_table.add_row(vec![
        Cell::new("update_by_id_returning").fg(Color::Green),
        Cell::new("Yes").fg(Color::Green),
        Cell::new("Update with version check, returns updated row"),
    ]);
    summary_table.add_row(vec![
        Cell::new("update_by_id_force").fg(Color::Yellow),
        Cell::new("No").fg(Color::Red),
        Cell::new("Skip version check (admin override)"),
    ]);
    summary_table.add_row(vec![
        Cell::new("update_by_id_force_returning").fg(Color::Yellow),
        Cell::new("No").fg(Color::Red),
        Cell::new("Skip version check, returns updated row"),
    ]);
    summary_table.add_row(vec![
        Cell::new("update_by_ids").fg(Color::DarkGrey),
        Cell::new("No").fg(Color::Red),
        Cell::new("Bulk updates do not support version checking"),
    ]);

    println!();
    println!("{summary_table}");

    print_done();

    Ok(())
}
