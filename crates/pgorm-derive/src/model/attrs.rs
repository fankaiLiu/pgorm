//! Attribute parsing for Model derive macro.
//!
//! Handles parsing of struct-level and field-level `#[orm(...)]` attributes.

use syn::{DeriveInput, Result};

/// Field info with table source
pub(super) struct FieldInfo {
    /// The table this field comes from (None means main table)
    pub table: Option<String>,
    /// The column name in the database
    pub column: String,
}

/// Helper struct for parsing field attributes
pub(super) struct FieldAttr {
    pub is_id: bool,
    pub table: Option<String>,
    pub column: Option<String>,
}

impl syn::parse::Parse for FieldAttr {
    fn parse(input: syn::parse::ParseStream) -> Result<Self> {
        let mut is_id = false;
        let mut table = None;
        let mut column = None;

        // Parse comma-separated key=value pairs or single identifiers
        loop {
            if input.is_empty() {
                break;
            }

            // Check for 'id' keyword first
            if input.peek(syn::Ident) {
                let ident: syn::Ident = input.parse()?;
                if ident == "id" {
                    // Skip 'id' marker
                    is_id = true;
                    if input.peek(syn::Token![,]) {
                        let _: syn::Token![,] = input.parse()?;
                    }
                    continue;
                }
                // Otherwise it's a key = value
                let _: syn::Token![=] = input.parse()?;
                let value: syn::LitStr = input.parse()?;

                if ident == "table" {
                    table = Some(value.value());
                } else if ident == "column" {
                    column = Some(value.value());
                }
            }

            if input.peek(syn::Token![,]) {
                let _: syn::Token![,] = input.parse()?;
            } else {
                break;
            }
        }

        Ok(FieldAttr {
            is_id,
            table,
            column,
        })
    }
}

/// Extract table name from struct-level `#[orm(table = "...")]` attribute.
pub(super) fn get_table_name(input: &DeriveInput) -> Result<String> {
    for attr in &input.attrs {
        if attr.path().is_ident("orm") {
            if let Ok(nested) = attr.parse_args::<syn::MetaNameValue>() {
                if nested.path.is_ident("table") {
                    if let syn::Expr::Lit(syn::ExprLit {
                        lit: syn::Lit::Str(lit),
                        ..
                    }) = &nested.value
                    {
                        return Ok(lit.value());
                    }
                }
            }
        }
    }
    Err(syn::Error::new_spanned(
        input,
        "Model requires #[orm(table = \"table_name\")] attribute",
    ))
}

/// Get field info including table source and column name.
///
/// Supports: `#[orm(table = "categories", column = "name")]`
pub(super) fn get_field_info(field: &syn::Field, _default_table: &str) -> FieldInfo {
    let mut table: Option<String> = None;
    let mut column: Option<String> = None;

    for attr in &field.attrs {
        if attr.path().is_ident("orm") {
            // Try to parse as a list of key=value pairs
            if let syn::Meta::List(meta_list) = &attr.meta {
                let tokens = meta_list.tokens.clone();
                if let Ok(parsed) = syn::parse2::<FieldAttr>(tokens) {
                    if parsed.table.is_some() {
                        table = parsed.table;
                    }
                    if parsed.column.is_some() {
                        column = parsed.column;
                    }
                }
            }
            // Also try single key=value for backward compatibility
            if let Ok(nested) = attr.parse_args::<syn::MetaNameValue>() {
                if nested.path.is_ident("column") {
                    if let syn::Expr::Lit(syn::ExprLit {
                        lit: syn::Lit::Str(lit),
                        ..
                    }) = &nested.value
                    {
                        column = Some(lit.value());
                    }
                } else if nested.path.is_ident("table") {
                    if let syn::Expr::Lit(syn::ExprLit {
                        lit: syn::Lit::Str(lit),
                        ..
                    }) = &nested.value
                    {
                        table = Some(lit.value());
                    }
                }
            }
        }
    }

    FieldInfo {
        table,
        column: column.unwrap_or_else(|| field.ident.as_ref().unwrap().to_string()),
    }
}

/// Check if a field is marked as the primary key with `#[orm(id)]`.
pub(super) fn is_id_field(field: &syn::Field) -> bool {
    for attr in &field.attrs {
        if attr.path().is_ident("orm") {
            if let syn::Meta::List(meta_list) = &attr.meta {
                let tokens = meta_list.tokens.clone();
                if let Ok(parsed) = syn::parse2::<FieldAttr>(tokens) {
                    if parsed.is_id {
                        return true;
                    }
                }
            }
        }
    }
    false
}
