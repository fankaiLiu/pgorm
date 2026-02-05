//! `#[derive(PgEnum)]` — map a Rust enum to a PostgreSQL ENUM type.

use heck::ToSnakeCase;
use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Expr, ExprLit, Fields, Lit, Meta, Result};

pub fn expand(input: DeriveInput) -> Result<TokenStream> {
    let name = &input.ident;

    // Must be an enum
    let variants = match &input.data {
        Data::Enum(e) => &e.variants,
        _ => {
            return Err(syn::Error::new_spanned(
                &input,
                "PgEnum can only be derived for enums",
            ));
        }
    };

    // Parse struct-level #[orm(pg_type = "...")] attribute
    let pg_type = parse_pg_type(&input)?;

    // Build variant ↔ string mappings
    let mut to_sql_arms = Vec::new();
    let mut from_sql_arms = Vec::new();

    for variant in variants {
        // Ensure unit variant (no fields)
        if !matches!(&variant.fields, Fields::Unit) {
            return Err(syn::Error::new_spanned(
                variant,
                "PgEnum variants must be unit variants (no fields)",
            ));
        }

        let variant_ident = &variant.ident;
        let sql_name =
            parse_rename(variant)?.unwrap_or_else(|| variant_ident.to_string().to_snake_case());

        to_sql_arms.push(quote! {
            #name::#variant_ident => #sql_name,
        });
        from_sql_arms.push(quote! {
            #sql_name => Ok(#name::#variant_ident),
        });
    }

    let pg_type_array = format!("{pg_type}[]");

    let expanded = quote! {
        impl ::tokio_postgres::types::ToSql for #name {
            fn to_sql(
                &self,
                ty: &::tokio_postgres::types::Type,
                out: &mut ::bytes::BytesMut,
            ) -> ::std::result::Result<
                ::tokio_postgres::types::IsNull,
                ::std::boxed::Box<dyn ::std::error::Error + ::std::marker::Sync + ::std::marker::Send>,
            > {
                let s: &str = match self {
                    #(#to_sql_arms)*
                };
                s.to_sql(ty, out)
            }

            fn accepts(ty: &::tokio_postgres::types::Type) -> bool {
                ty.name() == #pg_type
                    || *ty == ::tokio_postgres::types::Type::TEXT
                    || *ty == ::tokio_postgres::types::Type::VARCHAR
            }

            ::tokio_postgres::types::to_sql_checked!();
        }

        impl<'__pgorm_a> ::tokio_postgres::types::FromSql<'__pgorm_a> for #name {
            fn from_sql(
                ty: &::tokio_postgres::types::Type,
                raw: &'__pgorm_a [u8],
            ) -> ::std::result::Result<
                Self,
                ::std::boxed::Box<dyn ::std::error::Error + ::std::marker::Sync + ::std::marker::Send>,
            > {
                let s = <&str as ::tokio_postgres::types::FromSql>::from_sql(ty, raw)?;
                match s {
                    #(#from_sql_arms)*
                    other => Err(::std::format!(
                        "unknown {} variant: {:?}",
                        #pg_type,
                        other
                    ).into()),
                }
            }

            fn accepts(ty: &::tokio_postgres::types::Type) -> bool {
                ty.name() == #pg_type
                    || *ty == ::tokio_postgres::types::Type::TEXT
                    || *ty == ::tokio_postgres::types::Type::VARCHAR
            }
        }

        impl ::pgorm::PgType for #name {
            fn pg_array_type() -> &'static str {
                #pg_type_array
            }
        }
    };

    Ok(expanded)
}

/// Parse `#[orm(pg_type = "...")]` from the derive input attributes.
fn parse_pg_type(input: &DeriveInput) -> Result<String> {
    for attr in &input.attrs {
        if !attr.path().is_ident("orm") {
            continue;
        }
        let nested = attr.parse_args_with(
            syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated,
        )?;
        for meta in &nested {
            if let Meta::NameValue(nv) = meta {
                if nv.path.is_ident("pg_type") {
                    if let Expr::Lit(ExprLit {
                        lit: Lit::Str(s), ..
                    }) = &nv.value
                    {
                        return Ok(s.value());
                    }
                }
            }
        }
    }
    Err(syn::Error::new_spanned(
        &input.ident,
        "PgEnum requires #[orm(pg_type = \"...\")] attribute",
    ))
}

/// Parse `#[orm(rename = "...")]` from a variant's attributes.
fn parse_rename(variant: &syn::Variant) -> Result<Option<String>> {
    for attr in &variant.attrs {
        if !attr.path().is_ident("orm") {
            continue;
        }
        let nested = attr.parse_args_with(
            syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated,
        )?;
        for meta in &nested {
            if let Meta::NameValue(nv) = meta {
                if nv.path.is_ident("rename") {
                    if let Expr::Lit(ExprLit {
                        lit: Lit::Str(s), ..
                    }) = &nv.value
                    {
                        return Ok(Some(s.value()));
                    }
                }
            }
        }
    }
    Ok(None)
}
