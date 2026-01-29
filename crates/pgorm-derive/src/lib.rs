//! Derive macros for pgorm
//!
//! Provides `#[derive(FromRow)]` and `#[derive(Model)]` macros.

use proc_macro::TokenStream;
use syn::{DeriveInput, parse_macro_input};

mod common;
mod from_row;
mod insert_model;
mod model;
mod sql_ident;
mod update_model;

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
/// Struct-level:
///
/// - `#[orm(table = "name")]` - Specify table name (required)
/// - `#[orm(join(table = "...", on = "...", type = "inner|left|right|full|cross"))]` - Add JOINs (optional, repeatable)
/// - `#[orm(has_many(ChildType, foreign_key = "...", as = "..."))]` - Generate select_has_many helpers (optional, repeatable)
/// - `#[orm(belongs_to(ParentType, foreign_key = "...", as = "..."))]` - Generate select_belongs_to helpers (optional, repeatable)
///
/// Field-level:
///
/// - `#[orm(id)]` - Mark field as primary key
/// - `#[orm(column = "name")]` - Map field to a different column name
/// - `#[orm(table = "name")]` - Mark field as coming from a joined table (for view/join models)
#[proc_macro_derive(Model, attributes(orm))]
pub fn derive_model(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    model::expand(input)
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}

/// Derive `ViewModel` metadata for a struct.
///
/// This is an alias of `Model` intended to express that the type is a read/view model
/// (optionally including JOINs), while write models are derived separately.
#[proc_macro_derive(ViewModel, attributes(orm))]
pub fn derive_view_model(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    model::expand(input)
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}

/// Derive `InsertModel` helpers for inserting into a table.
///
/// # Attributes
///
/// Struct-level:
///
/// - `#[orm(table = "name")]` - Specify table name (required)
/// - `#[orm(returning = "TypePath")]` - Enable `insert_returning` helpers (optional)
/// - Conflict handling (Postgres `ON CONFLICT`):
///   - `#[orm(conflict_target = "col1,col2")]` - conflict target columns (optional)
///   - `#[orm(conflict_constraint = "constraint_name")]` - conflict constraint (optional)
///   - `#[orm(conflict_update = "col1,col2")]` - columns to update on conflict (optional)
/// - Multi-table write graphs (advanced): function-style attrs like `#[orm(has_many(...))]`,
///   `#[orm(belongs_to(...))]`, `#[orm(before_insert(...))]`. See `docs/design/multi-table-writes-final.md`.
///
/// Field-level:
///
/// - `#[orm(id)]` - Mark field as primary key (optional)
/// - `#[orm(skip_insert)]` - Never include this field in INSERT
/// - `#[orm(default)]` - Use SQL `DEFAULT` for this field
/// - `#[orm(auto_now_add)]` - Use `NOW()` for this field on insert
/// - `#[orm(column = "name")]` / `#[orm(table = "name")]` - Override column/table mapping (optional)
#[proc_macro_derive(InsertModel, attributes(orm))]
pub fn derive_insert_model(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    insert_model::expand(input)
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}

/// Derive `UpdateModel` helpers for updating a table (patch-style).
///
/// # Attributes
///
/// Struct-level:
///
/// - `#[orm(table = "name")]` - Specify table name (required)
/// - One of:
///   - `#[orm(id_column = "id")]` - Explicit primary key column
///   - `#[orm(model = "TypePath")]` - Derive primary key column from a `Model`
///   - `#[orm(returning = "TypePath")]` where `TypePath::ID` exists
/// - `#[orm(returning = "TypePath")]` - Enable `update_by_id_returning` helpers (optional)
/// - Multi-table write graphs (advanced): see `docs/design/multi-table-writes-final.md`.
///
/// Field-level:
///
/// - `#[orm(skip_update)]` - Never include this field in UPDATE
/// - `#[orm(default)]` - Use SQL `DEFAULT` for this field
/// - `#[orm(auto_now)]` - Use `NOW()` for this field on update
/// - `#[orm(column = "name")]` / `#[orm(table = "name")]` - Override column/table mapping (optional)
#[proc_macro_derive(UpdateModel, attributes(orm))]
pub fn derive_update_model(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    update_model::expand(input)
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}
