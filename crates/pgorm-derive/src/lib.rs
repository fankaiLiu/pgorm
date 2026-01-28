//! Derive macros for pgorm
//!
//! Provides `#[derive(FromRow)]` and `#[derive(Model)]` macros.

use proc_macro::TokenStream;
use syn::{parse_macro_input, DeriveInput};

mod from_row;
mod model;

/// Derive `FromRow` trait for a struct.
///
/// # Example
///
/// ```ignore
/// use pgorm::FromRow;
///
/// #[derive(FromRow)]
/// struct User {
///     id: i64,
///     username: String,
///     #[orm(column = "email_address")]
///     email: Option<String>,
/// }
/// ```
///
/// # Attributes
///
/// - `#[orm(column = "name")]` - Map field to a different column name
#[proc_macro_derive(FromRow, attributes(orm))]
pub fn derive_from_row(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    from_row::expand(input)
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}

/// Derive `Model` metadata for a struct.
///
/// # Example
///
/// ```ignore
/// use pgorm::Model;
///
/// #[derive(Model)]
/// #[orm(table = "users")]
/// struct User {
///     #[orm(id)]
///     user_id: i64,
///     username: String,
///     email: Option<String>,
/// }
/// ```
///
/// # Generated
///
/// - `TABLE: &'static str` - Table name
/// - `COL_*: &'static str` - Column name constants
/// - `SELECT_LIST: &'static str` - Comma-separated column list
/// - `fn select_list_as(alias: &str) -> String` - Aliased column list for JOINs
///
/// # Attributes
///
/// - `#[orm(table = "name")]` - Specify table name (required)
/// - `#[orm(id)]` - Mark field as primary key
/// - `#[orm(column = "name")]` - Map field to different column name
#[proc_macro_derive(Model, attributes(orm))]
pub fn derive_model(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    model::expand(input)
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}
