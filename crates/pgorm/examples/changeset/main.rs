//! Example demonstrating `#[orm(input)]` (changeset-style input + validation).
//!
//! Run with: `cargo run --example changeset_input -p pgorm`
//!
//! Set `DATABASE_URL` in `.env` or environment variable:
//! `DATABASE_URL=postgres://postgres:postgres@localhost/pgorm_example`

mod common;

use common::{print_header, setup_users_schema};
use pgorm::changeset::ValidationErrors;
use pgorm::{FromRow, InsertModel, Model, OrmError, UpdateModel, create_pool};
use std::env;

#[derive(Debug, FromRow, Model)]
#[orm(table = "users")]
#[allow(dead_code)]
struct User {
    #[orm(id)]
    id: i64,
    name: String,
    email: String,
    age: Option<i32>,
    external_id: uuid::Uuid,
    homepage: Option<String>,
    created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, InsertModel)]
#[orm(table = "users", returning = "User")]
#[orm(input)]
struct NewUser {
    #[orm(len = "2..=100")]
    name: String,

    #[orm(email)]
    email: String,

    #[orm(range = "0..=150")]
    age: Option<i32>,

    // Accept any string from input, validate as UUID, and parse into uuid::Uuid.
    #[orm(uuid, input_as = "String")]
    external_id: uuid::Uuid,

    #[orm(url)]
    homepage: Option<String>,
}

#[derive(Debug, UpdateModel)]
#[orm(table = "users", model = "User", returning = "User")]
#[orm(input)]
struct UserPatch {
    #[orm(len = "2..=100")]
    name: Option<String>,

    #[orm(email)]
    email: Option<String>,

    // tri-state:
    // - None => skip
    // - Some(None) => set NULL
    // - Some(Some(v)) => set value
    #[orm(url)]
    homepage: Option<Option<String>>,
}

fn print_errors(errs: &ValidationErrors) {
    println!("{}", serde_json::to_string_pretty(errs).unwrap());
}

#[tokio::main]
async fn main() -> Result<(), OrmError> {
    dotenvy::dotenv().ok();
    let database_url =
        env::var("DATABASE_URL").expect("DATABASE_URL must be set in .env or environment");

    let pool = create_pool(&database_url)?;
    let client = pool.get().await?;

    setup_users_schema(&client).await?;

    // ─────────────────────────────────────────────────────────────────────────
    // 1) Insert: invalid input (collect multiple field errors)
    // ─────────────────────────────────────────────────────────────────────────
    print_header("1. Insert: Invalid Input (Validation Errors)");

    let bad_json = r#"
        {
          "name": "A",
          "email": "not-an-email",
          "age": 200,
          "external_id": "not-a-uuid",
          "homepage": "not-a-url"
        }
    "#;

    let bad_input: NewUserInput = serde_json::from_str(bad_json).unwrap();
    let errs = bad_input.validate();
    if !errs.is_empty() {
        println!("\ninsert validation failed:");
        print_errors(&errs);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 2) Insert: valid input -> model -> insert_returning
    // ─────────────────────────────────────────────────────────────────────────
    print_header("2. Insert: Valid Input");

    let ok_json = r#"
        {
          "name": "Alice",
          "email": "alice@example.com",
          "age": 42,
          "external_id": "550e8400-e29b-41d4-a716-446655440000",
          "homepage": "https://example.com"
        }
    "#;

    let ok_input: NewUserInput = serde_json::from_str(ok_json).unwrap();
    let new_user = match ok_input.try_into_model() {
        Ok(v) => v,
        Err(errs) => {
            println!("\ninsert validation failed unexpectedly:");
            print_errors(&errs);
            return Ok(());
        }
    };

    let inserted: User = new_user.insert_returning(&client).await?;
    println!("\ninserted user: {inserted:?}");

    // ─────────────────────────────────────────────────────────────────────────
    // 3) Update: patch input -> patch -> update_by_id_returning
    // ─────────────────────────────────────────────────────────────────────────
    print_header("3. Update: Patch Input");

    let patch_json = r#"
        {
          "email": "bob@example.com",
          "homepage": null
        }
    "#;

    let patch_input: UserPatchInput = serde_json::from_str(patch_json).unwrap();
    let patch = match patch_input.try_into_patch() {
        Ok(v) => v,
        Err(errs) => {
            println!("\nupdate validation failed:");
            print_errors(&errs);
            return Ok(());
        }
    };

    let updated: User = patch.update_by_id_returning(&client, inserted.id).await?;
    println!("\nupdated user: {updated:?}");

    Ok(())
}
