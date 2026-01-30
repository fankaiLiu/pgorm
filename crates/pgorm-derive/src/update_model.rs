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
use types::{AutoTimestampKind, detect_auto_timestamp_type, option_inner};

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
    let update_by_id_methods = generate_update_by_id_methods(
        table_name,
        &id_col_expr,
        &destructure,
        &set_stmts,
        has_auto_now,
    );

    let update_returning_methods = generate_update_returning_methods(
        &attrs,
        table_name,
        &id_col_expr,
        &destructure,
        &set_stmts,
        has_auto_now,
    );

    let update_graph_methods = generate_update_graph_methods(&attrs, &id_col_expr)?;

    let input_struct = if let Some(cfg) = &attrs.input {
        generate_input_struct(&input, fields, cfg)?
    } else {
        quote! {}
    };

    Ok(quote! {
        impl #impl_generics #name #ty_generics #where_clause {
            pub const TABLE: &'static str = #table_name;

            #update_by_id_methods

            #update_returning_methods

            #update_graph_methods
        }

        #input_struct
    })
}

fn generate_input_struct(
    input: &DeriveInput,
    fields: &syn::punctuated::Punctuated<syn::Field, syn::Token![,]>,
    cfg: &attrs::InputConfig,
) -> Result<TokenStream> {
    let patch_ident = &input.ident;
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
            let inner_ty = field_attrs
                .input_as
                .clone()
                .unwrap_or_else(|| field_ty.clone());
            syn::parse_quote!(Option<#inner_ty>)
        };

        input_fields.push(quote! { pub #field_ident: #input_ty });

        // Helper: extract Option<&Inner> for validations (handles Option<Option<T>>).
        let value_expr = if option_inner(&field_ty).and_then(option_inner).is_some() {
            quote! { self.#field_ident.as_ref().and_then(|v| v.as_ref()) }
        } else {
            quote! { self.#field_ident.as_ref() }
        };

        // required: auto-infer for non-Option fields (except #[orm(default)] / #[orm(skip_update)]).
        let inferred_required =
            option_inner(&field_ty).is_none() && !field_attrs.default && !field_attrs.skip_update;
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

        let output_expr =
            build_output_expr(&field_ident, &field_name_lit, &field_ty, &field_attrs)?;
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

            pub fn try_into_patch(self) -> ::core::result::Result<#patch_ident #ty_generics, ::pgorm::changeset::ValidationErrors> {
                let mut errors = self.validate();
                if !errors.is_empty() {
                    return Err(errors);
                }

                Ok(#patch_ident {
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
    let needs_default = field_attrs.default || field_attrs.skip_update;

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
