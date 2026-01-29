//! UpdateModel derive macro implementation
//!
//! This module provides the `#[derive(UpdateModel)]` macro for generating
//! UPDATE and graph-write methods for Rust structs.
//!
//! ## Module Structure
//!
//! - `attrs`: Struct and field attribute parsing
//! - `graph_decl`: Graph declaration data structures
//! - `graph_parse`: Graph declaration parsing
//! - `types`: Type helper functions (option_inner)
//! - `gen_base`: Base update_by_id/update_by_ids code generation
//! - `gen_children`: has_many/has_one child update code generation
//! - `gen_graph`: Graph update methods code generation

mod attrs;
mod gen_base;
mod gen_children;
mod gen_graph;
mod graph_decl;
mod graph_parse;
mod types;

use attrs::{get_field_attrs, get_struct_attrs};
use gen_base::{generate_update_by_id_methods, generate_update_returning_methods};
use gen_graph::generate_update_graph_methods;
use types::{detect_auto_timestamp_type, option_inner, AutoTimestampKind};

use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, Result};

pub fn expand(input: DeriveInput) -> Result<TokenStream> {
    let name = &input.ident;
    let generics = &input.generics;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let attrs = get_struct_attrs(&input)?;

    let fields = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(fields) => &fields.named,
            _ => {
                return Err(syn::Error::new_spanned(
                    &input,
                    "UpdateModel can only be derived for structs with named fields",
                ));
            }
        },
        _ => {
            return Err(syn::Error::new_spanned(
                &input,
                "UpdateModel can only be derived for structs",
            ));
        }
    };

    let id_col_expr = if let Some(model_ty) = attrs.model.as_ref() {
        quote! { #model_ty::ID }
    } else if let Some(id_col) = attrs.id_column.as_ref() {
        quote! { #id_col }
    } else if let Some(returning_ty) = attrs.returning.as_ref() {
        quote! { #returning_ty::ID }
    } else {
        return Err(syn::Error::new_spanned(
            &input,
            "UpdateModel requires #[orm(id_column = \"...\")] or #[orm(model = \"...\")] (or a returning type with ID)",
        ));
    };

    let mut destructure_idents: Vec<syn::Ident> = Vec::new();
    let mut set_stmts: Vec<TokenStream> = Vec::new();
    let mut has_auto_now = false;

    // Get field names used by graph declarations
    let graph_field_names = attrs.graph.graph_field_names();

    for field in fields.iter() {
        let field_ident = field.ident.clone().unwrap();
        let field_name = field_ident.to_string();
        let field_ty = &field.ty;

        // Skip fields used by graph declarations
        if graph_field_names.contains(&field_name) {
            continue;
        }

        let field_attrs = get_field_attrs(field)?;

        if let Some(field_table) = &field_attrs.table {
            if field_table != &attrs.table {
                return Err(syn::Error::new_spanned(
                    field,
                    "UpdateModel does not support fields from joined/other tables",
                ));
            }
        }

        if field_attrs.skip_update {
            continue;
        }

        let column_name = field_attrs.column.unwrap_or(field_name);

        if field_attrs.default {
            set_stmts.push(quote! {
                if !first {
                    q.push(", ");
                } else {
                    first = false;
                }
                q.push(#column_name);
                q.push(" = DEFAULT");
            });
            continue;
        }

        // Handle auto_now fields
        if field_attrs.auto_now {
            // Validate type is Option<DateTime<Utc>> or Option<NaiveDateTime>
            let ts_kind = detect_auto_timestamp_type(field_ty).ok_or_else(|| {
                syn::Error::new_spanned(
                    field,
                    "auto_now requires Option<DateTime<Utc>> or Option<NaiveDateTime>",
                )
            })?;

            has_auto_now = true;
            destructure_idents.push(field_ident.clone());

            // auto_now fields always participate in SET
            let bind_expr = match ts_kind {
                AutoTimestampKind::DateTimeUtc => {
                    quote! { #field_ident.unwrap_or(__pgorm_now) }
                }
                AutoTimestampKind::NaiveDateTime => {
                    quote! { #field_ident.unwrap_or_else(|| __pgorm_now.naive_utc()) }
                }
            };

            set_stmts.push(quote! {
                if !first {
                    q.push(", ");
                } else {
                    first = false;
                }
                q.push(#column_name);
                q.push(" = ");
                q.push_bind(#bind_expr);
            });
            continue;
        }

        // Non-default fields need the value.
        destructure_idents.push(field_ident.clone());

        if let Some(inner) = option_inner(field_ty) {
            if option_inner(inner).is_some() {
                // Option<Option<T>>: Some(Some(v)) => bind; Some(None) => NULL; None => skip.
                set_stmts.push(quote! {
                    if let Some(v) = #field_ident {
                        if !first {
                            q.push(", ");
                        } else {
                            first = false;
                        }
                        q.push(#column_name);
                        q.push(" = ");
                        match v {
                            Some(vv) => {
                                q.push_bind(vv);
                            }
                            None => {
                                q.push("NULL");
                            }
                        }
                    }
                });
            } else {
                // Option<T>: Some(v) => bind; None => skip.
                set_stmts.push(quote! {
                    if let Some(v) = #field_ident {
                        if !first {
                            q.push(", ");
                        } else {
                            first = false;
                        }
                        q.push(#column_name);
                        q.push(" = ");
                        q.push_bind(v);
                    }
                });
            }
        } else {
            // T: always bind.
            set_stmts.push(quote! {
                if !first {
                    q.push(", ");
                } else {
                    first = false;
                }
                q.push(#column_name);
                q.push(" = ");
                q.push_bind(#field_ident);
            });
        }
    }

    let table_name = &attrs.table;

    let destructure = if destructure_idents.is_empty() {
        quote! { let _ = self; }
    } else {
        quote! { let Self { #(#destructure_idents),*, .. } = self; }
    };

    // Generate methods using submodules
    let update_by_id_methods =
        generate_update_by_id_methods(table_name, &id_col_expr, &destructure, &set_stmts, has_auto_now);

    let update_returning_methods = generate_update_returning_methods(
        &attrs,
        table_name,
        &id_col_expr,
        &destructure,
        &set_stmts,
        has_auto_now,
    );

    let update_graph_methods = generate_update_graph_methods(&attrs, &id_col_expr)?;

    Ok(quote! {
        impl #impl_generics #name #ty_generics #where_clause {
            pub const TABLE: &'static str = #table_name;

            #update_by_id_methods

            #update_returning_methods

            #update_graph_methods
        }
    })
}
