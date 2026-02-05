//! SQL schema checking and linting utilities.
//!
//! This module provides runtime SQL validation against registered table schemas.
//! It can detect issues like missing tables, missing columns, dangerous operations, etc.
//!
//! `SchemaRegistry::check_sql` uses an internal LRU parse cache (via `pgorm-check`) to reduce
//! repeated `pg_query` parsing overhead.
//!
//! # Example
//!
//! ```ignore
//! use pgorm::check::{SchemaRegistry, TableMeta, lint_sql, LintLevel};
//!
//! // Register tables
//! let mut registry = SchemaRegistry::new();
//! registry.register::<User>();
//! registry.register::<Order>();
//!
//! // Check SQL against schema
//! let issues = registry.check_sql("SELECT id, name FROM users");
//! assert!(issues.is_empty());
//!
//! // Lint SQL for common issues
//! let result = lint_sql("DELETE FROM users");
//! assert!(result.has_errors()); // Missing WHERE clause
//! ```

#[cfg(feature = "check")]
mod lint;
mod registry;

#[cfg(test)]
mod tests;

pub use registry::{
    ColumnMeta, SchemaIssue, SchemaIssueKind, SchemaIssueLevel, SchemaRegistry, TableMeta,
    TableSchema,
};

#[cfg(feature = "check")]
pub use lint::*;

// Re-export CheckedClient here so it's available at pgorm::check::CheckedClient
#[cfg(feature = "check")]
pub use crate::checked_client::CheckedClient;

/// Check multiple models against a schema registry and return results.
///
/// This macro simplifies batch model validation by automatically checking
/// each model's generated SQL against the registry.
///
/// # Example
///
/// ```ignore
/// use pgorm::{check_models, SchemaRegistry, Model, FromRow};
///
/// #[derive(Debug, FromRow, Model)]
/// #[orm(table = "users")]
/// struct User { #[orm(id)] id: i64, name: String }
///
/// #[derive(Debug, FromRow, Model)]
/// #[orm(table = "orders")]
/// struct Order { #[orm(id)] id: i64, user_id: i64 }
///
/// let registry = SchemaRegistry::new();
/// // ... register tables ...
///
/// // Check all models at once
/// let results = check_models!(registry, User, Order);
/// for (name, issues) in &results {
///     if issues.is_empty() {
///         println!("✓ {}", name);
///     } else {
///         println!("✗ {} ({} issues)", name, issues.len());
///     }
/// }
/// ```
#[macro_export]
macro_rules! check_models {
    ($registry:expr, $($model:ty),+ $(,)?) => {{
        let mut results: Vec<(&'static str, std::collections::HashMap<&'static str, Vec<$crate::check::SchemaIssue>>)> = Vec::new();
        $(
            results.push((stringify!($model), <$model>::check_schema(&$registry)));
        )+
        results
    }};
}

/// Check models and panic if any have schema issues.
///
/// Useful for startup validation to catch schema mismatches early.
///
/// # Example
///
/// ```ignore
/// use pgorm::{assert_models_valid, SchemaRegistry, Model, FromRow};
///
/// #[derive(Debug, FromRow, Model)]
/// #[orm(table = "users")]
/// struct User { #[orm(id)] id: i64, name: String }
///
/// let registry = SchemaRegistry::new();
/// // ... register tables ...
///
/// // Panics if any model has schema issues
/// assert_models_valid!(registry, User);
/// ```
#[macro_export]
macro_rules! assert_models_valid {
    ($registry:expr, $($model:ty),+ $(,)?) => {{
        let mut all_issues: Vec<(&'static str, Vec<String>)> = Vec::new();
        $(
            let issues = <$model>::check_schema(&$registry);
            if !issues.is_empty() {
                let messages: Vec<String> = issues
                    .iter()
                    .flat_map(|(sql_name, issue_list)| {
                        issue_list.iter().map(move |i| format!("{}: {}", sql_name, i.message))
                    })
                    .collect();
                all_issues.push((stringify!($model), messages));
            }
        )+
        if !all_issues.is_empty() {
            let mut msg = String::from("Schema validation failed:\n");
            for (model, issues) in &all_issues {
                msg.push_str(&format!("\n{}:\n", model));
                for issue in issues {
                    msg.push_str(&format!("  - {}\n", issue));
                }
            }
            panic!("{}", msg);
        }
    }};
}

/// Print schema check results for multiple models.
///
/// A convenient macro for debugging and validation output.
///
/// # Example
///
/// ```ignore
/// use pgorm::{print_model_check, SchemaRegistry, Model, FromRow};
///
/// #[derive(Debug, FromRow, Model)]
/// #[orm(table = "users")]
/// struct User { #[orm(id)] id: i64, name: String }
///
/// let registry = SchemaRegistry::new();
/// // ... register tables ...
///
/// // Prints validation results
/// print_model_check!(registry, User);
/// // Output:
/// // Model Schema Validation:
/// //   ✓ User
/// ```
#[macro_export]
macro_rules! print_model_check {
    ($registry:expr, $($model:ty),+ $(,)?) => {{
        println!("Model Schema Validation:");
        let mut all_valid = true;
        $(
            let issues = <$model>::check_schema(&$registry);
            if issues.is_empty() {
                println!("  ✓ {}", stringify!($model));
            } else {
                all_valid = false;
                let total: usize = issues.values().map(|v| v.len()).sum();
                println!("  ✗ {} ({} issues)", stringify!($model), total);
                for (sql_name, issue_list) in &issues {
                    for issue in issue_list {
                        println!("      {}: {:?} - {}", sql_name, issue.kind, issue.message);
                    }
                }
            }
        )+
        all_valid
    }};
}

/// Check models against the actual database schema.
///
/// This macro loads the schema from the database and checks each model
/// to ensure its columns match the actual table structure.
///
/// # Example
///
/// ```ignore
/// use pgorm::{check_models_db, PgClient, Model, FromRow};
///
/// #[derive(Debug, FromRow, Model)]
/// #[orm(table = "users")]
/// struct User { #[orm(id)] id: i64, name: String }
///
/// let pg = PgClient::new(client);
///
/// // Check models against actual database
/// let results = check_models_db!(pg, User, Order).await?;
/// for result in &results {
///     result.print();
/// }
/// ```
#[macro_export]
macro_rules! check_models_db {
    ($client:expr, $($model:ty),+ $(,)?) => {{
        async {
            let db_schema = $client.load_db_schema().await?;
            let mut results: Vec<$crate::ModelCheckResult> = Vec::new();
            $(
                results.push($crate::ModelCheckResult::check::<$model>(&db_schema));
            )+
            Ok::<_, $crate::OrmError>(results)
        }
    }};
}

/// Check models against database and print results.
///
/// # Example
///
/// ```ignore
/// use pgorm::{print_models_db_check, PgClient, Model, FromRow};
///
/// #[derive(Debug, FromRow, Model)]
/// #[orm(table = "users")]
/// struct User { #[orm(id)] id: i64, name: String }
///
/// let pg = PgClient::new(client);
///
/// // Check and print results
/// let all_valid = print_models_db_check!(pg, User, Order).await?;
/// ```
#[macro_export]
macro_rules! print_models_db_check {
    ($client:expr, $($model:ty),+ $(,)?) => {{
        async {
            let db_schema = $client.load_db_schema().await?;
            println!("Model Database Validation:");
            let mut all_valid = true;
            $(
                let result = $crate::ModelCheckResult::check::<$model>(&db_schema);
                if !result.is_valid() {
                    all_valid = false;
                }
                result.print();
            )+
            Ok::<_, $crate::OrmError>(all_valid)
        }
    }};
}

/// Assert that all models match the database schema, panic if not.
///
/// # Example
///
/// ```ignore
/// use pgorm::{assert_models_db_valid, PgClient, Model, FromRow};
///
/// #[derive(Debug, FromRow, Model)]
/// #[orm(table = "users")]
/// struct User { #[orm(id)] id: i64, name: String }
///
/// let pg = PgClient::new(client);
///
/// // Panics if any model doesn't match the database
/// assert_models_db_valid!(pg, User, Order).await?;
/// ```
#[macro_export]
macro_rules! assert_models_db_valid {
    ($client:expr, $($model:ty),+ $(,)?) => {{
        async {
            let db_schema = $client.load_db_schema().await?;
            let mut errors: Vec<String> = Vec::new();
            $(
                let result = $crate::ModelCheckResult::check::<$model>(&db_schema);
                if !result.table_found {
                    errors.push(format!("{}: table '{}' not found", result.model, result.table));
                } else if !result.missing_in_db.is_empty() {
                    errors.push(format!("{}: missing columns {:?}", result.model, result.missing_in_db));
                }
            )+
            if !errors.is_empty() {
                panic!("Schema validation failed:\n  {}", errors.join("\n  "));
            }
            Ok::<_, $crate::OrmError>(())
        }
    }};
}
