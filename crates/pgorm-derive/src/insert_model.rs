//! InsertModel derive macro implementation
//!
//! This module provides the `#[derive(InsertModel)]` macro for generating
//! INSERT, UPSERT, and graph-write methods for Rust structs.
//!
//! ## Module Structure
//!
//! - `attrs`: Struct and field attribute parsing
//! - `graph_decl`: Graph declaration data structures
//! - `graph_parse`: Graph declaration parsing
//! - `setters`: `with_*` setter generation and type helpers
//! - `gen_base`: Base insert/upsert/returning code generation
//! - `gen_graph`: Graph insert methods code generation

mod attrs;
mod gen_base;
mod gen_graph;
mod graph_decl;
mod graph_parse;
mod setters;

use attrs::{get_field_attrs, get_struct_attrs};
use gen_base::{
    determine_conflict_spec, generate_insert_many_method, generate_insert_method,
    generate_insert_sql, generate_returning_methods, generate_upsert_methods, BindField,
};
use gen_graph::{generate_insert_graph_methods, InsertSqlInfo};
use setters::generate_with_setters;

use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, Result};

pub fn expand(input: DeriveInput) -> Result<TokenStream> {
    let name = &input.ident;
    let generics = &input.generics;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let struct_attrs = get_struct_attrs(&input)?;

    let fields = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(fields) => &fields.named,
            _ => {
                return Err(syn::Error::new_spanned(
                    &input,
                    "InsertModel can only be derived for structs with named fields",
                ));
            }
        },
        _ => {
            return Err(syn::Error::new_spanned(
                &input,
                "InsertModel can only be derived for structs",
            ));
        }
    };

    let mut insert_columns: Vec<String> = Vec::new();
    let mut insert_value_exprs: Vec<String> = Vec::new();
    let mut bind_field_idents: Vec<syn::Ident> = Vec::new();
    let mut batch_bind_fields: Vec<BindField> = Vec::new();
    let mut id_field: Option<BindField> = None;

    // Get field names used by graph declarations (should not be inserted into main table)
    let graph_field_names = struct_attrs.graph.graph_field_names();

    let mut param_idx = 0_usize;

    for field in fields.iter() {
        let field_ident = field.ident.clone().unwrap();
        let field_name = field_ident.to_string();
        let field_ty = field.ty.clone();

        // Skip fields used by graph declarations
        if graph_field_names.contains(&field_name) {
            continue;
        }

        let field_attrs = get_field_attrs(field)?;

        if let Some(field_table) = &field_attrs.table {
            if field_table != &struct_attrs.table {
                return Err(syn::Error::new_spanned(
                    field,
                    "InsertModel does not support fields from joined/other tables",
                ));
            }
        }

        let column_name = field_attrs
            .column
            .clone()
            .unwrap_or_else(|| field_name.clone());

        if field_attrs.is_id {
            if id_field.is_some() {
                return Err(syn::Error::new_spanned(
                    field,
                    "InsertModel supports only one #[orm(id)] field",
                ));
            }
            id_field = Some(BindField {
                ident: field_ident.clone(),
                ty: field_ty.clone(),
                column: column_name.clone(),
            });
        }

        if field_attrs.skip_insert || field_attrs.is_id {
            continue;
        }

        insert_columns.push(column_name.clone());

        if field_attrs.default {
            insert_value_exprs.push("DEFAULT".to_string());
        } else {
            param_idx += 1;
            insert_value_exprs.push(format!("${}", param_idx));
            bind_field_idents.push(field_ident.clone());
            batch_bind_fields.push(BindField {
                ident: field_ident,
                ty: field_ty,
                column: column_name,
            });
        }
    }

    let table_name = &struct_attrs.table;
    let insert_sql = generate_insert_sql(table_name, &insert_columns, &insert_value_exprs);

    // Generate methods
    let insert_method = generate_insert_method(&insert_sql, &bind_field_idents);
    let insert_many_method = generate_insert_many_method(table_name, &batch_bind_fields);

    // Generate upsert methods if conflict spec is available
    let upsert_methods = if let Some(conflict_spec) =
        determine_conflict_spec(&struct_attrs, id_field.as_ref())
    {
        generate_upsert_methods(
            table_name,
            &struct_attrs,
            &conflict_spec,
            &batch_bind_fields,
            id_field.as_ref(),
        )
    } else {
        quote! {}
    };

    // Generate returning methods
    let returning_method = generate_returning_methods(
        table_name,
        &struct_attrs,
        &insert_sql,
        &bind_field_idents,
        &batch_bind_fields,
    );

    // Generate with_* setters for all fields
    let with_setters = generate_with_setters(fields);

    // Create insert SQL info for graph methods
    let insert_sql_info = InsertSqlInfo {
        sql: insert_sql.clone(),
        bind_idents: bind_field_idents.clone(),
    };

    // Generate insert_graph methods
    let insert_graph_methods =
        generate_insert_graph_methods(&struct_attrs, &input, fields, &insert_sql_info)?;

    Ok(quote! {
        impl #impl_generics #name #ty_generics #where_clause {
            pub const TABLE: &'static str = #table_name;

            #insert_method

            #insert_many_method

            #upsert_methods

            #returning_method

            #insert_graph_methods

            #with_setters
        }
    })
}
