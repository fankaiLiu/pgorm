//! Join clause handling for Model derive macro.
//!
//! Parses `#[orm(join(table = "...", on = "...", type = "..."))]` attributes.

use proc_macro2::Span;
use syn::ext::IdentExt;
use syn::{DeriveInput, Result};

/// Represents a JOIN clause
pub(super) struct JoinClause {
    /// The table to join
    pub table: String,
    /// The ON condition
    pub on: String,
    /// Join type: inner, left, right, full
    pub join_type: String,
}

/// Helper struct for parsing join attribute
struct JoinAttr {
    table: String,
    on: String,
    join_type: String,
}

impl syn::parse::Parse for JoinAttr {
    fn parse(input: syn::parse::ParseStream) -> Result<Self> {
        let ident: syn::Ident = input.parse()?;
        if ident != "join" {
            return Err(syn::Error::new(ident.span(), "expected join"));
        }

        let content;
        syn::parenthesized!(content in input);

        let mut table: Option<String> = None;
        let mut on: Option<String> = None;
        let mut join_type = "inner".to_string();

        // Parse key = value pairs
        loop {
            if content.is_empty() {
                break;
            }

            let key = syn::Ident::parse_any(&content)?;
            let _: syn::Token![=] = content.parse()?;
            let value: syn::LitStr = content.parse()?;

            if key == "table" {
                table = Some(value.value());
            } else if key == "on" {
                on = Some(value.value());
            } else if key == "type" {
                join_type = value.value();
            }

            if content.peek(syn::Token![,]) {
                let _: syn::Token![,] = content.parse()?;
            } else {
                break;
            }
        }

        let table = table
            .ok_or_else(|| syn::Error::new(Span::call_site(), "join requires table = \"...\""))?;

        let on =
            on.ok_or_else(|| syn::Error::new(Span::call_site(), "join requires on = \"...\""))?;

        Ok(JoinAttr {
            table,
            on,
            join_type,
        })
    }
}

/// Parse join clauses from struct attributes.
///
/// Example: `#[orm(join(table = "categories", on = "products.category_id = categories.id", type = "inner"))]`
pub(super) fn get_join_clauses(input: &DeriveInput) -> Result<Vec<JoinClause>> {
    let mut joins = Vec::new();

    for attr in &input.attrs {
        if attr.path().is_ident("orm") {
            if let syn::Meta::List(meta_list) = &attr.meta {
                let tokens = meta_list.tokens.clone();
                if let Ok(parsed) = syn::parse2::<JoinAttr>(tokens) {
                    joins.push(JoinClause {
                        table: parsed.table,
                        on: parsed.on,
                        join_type: parsed.join_type,
                    });
                }
            }
        }
    }

    Ok(joins)
}
