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
    BindField, determine_conflict_spec, generate_insert_many_method, generate_insert_method,
    generate_insert_sql, generate_returning_methods, generate_upsert_methods,
};
use gen_graph::{InsertSqlInfo, generate_insert_graph_methods};
use setters::generate_with_setters;

use crate::common::syn_types::{detect_auto_timestamp_type, option_inner};

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
                auto_now_add: None,
            });
        }

        if field_attrs.skip_insert || field_attrs.is_id {
            continue;
        }

        insert_columns.push(column_name.clone());

        if field_attrs.default {
            insert_value_exprs.push("DEFAULT".to_string());
        } else {
            // Validate auto_now_add type if specified
            let auto_now_add = if field_attrs.auto_now_add {
                let ts_kind = detect_auto_timestamp_type(&field_ty).ok_or_else(|| {
                    syn::Error::new_spanned(
                        field,
                        "auto_now_add requires Option<DateTime<Utc>> or Option<NaiveDateTime>",
                    )
                })?;
                Some(ts_kind)
            } else {
                None
            };

            param_idx += 1;
            insert_value_exprs.push(format!("${}", param_idx));
            bind_field_idents.push(field_ident.clone());
            batch_bind_fields.push(BindField {
                ident: field_ident,
                ty: field_ty,
                column: column_name,
                auto_now_add,
            });
        }
    }

    let table_name = &struct_attrs.table;
    let insert_sql = generate_insert_sql(table_name, &insert_columns, &insert_value_exprs);

    // Generate methods
    let insert_method = generate_insert_method(&insert_sql, &batch_bind_fields);
    let insert_many_method = generate_insert_many_method(table_name, &batch_bind_fields);

    // Generate upsert methods if conflict spec is available
    let upsert_methods =
        if let Some(conflict_spec) = determine_conflict_spec(&struct_attrs, id_field.as_ref()) {
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

    let input_struct = if let Some(cfg) = &struct_attrs.input {
        generate_input_struct(
            &input,
            fields,
            cfg,
        )?
    } else {
        quote! {}
    };

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

        #input_struct
    })
}

fn generate_input_struct(
    input: &DeriveInput,
    fields: &syn::punctuated::Punctuated<syn::Field, syn::Token![,]>,
    cfg: &attrs::InputConfig,
) -> Result<TokenStream> {
    let model_ident = &input.ident;
    let generics = &input.generics;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let input_ident = &cfg.name;
    let vis = &cfg.vis;

    let mut input_fields: Vec<TokenStream> = Vec::new();
    let mut validate_stmts: Vec<TokenStream> = Vec::new();
    let mut build_field_stmts: Vec<TokenStream> = Vec::new();

    for field in fields {
        let field_ident = field.ident.clone().expect("named fields");
        let field_name = field_ident.to_string();
        let field_name_lit = syn::LitStr::new(&field_name, field_ident.span());
        let field_ty = field.ty.clone();
        let field_attrs = attrs::get_field_attrs(field)?;

        if field_attrs.skip_input {
            if option_inner(&field_ty).is_none() {
                return Err(syn::Error::new_spanned(
                    field,
                    "skip_input requires an Option<T> field",
                ));
            }
            // Fill skipped Option<T> fields with None during conversion.
            build_field_stmts.push(quote! { #field_ident: None });
            continue;
        }

        // Determine input field type: if already Option<...>, keep; otherwise wrap in Option<...>.
        let input_ty: syn::Type = if let Some(inner) = option_inner(&field_ty) {
            if let Some(input_as) = field_attrs.input_as.as_ref() {
                if option_inner(inner).is_some() {
                    return Err(syn::Error::new_spanned(
                        field,
                        "input_as is not supported on Option<Option<T>> fields",
                    ));
                }
                syn::parse_quote!(Option<#input_as>)
            } else {
                field_ty.clone()
            }
        } else {
            let inner_ty = field_attrs.input_as.clone().unwrap_or_else(|| field_ty.clone());
            syn::parse_quote!(Option<#inner_ty>)
        };

        input_fields.push(quote! { pub #field_ident: #input_ty });

        // Helper: extract Option<&Inner> for validations (handles Option<Option<T>>).
        let value_expr = if option_inner(&field_ty).and_then(option_inner).is_some() {
            quote! { self.#field_ident.as_ref().and_then(|v| v.as_ref()) }
        } else {
            quote! { self.#field_ident.as_ref() }
        };

        // required: auto-infer for non-Option fields (except #[orm(default)] / #[orm(skip_insert)] / #[orm(id)]).
        let inferred_required = option_inner(&field_ty).is_none()
            && !field_attrs.default
            && !field_attrs.skip_insert
            && !field_attrs.is_id;
        let required = field_attrs.required || inferred_required;
        if required {
            validate_stmts.push(quote! {
                if (#value_expr).is_none() {
                    errors.push(::pgorm::changeset::ValidationError::new(
                        #field_name_lit,
                        ::pgorm::changeset::ValidationCode::Required,
                        "is required",
                    ));
                }
            });
        }

        if let Some(spec) = field_attrs.len.as_ref() {
            let (min, max) = parse_min_max(spec, field)?;
            validate_stmts.push(quote! {
                if let Some(v) = #value_expr {
                    let s: &str = v.as_ref();
                    let n = s.len();
                    let min = #min;
                    let max = #max;
                    if n < min || n > max {
                        errors.push(::pgorm::changeset::ValidationError::new(
                            #field_name_lit,
                            ::pgorm::changeset::ValidationCode::Len,
                            "has invalid length",
                        ));
                    }
                }
            });
        }

        if let Some(spec) = field_attrs.range.as_ref() {
            let (min, max) = parse_min_max(spec, field)?;
            validate_stmts.push(quote! {
                if let Some(v) = #value_expr {
                    let min = #min;
                    let max = #max;
                    if *v < min || *v > max {
                        errors.push(::pgorm::changeset::ValidationError::new(
                            #field_name_lit,
                            ::pgorm::changeset::ValidationCode::Range,
                            "is out of range",
                        ));
                    }
                }
            });
        }

        if field_attrs.email {
            validate_stmts.push(quote! {
                if let Some(v) = #value_expr {
                    if !::pgorm::validate::is_email(v.as_ref()) {
                        errors.push(::pgorm::changeset::ValidationError::new(
                            #field_name_lit,
                            ::pgorm::changeset::ValidationCode::Email,
                            "has invalid email format",
                        ));
                    }
                }
            });
        }

        if let Some(pattern) = field_attrs.regex.as_ref() {
            let pat_lit = syn::LitStr::new(pattern, field_ident.span());
            validate_stmts.push(quote! {
                if let Some(v) = #value_expr {
                    if !::pgorm::validate::regex_is_match(#pat_lit, v.as_ref()) {
                        errors.push(::pgorm::changeset::ValidationError::new(
                            #field_name_lit,
                            ::pgorm::changeset::ValidationCode::Regex,
                            "has invalid format",
                        ));
                    }
                }
            });
        }

        if field_attrs.url {
            validate_stmts.push(quote! {
                if let Some(v) = #value_expr {
                    if !::pgorm::validate::is_url(v.as_ref()) {
                        errors.push(::pgorm::changeset::ValidationError::new(
                            #field_name_lit,
                            ::pgorm::changeset::ValidationCode::Url,
                            "is not a valid url",
                        ));
                    }
                }
            });
        }

        if field_attrs.uuid {
            validate_stmts.push(quote! {
                if let Some(v) = #value_expr {
                    if !::pgorm::validate::is_uuid(v.as_ref()) {
                        errors.push(::pgorm::changeset::ValidationError::new(
                            #field_name_lit,
                            ::pgorm::changeset::ValidationCode::Uuid,
                            "is not a valid uuid",
                        ));
                    }
                }
            });
        }

        if let Some(spec) = field_attrs.one_of.as_ref() {
            let allowed = split_one_of(spec, field)?;
            validate_stmts.push(quote! {
                if let Some(v) = #value_expr {
                    const __PGORM_ALLOWED: &[&str] = &[#(#allowed),*];
                    if !__PGORM_ALLOWED.contains(&v.as_ref()) {
                        errors.push(::pgorm::changeset::ValidationError::new(
                            #field_name_lit,
                            ::pgorm::changeset::ValidationCode::OneOf,
                            "is not an allowed value",
                        ));
                    }
                }
            });
        }

        if let Some(path) = field_attrs.custom.as_ref() {
            validate_stmts.push(quote! {
                if let Err(msg) = #path(&self.#field_ident) {
                    errors.push(::pgorm::changeset::ValidationError::new(
                        #field_name_lit,
                        ::pgorm::changeset::ValidationCode::Custom("custom".to_string()),
                        msg,
                    ));
                }
            });
        }

        // Build output field for InsertModel (conversion).
        let output_expr = build_output_expr(&field_ident, &field_name_lit, &field_ty, &field_attrs)?;
        build_field_stmts.push(quote! { #field_ident: #output_expr });
    }

    Ok(quote! {
        #[derive(Debug, Clone, Default, ::pgorm::serde::Deserialize)]
        #[serde(crate = "::pgorm::serde")]
        #vis struct #input_ident #generics {
            #(#input_fields,)*
        }

        impl #impl_generics #input_ident #ty_generics #where_clause {
            pub fn validate(&self) -> ::pgorm::changeset::ValidationErrors {
                let mut errors = ::pgorm::changeset::ValidationErrors::default();
                #(#validate_stmts)*
                errors
            }

            pub fn try_into_model(self) -> ::core::result::Result<#model_ident #ty_generics, ::pgorm::changeset::ValidationErrors> {
                let mut errors = self.validate();
                if !errors.is_empty() {
                    return Err(errors);
                }

                Ok(#model_ident {
                    #(#build_field_stmts,)*
                })
            }
        }
    })
}

fn parse_min_max(spec: &str, field: &syn::Field) -> Result<(syn::Expr, syn::Expr)> {
    let (min, max) = spec
        .split_once("..=")
        .ok_or_else(|| syn::Error::new_spanned(field, "expected range like \"min..=max\""))?;
    let min_expr: syn::Expr = syn::parse_str(min.trim())
        .map_err(|e| syn::Error::new_spanned(field, format!("invalid min expr: {e}")))?;
    let max_expr: syn::Expr = syn::parse_str(max.trim())
        .map_err(|e| syn::Error::new_spanned(field, format!("invalid max expr: {e}")))?;
    Ok((min_expr, max_expr))
}

fn split_one_of(spec: &str, field: &syn::Field) -> Result<Vec<syn::LitStr>> {
    let mut out = Vec::new();
    for part in spec.split('|').map(str::trim).filter(|s| !s.is_empty()) {
        out.push(syn::LitStr::new(part, proc_macro2::Span::call_site()));
    }
    if out.is_empty() {
        return Err(syn::Error::new_spanned(field, "one_of must not be empty"));
    }
    Ok(out)
}

fn build_output_expr(
    field_ident: &syn::Ident,
    field_name_lit: &syn::LitStr,
    field_ty: &syn::Type,
    field_attrs: &attrs::FieldAttrs,
) -> Result<TokenStream> {
    let is_opt = option_inner(field_ty).is_some();
    let needs_default = field_attrs.default || field_attrs.skip_insert || field_attrs.is_id;

    // input_as conversions for uuid/url, to allow ValidationErrors instead of serde errors.
    if field_attrs.input_as.is_some() {
        if is_uuid_type(field_ty) {
            if is_opt {
                return Ok(quote! {
                    match self.#field_ident {
                        Some(s) => match ::pgorm::validate::parse_uuid(s.as_ref()) {
                            Ok(v) => Some(v),
                            Err(_) => {
                                let mut errors = ::pgorm::changeset::ValidationErrors::default();
                                errors.push(::pgorm::changeset::ValidationError::new(
                                    #field_name_lit,
                                    ::pgorm::changeset::ValidationCode::Uuid,
                                    "is not a valid uuid",
                                ));
                                return Err(errors);
                            }
                        },
                        None => None,
                    }
                });
            }
            return Ok(quote! {
                match self.#field_ident {
                    Some(s) => match ::pgorm::validate::parse_uuid(s.as_ref()) {
                        Ok(v) => v,
                        Err(_) => {
                            let mut errors = ::pgorm::changeset::ValidationErrors::default();
                            errors.push(::pgorm::changeset::ValidationError::new(
                                #field_name_lit,
                                ::pgorm::changeset::ValidationCode::Uuid,
                                "is not a valid uuid",
                            ));
                            return Err(errors);
                        }
                    },
                    None => {
                        let mut errors = ::pgorm::changeset::ValidationErrors::default();
                        errors.push(::pgorm::changeset::ValidationError::new(
                            #field_name_lit,
                            ::pgorm::changeset::ValidationCode::Required,
                            "is required",
                        ));
                        return Err(errors);
                    }
                }
            });
        }
        if is_url_type(field_ty) {
            if is_opt {
                return Ok(quote! {
                    match self.#field_ident {
                        Some(s) => match ::pgorm::validate::parse_url(s.as_ref()) {
                            Ok(v) => Some(v),
                            Err(_) => {
                                let mut errors = ::pgorm::changeset::ValidationErrors::default();
                                errors.push(::pgorm::changeset::ValidationError::new(
                                    #field_name_lit,
                                    ::pgorm::changeset::ValidationCode::Url,
                                    "is not a valid url",
                                ));
                                return Err(errors);
                            }
                        },
                        None => None,
                    }
                });
            }
            return Ok(quote! {
                match self.#field_ident {
                    Some(s) => match ::pgorm::validate::parse_url(s.as_ref()) {
                        Ok(v) => v,
                        Err(_) => {
                            let mut errors = ::pgorm::changeset::ValidationErrors::default();
                            errors.push(::pgorm::changeset::ValidationError::new(
                                #field_name_lit,
                                ::pgorm::changeset::ValidationCode::Url,
                                "is not a valid url",
                            ));
                            return Err(errors);
                        }
                    },
                    None => {
                        let mut errors = ::pgorm::changeset::ValidationErrors::default();
                        errors.push(::pgorm::changeset::ValidationError::new(
                            #field_name_lit,
                            ::pgorm::changeset::ValidationCode::Required,
                            "is required",
                        ));
                        return Err(errors);
                    }
                }
            });
        }

        return Err(syn::Error::new_spanned(
            field_ident,
            "input_as currently only supports uuid::Uuid and url::Url fields",
        ));
    }

    if is_opt {
        return Ok(quote! { self.#field_ident });
    }

    if needs_default {
        return Ok(quote! { self.#field_ident.unwrap_or_default() });
    }

    Ok(quote! {
        match self.#field_ident {
            Some(v) => v,
            None => {
                let mut errors = ::pgorm::changeset::ValidationErrors::default();
                errors.push(::pgorm::changeset::ValidationError::new(
                    #field_name_lit,
                    ::pgorm::changeset::ValidationCode::Required,
                    "is required",
                ));
                return Err(errors);
            }
        }
    })
}

fn is_uuid_type(ty: &syn::Type) -> bool {
    let ty = option_inner(ty).unwrap_or(ty);
    let syn::Type::Path(p) = ty else { return false };
    p.qself.is_none() && p.path.segments.last().is_some_and(|s| s.ident == "Uuid")
}

fn is_url_type(ty: &syn::Type) -> bool {
    let ty = option_inner(ty).unwrap_or(ty);
    let syn::Type::Path(p) = ty else { return false };
    p.qself.is_none() && p.path.segments.last().is_some_and(|s| s.ident == "Url")
}
